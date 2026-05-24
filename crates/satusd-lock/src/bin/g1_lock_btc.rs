//! G1 on-chain demo: build the §5.D3 Bitcoin-layer lock anchor on regtest and
//! spend it via BOTH the finalize and refund script paths, then show a tampered
//! finalize is rejected. Proves the lock's spend mechanics (key-path is
//! structurally impossible: NUMS internal key).
//!
//! Requires the devnet up (`make devnet-up`). Run:
//!   cargo run -p satusd-lock --bin g1_lock_btc

use bitcoin::hashes::{sha256, Hash};
use bitcoin::key::Keypair;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::{
    absolute::LockTime, transaction::Version, Address, Amount, Network, OutPoint, Sequence,
    TapLeafHash, Transaction, TxIn, TxOut, Txid, Witness,
};
use bitcoincore_rpc::{Auth, Client, RpcApi};

use satusd_lock::{build_lock_anchor, LockAnchor};

type Err = Box<dyn std::error::Error>;

const FEE: Amount = Amount::from_sat(2_000);
const FUND: Amount = Amount::from_sat(1_000_000);

fn rand32() -> [u8; 32] {
    rand::random()
}

fn new_keypair(secp: &Secp256k1<bitcoin::secp256k1::All>) -> Keypair {
    loop {
        if let Ok(sk) = SecretKey::from_slice(&rand32()) {
            return Keypair::from_secret_key(secp, &sk);
        }
    }
}

/// Send FUND to `addr`, confirm, and return the funding outpoint + value.
fn fund_anchor(rpc: &Client, addr: &Address, miner: &Address) -> Result<(OutPoint, Amount), Err> {
    let txid = rpc.send_to_address(addr, FUND, None, None, None, None, None, None)?;
    rpc.generate_to_address(1, miner)?;
    let tx = rpc.get_raw_transaction(&txid, None)?;
    let spk = addr.script_pubkey();
    let vout = tx
        .output
        .iter()
        .position(|o| o.script_pubkey == spk)
        .ok_or("anchor output not found")? as u32;
    Ok((OutPoint { txid, vout }, tx.output[vout as usize].value))
}

fn anchor_address(anchor: &LockAnchor) -> Address {
    Address::p2tr_tweaked(anchor.spend_info.output_key(), Network::Regtest)
}

/// Build a 1-in/1-out spend of `outpoint` to `dest` with the given CSV sequence.
fn spend_tx(outpoint: OutPoint, value: Amount, dest: &Address, csv: u16) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: Default::default(),
            sequence: Sequence::from_height(csv),
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: value - FEE,
            script_pubkey: dest.script_pubkey(),
        }],
    }
}

/// Taproot script-path schnorr signature over the given leaf.
fn sign_leaf(
    secp: &Secp256k1<bitcoin::secp256k1::All>,
    tx: &Transaction,
    prevout: &TxOut,
    leaf: &bitcoin::ScriptBuf,
    kp: &Keypair,
) -> [u8; 64] {
    let leaf_hash = TapLeafHash::from_script(leaf, LeafVersion::TapScript);
    let sighash = SighashCache::new(tx)
        .taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(std::slice::from_ref(prevout)),
            leaf_hash,
            TapSighashType::Default,
        )
        .expect("sighash");
    let msg = Message::from_digest(sighash.to_byte_array());
    secp.sign_schnorr_no_aux_rand(&msg, kp).serialize()
}

