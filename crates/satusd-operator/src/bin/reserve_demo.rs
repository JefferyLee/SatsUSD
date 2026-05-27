//! Reserve reimburse-leg demo (M6 pilot, §11.2) on the regtest devnet — the
//! in-environment half of the signet pilot's `finalize → reimburse` path.
//!
//! Builds a real 3-of-5 P2WSH committee reserve address, funds it (the reserve
//! UTXO), then reimburses an operator by building a spend, signing it with 3 of the
//! 5 committee keys, and broadcasting it. bitcoind performs the actual consensus
//! validation of the multisig spend. The `reimbursement_sats` + the authorization
//! correspond to a `satusd_reserve::MultisigReserve::finalize_claim` outcome (the
//! committee 3-of-5 approval over `claim_id`); here we realize the resulting
//! `ReserveSpend` as a real Bitcoin transaction.
//!
//!   make devnet-up && cargo run -p satusd-operator --bin reserve_demo
//!
//! On signet this is the same code with `Network::Signet` + a signet bitcoind.

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use bitcoin::{Network, OutPoint, PublicKey};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use satusd_operator::reserve_spend::{
    build_reserve_spend, reserve_address, reserve_witness_script, sign_reserve_spend,
};

type Err = Box<dyn std::error::Error>;

/// Deterministic committee key from a label (committee keys are distinct from issuers).
fn committee_key(secp: &Secp256k1<bitcoin::secp256k1::All>, label: &str) -> (SecretKey, PublicKey) {
    let mut s = sha256::Hash::hash(label.as_bytes()).to_byte_array();
    loop {
        if let Ok(sk) = SecretKey::from_slice(&s) {
            return (sk, PublicKey::new(sk.public_key(secp)));
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

    // ---- the 3-of-5 committee reserve (§11.2) ----
    let keys: Vec<(SecretKey, PublicKey)> = (1..=5)
        .map(|i| committee_key(&secp, &format!("satusd-reserve-committee-{i}")))
        .collect();
    let pubkeys: Vec<PublicKey> = keys.iter().map(|(_, pk)| *pk).collect();
    let witness_script = reserve_witness_script(&pubkeys, 3);
    let reserve = reserve_address(&witness_script, Network::Regtest);
    println!("reserve 3-of-5 P2WSH address: {reserve}");

    // ---- fund the reserve UTXO (a BTC deposit into custody) ----
    let funded = bitcoin::Amount::from_sat(2_000_000);
    let fund_txid = btc.send_to_address(&reserve, funded, None, None, None, None, None, None)?;
    btc.generate_to_address(1, &miner)?;
    let fund_tx = btc.get_raw_transaction(&fund_txid, None)?;
    let spk = reserve.script_pubkey();
    let vout = fund_tx
        .output
        .iter()
        .position(|o| o.script_pubkey == spk)
        .ok_or("reserve output not found")? as u32;
    let reserve_utxo = OutPoint {
        txid: fund_txid,
        vout,
    };
    println!("reserve funded: {reserve_utxo} ({} sat)", funded.to_sat());

    // ---- reimburse the operator (the ReserveSpend a finalized claim authorizes) ----
    let operator_payout = btc.get_new_address(None, None)?.assume_checked();
    let reimbursement_sats = 80_000u64; // = claim.reimbursement_sats
    let fee = 1_000u64;
    let change_to_reserve = funded.to_sat() - reimbursement_sats - fee;

    // Pay the operator; return the rest to the reserve (a real spend has change).
    let mut tx = build_reserve_spend(
        reserve_utxo,
        operator_payout.script_pubkey(),
        reimbursement_sats,
    );
    tx.output.push(bitcoin::TxOut {
        value: bitcoin::Amount::from_sat(change_to_reserve),
        script_pubkey: spk.clone(),
    });

    // 3-of-5: committee members 0, 1, 2 sign (governance approval realized on-chain).
    let signers = vec![(0usize, keys[0].0), (1, keys[1].0), (2, keys[2].0)];
    sign_reserve_spend(&mut tx, &witness_script, funded.to_sat(), signers);

    let spend_txid = btc.send_raw_transaction(&tx)?;
    btc.generate_to_address(1, &miner)?;
    println!("committee 3-of-5 reserve spend broadcast + confirmed: {spend_txid}");

    // ---- verify the operator was paid ----
    let got = btc.get_received_by_address(&operator_payout, Some(1))?;
    assert_eq!(got.to_sat(), reimbursement_sats, "operator reimbursement");
    println!(
        "operator {} received {} sat ✓",
        operator_payout,
        got.to_sat()
    );
    println!("\nReserve reimburse-leg closed end-to-end on regtest (real P2WSH 3-of-5 spend).");
    Ok(())
}
