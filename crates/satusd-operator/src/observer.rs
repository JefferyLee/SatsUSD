//! Confirmed-payout observer (PRD §5.D14, §14.6 — DL-22 "confirmed-payout
//! observer"). Once the user claims the operator's BTC HTLC (revealing the
//! preimage), the operator watches the chain and, after the claim is buried
//! `min_depth` blocks, builds the [`BtcPayoutConfirmation`] SPV witness that the
//! state node re-verifies (`satusd_state::spv::verify_payout_confirmation`) before
//! a reserve reimbursement claim is allowed. This is the production-side dual of
//! that verifier: the operator *produces* exactly the evidence the node *checks*.

use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::{Transaction, Txid};
use bitcoincore_rpc::{Client, RpcApi};
use satusd_types::types::{BtcPayoutConfirmation, OutPoint};

type Err = Box<dyn std::error::Error>;

fn dsha256(b: &[u8]) -> [u8; 32] {
    use bitcoin::hashes::sha256d;
    sha256d::Hash::hash(b).to_byte_array()
}

/// Merkle branch (siblings) for the tx at `index` among `txids` (internal order),
/// in the form `satusd_state::spv` folds with the index for direction.
fn merkle_branch(txids: &[[u8; 32]], mut index: usize) -> Vec<[u8; 32]> {
    let mut layer = txids.to_vec();
    let mut branch = Vec::new();
    while layer.len() > 1 {
        if layer.len() % 2 == 1 {
            layer.push(*layer.last().unwrap()); // Bitcoin duplicates the last leaf
        }
        branch.push(layer[index ^ 1]);
        let mut next = Vec::with_capacity(layer.len() / 2);
        for pair in layer.chunks(2) {
            let mut buf = [0u8; 64];
            buf[..32].copy_from_slice(&pair[0]);
            buf[32..].copy_from_slice(&pair[1]);
            next.push(dsha256(&buf));
        }
        layer = next;
        index /= 2;
    }
    branch
}

/// Header (80 bytes) + height + the in-block tx index + merkle branch for a tx.
struct TxInclusion {
    header: [u8; 80],
    block_hash: [u8; 32],
    height: u32,
    tx_index: u32,
    merkle_proof: Vec<[u8; 32]>,
}

fn locate(btc: &Client, txid: &Txid) -> Result<TxInclusion, Err> {
    let info = btc.get_raw_transaction_info(txid, None)?;
    let block_hash = info.blockhash.ok_or("tx not confirmed")?;
    let block = btc.get_block(&block_hash)?;
    let hdr_info = btc.get_block_header_info(&block_hash)?;
    let txids: Vec<[u8; 32]> = block
        .txdata
        .iter()
        .map(|t| t.compute_txid().to_byte_array())
        .collect();
    let target = txid.to_byte_array();
    let tx_index = txids
        .iter()
        .position(|t| *t == target)
        .ok_or("tx not in block")?;
    Ok(TxInclusion {
        header: serialize(&block.header)
            .try_into()
            .map_err(|_| "header len")?,
        block_hash: block_hash.to_byte_array(),
        height: hdr_info.height as u32,
        tx_index: tx_index as u32,
        merkle_proof: merkle_branch(&txids, tx_index),
    })
}

/// Legacy (no-witness) serialization of a tx — its dSHA256 is the txid (R-15).
fn legacy_bytes(tx: &Transaction) -> Vec<u8> {
    let mut stripped = tx.clone();
    for i in &mut stripped.input {
        i.witness = bitcoin::Witness::new();
    }
    serialize(&stripped)
}

/// Build the SPV `BtcPayoutConfirmation` for a confirmed HTLC claim. Reads the
/// HTLC tx, the claim tx (which spends the HTLC outpoint and reveals the
/// preimage), their block inclusions, and `min_depth` confirmation headers from
/// `btc`. The result is exactly what `verify_payout_confirmation` checks.
pub fn build_payout_confirmation(
    btc: &Client,
    payment_hash: &[u8; 32],
    htlc_outpoint: OutPoint,
    claim_txid: Txid,
    min_depth: usize,
) -> Result<BtcPayoutConfirmation, Err> {
    let htlc_txid = Txid::from_byte_array(htlc_outpoint.txid);
    let htlc = locate(btc, &htlc_txid)?;
    let htlc_tx = btc.get_raw_transaction(&htlc_txid, None)?;
    let htlc_out = htlc_tx
        .output
        .get(htlc_outpoint.vout as usize)
        .ok_or("htlc vout out of range")?;
    let htlc_value_sats = htlc_out.value.to_sat();
    let htlc_spk = htlc_out.script_pubkey.to_bytes();
    let claim = locate(btc, &claim_txid)?;
    let claim_tx = btc.get_raw_transaction(&claim_txid, None)?;

    // The claim input that spends the HTLC outpoint + the revealed preimage.
    let (input_index, claim_in) = claim_tx
        .input
        .iter()
        .enumerate()
        .find(|(_, i)| {
            i.previous_output.txid == htlc_txid && i.previous_output.vout == htlc_outpoint.vout
        })
        .ok_or("claim tx does not spend the HTLC outpoint")?;
    let preimage: [u8; 32] = claim_in
        .witness
        .iter()
        .find(|w| w.len() == 32 && satusd_types::sha256(&[w]) == *payment_hash)
        .ok_or("preimage not revealed in claim witness")?
        .try_into()
        .unwrap();

    // `min_depth` headers chaining onto the claim block (the burial proof).
    let mut confirmation_headers = Vec::with_capacity(min_depth);
    let mut h = bitcoin::BlockHash::from_byte_array(claim.block_hash);
    for _ in 0..min_depth {
        h = btc.get_block_hash(btc.get_block_header_info(&h)?.height as u64 + 1)?;
        confirmation_headers.push(serialize(&btc.get_block_header(&h)?).try_into().unwrap());
    }

    Ok(BtcPayoutConfirmation {
        btc_htlc_txid: htlc_outpoint.txid,
        btc_htlc_vout: htlc_outpoint.vout,
        htlc_output_value_sats: htlc_value_sats,
        htlc_output_script: htlc_spk,
        htlc_inclusion_block_hash: htlc.block_hash,
        htlc_inclusion_block_height: htlc.height,
        htlc_inclusion_merkle_proof: htlc.merkle_proof,
        claim_spend_txid: claim_txid.to_byte_array(),
        claim_spend_input_index: input_index as u32,
        claim_spend_witness: claim_in.witness.to_vec(),
        claim_tx_legacy: legacy_bytes(&claim_tx),
        revealed_preimage: preimage,
        claim_inclusion_block_hash: claim.block_hash,
        claim_inclusion_block_height: claim.height,
        claim_inclusion_merkle_proof: claim.merkle_proof,
        confirmation_headers,
        htlc_tx_index: htlc.tx_index,
        claim_tx_index: claim.tx_index,
        htlc_inclusion_header: htlc.header,
        claim_inclusion_header: claim.header,
    })
}