fn main() -> Result<(), Err> {
    let rpc = Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        Auth::UserPass("satusd".into(), "satusd".into()),
    )?;
    let miner = rpc.get_new_address(None, None)?.assume_checked();
    let secp = Secp256k1::new();

    let operator = new_keypair(&secp);
    let user = new_keypair(&secp);
    let op_xonly = operator.x_only_public_key().0;
    let user_xonly = user.x_only_public_key().0;

    let preimage = rand32();
    let payment_hash: [u8; 32] = sha256::Hash::hash(&preimage).to_byte_array();
    let (finalize_csv, refund_csv) = (6u16, 10u16);

    let anchor = build_lock_anchor(
        &payment_hash,
        op_xonly,
        user_xonly,
        finalize_csv as i64,
        refund_csv as i64,
    );
    let addr = anchor_address(&anchor);
    println!("lock anchor address: {addr}");
    println!(
        "  output_key = 0x{}",
        hex::encode(anchor.output_key_bytes())
    );

    let control_finalize = anchor
        .spend_info
        .control_block(&(anchor.finalize_script.clone(), LeafVersion::TapScript))
        .ok_or("no finalize control block")?;
    let control_refund = anchor
        .spend_info
        .control_block(&(anchor.refund_script.clone(), LeafVersion::TapScript))
        .ok_or("no refund control block")?;

    // ---- FINALIZE path: preimage + operator sig, after finalize_csv blocks ----
    {
        let (op, value) = fund_anchor(&rpc, &addr, &miner)?;
        rpc.generate_to_address(finalize_csv as u64, &miner)?; // mature CSV
        let dest = rpc.get_new_address(None, None)?.assume_checked();
        let mut tx = spend_tx(op, value, &dest, finalize_csv);
        let prevout = TxOut {
            value,
            script_pubkey: addr.script_pubkey(),
        };
        let sig = sign_leaf(&secp, &tx, &prevout, &anchor.finalize_script, &operator);
        let mut w = Witness::new();
        w.push(sig);
        w.push(preimage);
        w.push(anchor.finalize_script.as_bytes());
        w.push(control_finalize.serialize());
        tx.input[0].witness = w;
        let txid: Txid = rpc.send_raw_transaction(&tx)?;
        rpc.generate_to_address(1, &miner)?;
        println!("FINALIZE spend confirmed: {txid}");
    }

    // ---- REFUND path: user sig, after refund_csv blocks (fresh anchor) ----
    {
        let (op, value) = fund_anchor(&rpc, &addr, &miner)?;
        rpc.generate_to_address(refund_csv as u64, &miner)?;
        let dest = rpc.get_new_address(None, None)?.assume_checked();
        let mut tx = spend_tx(op, value, &dest, refund_csv);
        let prevout = TxOut {
            value,
            script_pubkey: addr.script_pubkey(),
        };
        let sig = sign_leaf(&secp, &tx, &prevout, &anchor.refund_script, &user);
        let mut w = Witness::new();
        w.push(sig);
        w.push(anchor.refund_script.as_bytes());
        w.push(control_refund.serialize());
        tx.input[0].witness = w;
        let txid: Txid = rpc.send_raw_transaction(&tx)?;
        rpc.generate_to_address(1, &miner)?;
        println!("REFUND   spend confirmed: {txid}");
    }

    // ---- TAMPER: finalize with a WRONG preimage must be rejected ----
    {
        let (op, value) = fund_anchor(&rpc, &addr, &miner)?;
        rpc.generate_to_address(finalize_csv as u64, &miner)?;
        let dest = rpc.get_new_address(None, None)?.assume_checked();
        let mut tx = spend_tx(op, value, &dest, finalize_csv);
        let prevout = TxOut {
            value,
            script_pubkey: addr.script_pubkey(),
        };
        let sig = sign_leaf(&secp, &tx, &prevout, &anchor.finalize_script, &operator);
        let mut w = Witness::new();
        w.push(sig);
        w.push(rand32()); // wrong preimage
        w.push(anchor.finalize_script.as_bytes());
        w.push(control_finalize.serialize());
        tx.input[0].witness = w;
        match rpc.send_raw_transaction(&tx) {
            Err(e) => println!("TAMPER (wrong preimage) rejected as expected: {e}"),
            Ok(txid) => return Err(format!("tamper unexpectedly accepted: {txid}").into()),
        }
    }

    // Key-path spend is structurally impossible: the internal key is the
    // lock-anchor NUMS (unknown discrete log), so no key-path signature exists.
    println!("KEY-PATH spend: impossible by construction (NUMS internal key)");

    println!("\nG1 on-chain lock spend paths verified.");
    Ok(())
}
