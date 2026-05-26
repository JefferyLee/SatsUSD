//! SatUSD wallet refund monitor (MVP, PRD §14.4 — "Refund tx 构造 + 时机监听").
//!
//! A user who locked SatUSD for a redemption must get it back if the operator
//! never pays: once the lock anchor's relative-timelock (`asset_lock_csv_delta`)
//! matures, the refund tapleaf (`<user_key> CHECKSIGVERIFY <csv> CSV`) is
//! spendable. This bin demonstrates the monitor on the Bitcoin layer against live
//! devnet bitcoind: build + fund a lock anchor, poll the chain, show the refund
//! is rejected before maturity, then broadcast it once the CSV is satisfied.
//!
//!   cargo run -p satusd-tapd-client --bin refund_monitor

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

const FINALIZE_CSV: i64 = 6;
const REFUND_CSV: i64 = 10; // the user's asset-lock CSV (relative timelock)

fn det_keypair(secp: &Secp256k1<bitcoin::secp256k1::All>, label: &str) -> Keypair {
    let mut seed = sha256::Hash::hash(label.as_bytes()).to_byte_array();
    loop {
        if let Ok(sk) = SecretKey::from_slice(&seed) {
            return Keypair::from_secret_key(secp, &sk);
        }
        seed = sha256::Hash::hash(&seed).to_byte_array();
    }
}

fn main() -> Result<(), Err> {
    let secp = Secp256k1::new();
    let btc = Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        Auth::UserPass("satusd".into(), "satusd".into()),
    )?;
    let miner = btc.get_new_address(None, None)?.assume_checked();

    // ---- build the lock anchor (NUMS internal + finalize/refund) ----
    let user = det_keypair(&secp, "refund-monitor-user");
    let user_xonly: XOnlyPublicKey = user.x_only_public_key().0;
    let op_xonly: XOnlyPublicKey = det_keypair(&secp, "refund-monitor-op")
        .x_only_public_key()
        .0;
    let payment_hash = sha256::Hash::hash(b"refund-monitor-preimage").to_byte_array();
    let anchor = satusd_lock::build_lock_anchor(
        &payment_hash,
        op_xonly,
        user_xonly,
        FINALIZE_CSV,
        REFUND_CSV,
    );

    // ---- fund it on-chain; the confirming block is the anchor height ----
    let spk = anchor.script_pubkey();
    let addr = Address::from_script(&spk, Network::Regtest)?;
    let value = Amount::from_sat(50_000);
    let fund_txid = btc.send_to_address(&addr, value, None, None, None, None, None, None)?;
    btc.generate_to_address(1, &miner)?;
    let anchor_height = btc.get_block_count()? as i64;
    let fund_tx = btc.get_raw_transaction(&fund_txid, None)?;
    let vout = fund_tx
        .output
        .iter()
        .position(|o| o.script_pubkey == spk)
        .ok_or("lock output not found in funding tx")? as u32;
    let outpoint = OutPoint {
        txid: fund_txid,
        vout,
    };
    println!(
        "lock anchor funded: {outpoint}  ({} sat)  at height {anchor_height}; refund matures at +{REFUND_CSV}",
        value.to_sat()
    );

    // ---- build the refund spend (refund tapleaf: user sig + CSV) ----
    let build_refund = || -> Result<Transaction, Err> {
        let dest = btc.get_new_address(None, None)?.assume_checked();
        let mut tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: outpoint,
                script_sig: Default::default(),
                sequence: Sequence::from_height(REFUND_CSV as u16),
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
        let leaf = TapLeafHash::from_script(&anchor.refund_script, LeafVersion::TapScript);
        let sighash = SighashCache::new(&tx).taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(std::slice::from_ref(&prevout)),
            leaf,
            TapSighashType::Default,
        )?;
        // The refund tapleaf checks the *untweaked* user key, so sign with it.
        let sig =
            secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &user);
        let control = anchor
            .spend_info
            .control_block(&(anchor.refund_script.clone(), LeafVersion::TapScript))
            .ok_or("no refund control block")?;
        let mut w = Witness::new();
        w.push(sig.serialize());
        w.push(anchor.refund_script.as_bytes());
        w.push(control.serialize());
        tx.input[0].witness = w;
        Ok(tx)
    };

    // ---- monitor: poll the chain; refund only once the CSV is satisfied ----
    let refund = build_refund()?;
    // Before maturity: the relative timelock makes the spend non-final → rejected.
    let tip = btc.get_block_count()? as i64;
    if tip < anchor_height + REFUND_CSV {
        match btc.send_raw_transaction(&refund) {
            Err(_) => println!(
                "  [t={tip}] refund correctly REJECTED (CSV not yet matured: need height {})",
                anchor_height + REFUND_CSV
            ),
            Ok(_) => return Err("refund accepted before CSV matured!".into()),
        }
    }

    loop {
        let tip = btc.get_block_count()? as i64;
        if tip >= anchor_height + REFUND_CSV {
            let txid = btc.send_raw_transaction(&refund)?;
            btc.generate_to_address(1, &miner)?;
            println!("  [t={tip}] CSV matured → refund BROADCAST + confirmed: {txid}");
            break;
        }
        println!(
            "  [t={tip}] waiting for refund maturity (height {})…",
            anchor_height + REFUND_CSV
        );
        btc.generate_to_address(1, &miner)?; // advance the chain (regtest stand-in for time)
    }

    println!("\nRefund monitor: lock returned to the user after CSV. ✓");
    Ok(())
}
