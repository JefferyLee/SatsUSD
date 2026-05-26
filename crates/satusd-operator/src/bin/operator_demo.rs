//! Operator BTC-leg demo (MVP, PRD §14.6): quote a redemption, build the operator
//! BTC HTLC, fund it on devnet, and have the user claim it by revealing the
//! payment-hash preimage. The confirmed claim is exactly the spend the
//! confirmed-payout observer would SPV-prove (satusd-state::spv) before the
//! operator submits a reserve claim.
//!
//!   cargo run -p satusd-operator --bin operator_demo

use bitcoin::hashes::{sha256, Hash};
use bitcoin::key::Keypair;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey, XOnlyPublicKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::{
    absolute::LockTime, transaction::Version, Address, Amount, Network, OutPoint, Sequence,
    TapLeafHash, Transaction, TxIn, TxOut, Witness,
};
use bitcoincore_rpc::{Auth, Client, RpcApi};

type Err = Box<dyn std::error::Error>;

const PRICE_50K: u64 = 5_000_000_000_000;
const BTC_HTLC_CSV: i64 = 144;

fn det(secp: &Secp256k1<bitcoin::secp256k1::All>, label: &str) -> Keypair {
    let mut s = sha256::Hash::hash(label.as_bytes()).to_byte_array();
    loop {
        if let Ok(sk) = SecretKey::from_slice(&s) {
            return Keypair::from_secret_key(secp, &sk);
        }
        s = sha256::Hash::hash(&s).to_byte_array();
    }
}

fn main() -> Result<(), Err> {
    let secp = Secp256k1::new();
    let btc = Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        Auth::UserPass("satusd".into(), "satusd".into()),
    )?;
    let miner = btc.get_new_address(None, None)?.assume_checked();

    // ---- quote a $40 redemption at $50k/BTC, 0.50% fee ----
    let q = satusd_operator::quote(4_000, PRICE_50K, 50).map_err(|e| format!("{e:?}"))?;
    println!(
        "quote: {} atoms @ {} → gross {} sat, fee {} sat, user gets {} sat",
        q.amount_atoms, q.price_e8, q.gross_sats, q.fee_sats, q.user_payout_sats
    );

    // ---- build the operator BTC HTLC for that payout, atomic on `payment_hash` ----
    let user = det(&secp, "operator-demo-user-claim");
    let user_xonly: XOnlyPublicKey = user.x_only_public_key().0;
    let operator: XOnlyPublicKey = det(&secp, "operator-demo-op").x_only_public_key().0;
    let preimage = sha256::Hash::hash(b"operator-demo-preimage").to_byte_array();
    let payment_hash = sha256::Hash::hash(&preimage).to_byte_array();
    let htlc = satusd_operator::build_btc_htlc(&payment_hash, user_xonly, operator, BTC_HTLC_CSV);

    // ---- operator funds the HTLC with the user payout ----
    let spk = htlc.script_pubkey();
    let addr = Address::from_script(&spk, Network::Regtest)?;
    let value = Amount::from_sat(q.user_payout_sats);
    let txid = btc.send_to_address(&addr, value, None, None, None, None, None, None)?;
    btc.generate_to_address(1, &miner)?;
    let fund = btc.get_raw_transaction(&txid, None)?;
    let vout = fund
        .output
        .iter()
        .position(|o| o.script_pubkey == spk)
        .ok_or("htlc output not found")? as u32;
    let outpoint = OutPoint { txid, vout };
    println!("operator funded HTLC: {outpoint} ({} sat)", value.to_sat());

    // ---- user claims by revealing the preimage (claim leaf) ----
    let dest = btc.get_new_address(None, None)?.assume_checked();
    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: Default::default(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: value - Amount::from_sat(500),
            script_pubkey: dest.script_pubkey(),
        }],
    };
    let prevout = TxOut {
        value,
        script_pubkey: spk.clone(),
    };
    let leaf = TapLeafHash::from_script(&htlc.claim_script, LeafVersion::TapScript);
    let sighash = SighashCache::new(&tx).taproot_script_spend_signature_hash(
        0,
        &Prevouts::All(std::slice::from_ref(&prevout)),
        leaf,
        TapSighashType::Default,
    )?;
    let sig = secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &user);
    let control = htlc
        .spend_info
        .control_block(&(htlc.claim_script.clone(), LeafVersion::TapScript))
        .ok_or("no claim control block")?;
    // claim-leaf stack (bottom→top): <sig> <preimage>, then script + control block.
    let mut w = Witness::new();
    w.push(sig.serialize());
    w.push(preimage);
    w.push(htlc.claim_script.as_bytes());
    w.push(control.serialize());
    tx.input[0].witness = w;

    let claim_txid = btc.send_raw_transaction(&tx)?;
    btc.generate_to_address(1, &miner)?;
    println!("user CLAIMED with preimage: {claim_txid} (preimage now revealed on-chain)");

    // ---- observer: bury the claim, build the SPV confirmation, and check it
    //      against the state node's own verifier (operator produces → node accepts).
    const MIN_DEPTH: usize = 6;
    btc.generate_to_address(MIN_DEPTH as u64, &miner)?; // bury ≥ 6 deep
    let htlc_outpoint = satusd_types::types::OutPoint {
        txid: outpoint.txid.to_byte_array(),
        vout: outpoint.vout,
    };
    let confirmation = satusd_operator::build_payout_confirmation(
        &btc,
        &payment_hash,
        htlc_outpoint,
        claim_txid,
        MIN_DEPTH,
    )?;
    let tip = btc.get_block_count()? as u32;
    satusd_state::spv::verify_payout_confirmation(&confirmation, &payment_hash, MIN_DEPTH, tip)
        .map_err(|e| format!("state node SPV verifier rejected the confirmation: {e:?}"))?;
    println!(
        "observer built BtcPayoutConfirmation; state node SPV verifier ACCEPTED it \
         (R-07/13/14/15 all satisfied) ✓"
    );
    println!("\nOperator confirmed-payout loop closed end-to-end against live bitcoind.");
    Ok(())
}
