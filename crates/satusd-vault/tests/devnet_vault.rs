//! Vault lifecycle E2E on a live regtest devnet (spec 06).
//!
//! The vault funding output is pure BTC collateral (no TA committed —
//! the minted SatUSD is issued separately), so its CETs are plain
//! Bitcoin transactions: this test needs only bitcoind + the
//! in-process oracle, no tapd vPSBT machinery.
//!
//! ```text
//! OPEN     send collateral to Q (single-leaf {refund} P2TR)
//!          → on-chain scriptPubKey == reconstructed Q
//! PRESIGN  for every crash bucket, build its CET (key-path spend of Q,
//!          payout reserve/broadcaster/minter), presign the keyspend
//!          adaptor anticipating the oracle — BEFORE the outcome
//! GLIDE    a healthy price falls in no pre-signed bucket
//! CRASH    oracle attests a crash price → that bucket's secret decrypts
//!          its CET → witness goes in → broadcast → confirmed → the
//!          payout outputs are on-chain
//! ```
//!
//! Requires `make devnet-up`. Run:
//! ```text
//! cargo test -p satusd-vault --test devnet_vault -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use bitcoin::hashes::Hash;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::taproot::{LeafVersion, TapLeafHash};
use bitcoin::{
    Amount, OutPoint, ScriptBuf, Sequence, TapSighashType, Transaction, TxIn, TxOut, Witness,
};
use satusd_oracle::oracle::Oracle;
use satusd_oracle::schnorr::sign_with_nonce;
use satusd_rail::encode::tagged_hash;
use satusd_rail1::adaptor::{decrypt, presign, verify_presig, AdaptorSig};
use satusd_vault::cet::{bucket_of, bucket_secret, crash_adaptor_point, crash_schedule};
use satusd_vault::contract::{opening_ok, VaultTerms};
use satusd_vault::funding::{keyspend_secret, refund_leaf_script, spend_info, vault_funding_output};
use satusd_vault::musig::{aggregate_internal_x, cosign_keyspend};
use satusd_vault::settle::{face_sats, CrashPayout, PayoutParams};
use secp256k1::{Secp256k1, SecretKey, XOnlyPublicKey};

const M: u8 = 9;
const FEE_SATS: u64 = 2_000;
const DUST: u64 = 330;
const REF_PRICE: u32 = 64_000;
const CRASH_PRICE: u32 = 46_080; // a crash bucket whose midpoint still leaves the minter a cushion
const HEALTHY_PRICE: u32 = 64_000;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn env() -> satusd_tapd_client::env::NodeEnv {
    satusd_tapd_client::env::NodeEnv::from_env(root())
}
fn bcli(args: &[&str]) -> serde_json::Value {
    env().bcli(args)
}

/// A fresh wallet address' scriptPubKey.
fn new_spk(label: &str) -> ScriptBuf {
    let a = bcli(&["getnewaddress", label, "bech32"]);
    let info = bcli(&["getaddressinfo", a.as_str().unwrap()]);
    ScriptBuf::from_hex(info["scriptPubKey"].as_str().unwrap()).unwrap()
}

fn terms() -> VaultTerms {
    VaultTerms {
        collateral_sats: 2_343_750, // ~150% CR for $1000 @ $64k
        mint_micro_usd: 1_000_000_000,
        opening_cr_bps: 15_000,
        liq_cr_bps: 11_000,
        checkpoint_interval: 144,
        maturity_height: 1_000_000,
        m: M,
        penalty_bps: 500,
        oracle_event_series: [7u8; 32],
    }
}
fn params() -> PayoutParams {
    PayoutParams {
        penalty_bps: 500,
        bounty_bps_of_penalty: 1_000,
        bounty_cap_sats: 5_000,
        fee_budget_sats: FEE_SATS,
    }
}

/// The CET's outputs for a payout (zero/dust outputs dropped).
fn payout_outputs(po: &CrashPayout, reserve: &ScriptBuf, bc: &ScriptBuf, minter: &ScriptBuf) -> Vec<TxOut> {
    [(po.reserve_sats, reserve), (po.broadcaster_sats, bc), (po.minter_sats, minter)]
        .into_iter()
        .filter(|(v, _)| *v >= DUST)
        .map(|(v, spk)| TxOut { value: Amount::from_sat(v), script_pubkey: spk.clone() })
        .collect()
}

fn cet_tx(funding_outpoint: OutPoint, outputs: Vec<TxOut>) -> Transaction {
    Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: funding_outpoint,
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            ..Default::default()
        }],
        output: outputs,
    }
}

#[tokio::test]
#[ignore = "requires live devnet bitcoind (make devnet-up)"]
async fn vault_lifecycle_crash_settle() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let secp = Secp256k1::new();
    let t = terms();

    // Opening CR holds at the reference price.
    assert!(opening_ok(&t, REF_PRICE), "vault must meet opening CR @ ${REF_PRICE}");

    // ---- vault funding output (single-leaf {refund}) ----
    let funding_sk = tagged_hash("devnet/vault-funding", b"v1");
    let (px, _) = SecretKey::from_byte_array(funding_sk).unwrap().x_only_public_key(&secp);
    let internal_x = px.serialize();
    let refund = refund_leaf_script(
        4032,
        &tagged_hash("devnet/minter", b"m"),
        &tagged_hash("devnet/reserve", b"r"),
    );
    let f = vault_funding_output(&internal_x, &refund);
    let tweaked = keyspend_secret(&funding_sk, &f.merkle_root)?;
    // The tweaked secret's pubkey is Q.
    let (qx, _) = SecretKey::from_byte_array(tweaked).unwrap().x_only_public_key(&secp);
    assert_eq!(qx.serialize(), f.output_x);

    // ---- OPEN: lock collateral into Q ----
    let q = XOnlyPublicKey::from_byte_array(f.output_x)?;
    let spk_hex = format!("5120{}", hex::encode(f.output_x));
    let q_addr = bcli(&["decodescript", &spk_hex])["address"]
        .as_str()
        .ok_or("decodescript address")?
        .to_string();
    let c_btc = format!("{:.8}", t.collateral_sats as f64 / 1e8);
    let open_txid = bcli(&["sendtoaddress", &q_addr, &c_btc])
        .as_str()
        .ok_or("sendtoaddress")?
        .to_string();
    bcli(&["-generate", "2"]);

    let raw = bcli(&["getrawtransaction", &open_txid, "true"]);
    let vout = raw["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(spk_hex.as_str()))
        .ok_or("funding output (Q) not in the open tx")?;
    let fund_vout = vout["n"].as_u64().unwrap() as u32;
    let funding_outpoint = OutPoint::new(open_txid.parse()?, fund_vout);
    let fund_value = Amount::from_btc(vout["value"].as_f64().unwrap())?;
    let funding_txout = TxOut { value: fund_value, script_pubkey: ScriptBuf::from_hex(&spk_hex)? };
    assert_eq!(fund_value.to_sat(), t.collateral_sats, "collateral locked at Q");
    println!("OPEN: {} sats locked at Q={} ({funding_outpoint})", t.collateral_sats, hex::encode(f.output_x));

    // ---- payout addresses ----
    let reserve_spk = new_spk("vault-reserve");
    let bcaster_spk = new_spk("vault-broadcaster");
    let minter_spk = new_spk("vault-minter");

    // ---- oracle announces the checkpoint event ----
    let oracle = Oracle::from_seed(&tagged_hash("devnet/vault-oracle", b"seed"))?;
    let tick = 1_700_000_000u64;
    let ann = oracle.announce(tick)?;

    // ---- PRESIGN every crash bucket's CET, before the outcome ----
    struct PreCet {
        bucket: u32,
        tx: Transaction,
        sighash: [u8; 32],
        presig: AdaptorSig,
    }
    let sched = crash_schedule(&t, params());
    assert!(!sched.is_empty());
    let mut cets: Vec<PreCet> = Vec::new();
    for c in &sched {
        let outs = payout_outputs(&c.payout, &reserve_spk, &bcaster_spk, &minter_spk);
        let tx = cet_tx(funding_outpoint, outs);
        let sighash = SighashCache::new(&tx)
            .taproot_key_spend_signature_hash(0, &Prevouts::All(&[funding_txout.clone()]), TapSighashType::Default)?
            .to_byte_array();
        let point = crash_adaptor_point(&ann, &oracle.pubkey, &t, c.bucket_index)?;
        let nonce = tagged_hash("devnet/vault-cet-nonce", &c.bucket_index.to_be_bytes());
        let presig = presign(&tweaked, &nonce, &sighash, &point)?;
        assert!(verify_presig(&presig, &f.output_x, &sighash, &point)?);
        cets.push(PreCet { bucket: c.bucket_index, tx, sighash, presig });
    }
    println!("PRESIGN: {} crash-bucket CETs pre-signed (before any attestation)", cets.len());

    // ---- GLIDE: a healthy price falls in no pre-signed bucket ----
    let healthy_bucket = bucket_of(HEALTHY_PRICE, M);
    assert!(
        !cets.iter().any(|c| c.bucket == healthy_bucket),
        "healthy price ${HEALTHY_PRICE} (bucket {healthy_bucket}) is not a crash bucket — the vault glides"
    );
    println!("GLIDE: healthy ${HEALTHY_PRICE} → bucket {healthy_bucket}, nothing broadcastable");

    // ---- CRASH: the oracle attests a crash price ----
    let att = oracle.attest(tick, CRASH_PRICE)?;
    let winner = bucket_of(CRASH_PRICE, M);
    let loser = winner ^ 1;
    let win = cets.iter().find(|c| c.bucket == winner).ok_or("crash price is in a pre-signed bucket")?;

    // Selective decryption: a different bucket's secret does not exist.
    assert!(bucket_secret(&att, M, loser).is_err(), "only the attested bucket decrypts");

    let secret = bucket_secret(&att, M, winner)?;
    let sig = decrypt(&win.presig, &secret)?;
    // The decrypted signature is a valid BIP-340 keyspend under Q.
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(sig),
        &win.sighash,
        &q,
    )?;

    // ---- assemble + broadcast the winning CET ----
    let mut tx = win.tx.clone();
    tx.input[0].witness = Witness::from_slice(&[sig.to_vec()]);
    let raw_hex = hex::encode(bitcoin::consensus::serialize(&tx));
    let cet_txid = bcli(&["sendrawtransaction", &raw_hex]);
    let cet_txid = cet_txid.as_str().ok_or_else(|| format!("sendrawtransaction: {cet_txid}"))?.to_string();
    bcli(&["-generate", "2"]);
    println!("CRASH: bucket {winner} CET broadcast: {cet_txid}");

    // ---- confirm: the CET is mined and its payout outputs are on-chain ----
    let mined = bcli(&["getrawtransaction", &cet_txid, "true"]);
    assert!(mined["confirmations"].as_i64().unwrap_or(0) >= 1, "CET confirmed");
    let outs = &win.tx.output;
    let reserve_out = outs.iter().find(|o| o.script_pubkey == reserve_spk).ok_or("reserve output")?;
    let total_out: u64 = outs.iter().map(|o| o.value.to_sat()).sum();
    assert_eq!(total_out, t.collateral_sats - FEE_SATS, "outputs sum to collateral − fee");
    assert!(reserve_out.value.to_sat() > 0, "reserve receives the face-backing");
    // On-chain confirmation of the reserve payout amount.
    let onchain_reserve = mined["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(hex::encode(reserve_spk.as_bytes()).as_str()))
        .ok_or("reserve vout on-chain")?;
    assert_eq!(
        Amount::from_btc(onchain_reserve["value"].as_f64().unwrap())?.to_sat(),
        reserve_out.value.to_sat(),
        "on-chain reserve payout matches the pre-signed CET"
    );

    println!(
        "VAULT SETTLED: oracle-gated crash CET spent the vault collateral by key path — \
         reserve {} sats, broadcaster {} sats, minter {} sats (bucket {winner} of 2^{M})",
        outs.iter().find(|o| o.script_pubkey == reserve_spk).map(|o| o.value.to_sat()).unwrap_or(0),
        outs.iter().find(|o| o.script_pubkey == bcaster_spk).map(|o| o.value.to_sat()).unwrap_or(0),
        outs.iter().find(|o| o.script_pubkey == minter_spk).map(|o| o.value.to_sat()).unwrap_or(0),
    );
    Ok(())
}

/// The healthy close (spec 06 §5): the minter proves the SatUSD burn,
/// the reserve co-signs, and the collateral is reclaimed in full. v0
/// has a single funding key, so the reclaim is one key-path spend of Q
/// back to the minter — no oracle, no adaptor gating. The keyspend
/// signature is synthesized via the adaptor machinery with a throwaway
/// scalar (the stand-in for the MuSig2(minter, reserve) co-sign), which
/// yields a valid BIP-340 signature under Q exactly as the unit tests
/// show.
#[tokio::test]
#[ignore = "requires live devnet bitcoind (make devnet-up)"]
async fn vault_burn_reclaim() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let secp = Secp256k1::new();
    let t = terms();

    // ---- OPEN a vault (distinct funding key ⇒ distinct Q/outpoint) ----
    let funding_sk = tagged_hash("devnet/vault-reclaim-funding", b"v2");
    let (px, _) = SecretKey::from_byte_array(funding_sk).unwrap().x_only_public_key(&secp);
    let internal_x = px.serialize();
    let refund = refund_leaf_script(
        4032,
        &tagged_hash("devnet/minter", b"m"),
        &tagged_hash("devnet/reserve", b"r"),
    );
    let f = vault_funding_output(&internal_x, &refund);
    let tweaked = keyspend_secret(&funding_sk, &f.merkle_root)?;
    let q = XOnlyPublicKey::from_byte_array(f.output_x)?;
    let spk_hex = format!("5120{}", hex::encode(f.output_x));
    let q_addr = bcli(&["decodescript", &spk_hex])["address"].as_str().ok_or("addr")?.to_string();
    let c_btc = format!("{:.8}", t.collateral_sats as f64 / 1e8);
    let open_txid = bcli(&["sendtoaddress", &q_addr, &c_btc]).as_str().ok_or("send")?.to_string();
    bcli(&["-generate", "2"]);
    let raw = bcli(&["getrawtransaction", &open_txid, "true"]);
    let vout = raw["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(spk_hex.as_str()))
        .ok_or("Q vout")?;
    let funding_outpoint = OutPoint::new(open_txid.parse()?, vout["n"].as_u64().unwrap() as u32);
    let fund_value = Amount::from_btc(vout["value"].as_f64().unwrap())?;
    let funding_txout = TxOut { value: fund_value, script_pubkey: ScriptBuf::from_hex(&spk_hex)? };
    println!("OPEN(reclaim): {} sats at Q ({funding_outpoint})", fund_value.to_sat());

    // ---- BURN-RECLAIM: keyspend Q → all collateral (− fee) to minter ----
    let minter_spk = new_spk("vault-reclaim-minter");
    let reclaim_value = fund_value.to_sat() - FEE_SATS;
    let tx = cet_tx(
        funding_outpoint,
        vec![TxOut { value: Amount::from_sat(reclaim_value), script_pubkey: minter_spk.clone() }],
    );
    let sighash = SighashCache::new(&tx)
        .taproot_key_spend_signature_hash(0, &Prevouts::All(&[funding_txout.clone()]), TapSighashType::Default)?
        .to_byte_array();

    // Synthesize the key-path signature (v0 stand-in for the co-sign).
    let cosign = tagged_hash("devnet/reclaim-cosign", b"v2");
    let cosign_point = SecretKey::from_byte_array(cosign)?.public_key(&secp);
    let presig = presign(&tweaked, &tagged_hash("devnet/reclaim-nonce", b"v2"), &sighash, &cosign_point)?;
    let sig = decrypt(&presig, &cosign)?;
    secp.verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(sig), &sighash, &q)?;

    let mut tx2 = tx.clone();
    tx2.input[0].witness = Witness::from_slice(&[sig.to_vec()]);
    let reclaim_txid = bcli(&["sendrawtransaction", &hex::encode(bitcoin::consensus::serialize(&tx2))]);
    let reclaim_txid = reclaim_txid.as_str().ok_or_else(|| format!("sendraw: {reclaim_txid}"))?.to_string();
    bcli(&["-generate", "2"]);

    let mined = bcli(&["getrawtransaction", &reclaim_txid, "true"]);
    assert!(mined["confirmations"].as_i64().unwrap_or(0) >= 1, "reclaim confirmed");
    let out = mined["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(hex::encode(minter_spk.as_bytes()).as_str()))
        .ok_or("minter vout on-chain")?;
    assert_eq!(
        Amount::from_btc(out["value"].as_f64().unwrap())?.to_sat(),
        reclaim_value,
        "minter reclaims all collateral − fee"
    );
    println!("BURN-RECLAIM: {reclaim_value} sats returned to the minter (healthy close), {reclaim_txid}");
    Ok(())
}

/// The oracle-silence backstop (spec 06 §6): if the oracle goes dark,
/// no crash CET can ever decrypt, so the minter is never liquidated —
/// and the refund_leaf's CSV script-path lets the minter (with the
/// reserve) reclaim the collateral after the tlock matures. This is
/// the only spend of Q by SCRIPT path; the crash/reclaim paths are
/// key-path. A small CSV is used so the relative timelock matures in
/// the test.
#[tokio::test]
#[ignore = "requires live devnet bitcoind (make devnet-up)"]
async fn vault_tlock_refund() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let secp = Secp256k1::new();
    let t = terms();
    const CSV: u16 = 6;

    // ---- real minter + reserve keypairs (the refund_leaf CHECKSIGs) ----
    let minter_sk = tagged_hash("devnet/vault-minter-sk", b"v3");
    let reserve_sk = tagged_hash("devnet/vault-reserve-sk", b"v3");
    let minter_x = SecretKey::from_byte_array(minter_sk)?.x_only_public_key(&secp).0.serialize();
    let reserve_x = SecretKey::from_byte_array(reserve_sk)?.x_only_public_key(&secp).0.serialize();
    let refund = refund_leaf_script(CSV, &minter_x, &reserve_x);

    // ---- vault funding output bound to this refund leaf ----
    let funding_sk = tagged_hash("devnet/vault-refund-funding", b"v3");
    let internal_x = SecretKey::from_byte_array(funding_sk)?.x_only_public_key(&secp).0.serialize();
    let f = vault_funding_output(&internal_x, &refund);
    let spk_hex = format!("5120{}", hex::encode(f.output_x));

    // ---- OPEN ----
    let q_addr = bcli(&["decodescript", &spk_hex])["address"].as_str().ok_or("addr")?.to_string();
    let c_btc = format!("{:.8}", t.collateral_sats as f64 / 1e8);
    let open_txid = bcli(&["sendtoaddress", &q_addr, &c_btc]).as_str().ok_or("send")?.to_string();
    bcli(&["-generate", "1"]);
    let raw = bcli(&["getrawtransaction", &open_txid, "true"]);
    let vout = raw["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(spk_hex.as_str()))
        .ok_or("Q vout")?;
    let funding_outpoint = OutPoint::new(open_txid.parse()?, vout["n"].as_u64().unwrap() as u32);
    let fund_value = Amount::from_btc(vout["value"].as_f64().unwrap())?;
    let funding_txout = TxOut { value: fund_value, script_pubkey: ScriptBuf::from_hex(&spk_hex)? };
    println!("OPEN(refund): {} sats at Q ({funding_outpoint})", fund_value.to_sat());

    // ---- the tlock must mature: the funding UTXO must be CSV deep ----
    bcli(&["-generate", &(CSV as u32 + 1).to_string()]);

    // ---- build the refund tx: CSV-sequenced input, collateral to minter ----
    let minter_spk = new_spk("vault-refund-minter");
    let refund_value = fund_value.to_sat() - FEE_SATS;
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: funding_outpoint,
            sequence: Sequence::from_consensus(CSV as u32), // relative-blocks tlock
            ..Default::default()
        }],
        output: vec![TxOut { value: Amount::from_sat(refund_value), script_pubkey: minter_spk.clone() }],
    };

    // ---- script-path sighash over the refund leaf, then both sigs ----
    let leaf_hash = TapLeafHash::from_script(&refund, LeafVersion::TapScript);
    let sighash = SighashCache::new(&tx)
        .taproot_script_spend_signature_hash(0, &Prevouts::All(&[funding_txout.clone()]), leaf_hash, TapSighashType::Default)?
        .to_byte_array();
    let minter_sig = sign_with_nonce(&minter_sk, &tagged_hash("devnet/refund-nonce-m", b"v3"), &sighash)?;
    let reserve_sig = sign_with_nonce(&reserve_sk, &tagged_hash("devnet/refund-nonce-r", b"v3"), &sighash)?;

    // ---- control block for the refund leaf ----
    let info = spend_info(&internal_x, &refund)?;
    let cb = info
        .control_block(&(refund.clone(), LeafVersion::TapScript))
        .ok_or("refund leaf control block")?;

    // ---- witness: script consumes minter sig (CHECKSIGVERIFY) then
    //      reserve sig (CHECKSIG); so minter sig sits on top ----
    let mut tx2 = tx.clone();
    tx2.input[0].witness = Witness::from_slice(&[
        reserve_sig.to_vec(),
        minter_sig.to_vec(),
        refund.as_bytes().to_vec(),
        cb.serialize(),
    ]);
    let refund_txid = bcli(&["sendrawtransaction", &hex::encode(bitcoin::consensus::serialize(&tx2))]);
    let refund_txid = refund_txid.as_str().ok_or_else(|| format!("sendraw: {refund_txid}"))?.to_string();
    bcli(&["-generate", "2"]);

    let mined = bcli(&["getrawtransaction", &refund_txid, "true"]);
    assert!(mined["confirmations"].as_i64().unwrap_or(0) >= 1, "refund confirmed");
    let out = mined["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(hex::encode(minter_spk.as_bytes()).as_str()))
        .ok_or("minter vout on-chain")?;
    assert_eq!(
        Amount::from_btc(out["value"].as_f64().unwrap())?.to_sat(),
        refund_value,
        "tlock refund returns the collateral to the minter"
    );
    println!("TLOCK-REFUND: script-path CSV({CSV}) spend reclaimed {refund_value} sats to the minter, {refund_txid}");
    Ok(())
}

/// SatUSD issuance at open (spec 06 §3 step 4): conditioned on a valid
/// vault, the group key reissues $X SatUSD to the minter — the "mint"
/// half that makes the vault a real CDP (lock BTC, RECEIVE SatUSD).
/// v0: one devnet tapd is both the group-key holder (founder,
/// scaffolding) and the minter; this validates the open→reissue
/// sequence and that the SatUSD group's supply grows by exactly the
/// minted amount (the new SatUSD exists, backed by the vault).
#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) — bitcoind + tapd"]
async fn vault_open_and_mint() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let secp = Secp256k1::new();
    let t = terms();
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

    const S0: u64 = 1_000;
    const MINT_UNITS: u64 = 1_000; // the $X reissued against the vault

    // ---- 1. mint the grouped SatUSD asset (a fresh group) ----
    let group_name = format!("SatUSD-vault-{ts}");
    env.tapcli(&["assets", "mint", "--type", "normal", "--name", &group_name, "--supply", &S0.to_string(), "--new_grouped_asset"]);
    env.tapcli(&["assets", "mint", "finalize"]);
    env.bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let amount_of = |a: &serde_json::Value| -> u64 {
        a["amount"].as_str().and_then(|s| s.parse().ok()).or_else(|| a["amount"].as_u64()).unwrap_or(0)
    };
    let list: serde_json::Value = serde_json::from_str(&env.tapcli(&["assets", "list"]))?;
    let asset = list["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["asset_genesis"]["name"].as_str() == Some(group_name.as_str()))
        .ok_or("minted group asset not found")?;
    let gk = asset["asset_group"]["tweaked_group_key"].as_str().ok_or("no group key")?.to_string();
    println!("MINT GROUP: {group_name} supply {S0}, group_key {}…", &gk[..16]);

    // ---- 2. OPEN a vault (lock BTC collateral at Q) ----
    assert!(opening_ok(&t, REF_PRICE));
    let funding_sk = tagged_hash("devnet/vault-mint-funding", b"v4");
    let internal_x = SecretKey::from_byte_array(funding_sk)?.x_only_public_key(&secp).0.serialize();
    let refund = refund_leaf_script(4032, &tagged_hash("devnet/minter", b"m"), &tagged_hash("devnet/reserve", b"r"));
    let f = vault_funding_output(&internal_x, &refund);
    let spk_hex = format!("5120{}", hex::encode(f.output_x));
    let q_addr = env.bcli(&["decodescript", &spk_hex])["address"].as_str().ok_or("addr")?.to_string();
    let open_txid = env
        .bcli(&["sendtoaddress", &q_addr, &format!("{:.8}", t.collateral_sats as f64 / 1e8)])
        .as_str()
        .ok_or("send")?
        .to_string();
    env.bcli(&["-generate", "2"]);
    println!("OPEN: {} sats collateral locked at Q ({open_txid})", t.collateral_sats);

    // ---- 3. ISSUE $X against the vault: reissue into the group ----
    //         (scaffolding: the founder-held group key signs only with
    //         a standing vault as evidence — the open above, §3 step 4.)
    let mint_name = format!("SatUSD-vault-mint-{ts}");
    env.tapcli(&["assets", "mint", "--type", "normal", "--name", &mint_name, "--supply", &MINT_UNITS.to_string(), "--grouped_asset", "--group_key", &gk]);
    env.tapcli(&["assets", "mint", "finalize"]);
    env.bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // ---- 4. the SatUSD group's supply grew by exactly the minted $X ----
    let list2: serde_json::Value = serde_json::from_str(&env.tapcli(&["assets", "list"]))?;
    let group_supply: u64 = list2["assets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["asset_group"]["tweaked_group_key"].as_str() == Some(gk.as_str()))
        .map(amount_of)
        .sum();
    assert_eq!(
        group_supply,
        S0 + MINT_UNITS,
        "vault mint reissued {MINT_UNITS} SatUSD into the group (group key signs against the open vault)"
    );
    println!("ISSUE: SatUSD group supply {S0} → {group_supply} (+{MINT_UNITS} minted against the vault)");
    Ok(())
}

/// MuSig2(minter, reserve) funding output (spec 06 §2): the vault's
/// internal key is the real BIP-327 aggregate of the minter and reserve
/// keys (not v0's single funding key), and the key-path reclaim is a
/// genuine 2-of-2 co-signature. Validated on-chain: open a vault at
/// Q = taptweak(KeyAgg(minter, reserve), refund_root), then reclaim it
/// with a MuSig2 co-sign.
#[tokio::test]
#[ignore = "requires live devnet bitcoind (make devnet-up)"]
async fn vault_musig2_reclaim() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let secp = Secp256k1::new();
    let t = terms();

    // ---- minter + reserve keys; the funding key is their MuSig2 aggregate ----
    let minter_sk = tagged_hash("devnet/musig-minter-sk", b"v5");
    let reserve_sk = tagged_hash("devnet/musig-reserve-sk", b"v5");
    let minter_x = SecretKey::from_byte_array(minter_sk)?.x_only_public_key(&secp).0.serialize();
    let reserve_x = SecretKey::from_byte_array(reserve_sk)?.x_only_public_key(&secp).0.serialize();
    let internal_x = aggregate_internal_x(&minter_sk, &reserve_sk); // P = KeyAgg(minter, reserve)
    let refund = refund_leaf_script(4032, &minter_x, &reserve_x);
    let f = vault_funding_output(&internal_x, &refund);
    let spk_hex = format!("5120{}", hex::encode(f.output_x));

    // ---- OPEN: lock collateral at the MuSig2-aggregate Q ----
    let q_addr = env.bcli(&["decodescript", &spk_hex])["address"].as_str().ok_or("addr")?.to_string();
    let open_txid = env
        .bcli(&["sendtoaddress", &q_addr, &format!("{:.8}", t.collateral_sats as f64 / 1e8)])
        .as_str()
        .ok_or("send")?
        .to_string();
    env.bcli(&["-generate", "2"]);
    let raw = env.bcli(&["getrawtransaction", &open_txid, "true"]);
    let vout = raw["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(spk_hex.as_str()))
        .ok_or("Q vout")?;
    let funding_outpoint = OutPoint::new(open_txid.parse()?, vout["n"].as_u64().unwrap() as u32);
    let fund_value = Amount::from_btc(vout["value"].as_f64().unwrap())?;
    let funding_txout = TxOut { value: fund_value, script_pubkey: ScriptBuf::from_hex(&spk_hex)? };
    println!("OPEN(musig2): {} sats at MuSig2 Q ({funding_outpoint})", fund_value.to_sat());

    // ---- RECLAIM: a real 2-of-2 MuSig2 key-path spend back to the minter ----
    let minter_spk = new_spk("vault-musig2-minter");
    let reclaim_value = fund_value.to_sat() - FEE_SATS;
    let tx = cet_tx(
        funding_outpoint,
        vec![TxOut { value: Amount::from_sat(reclaim_value), script_pubkey: minter_spk.clone() }],
    );
    let sighash = SighashCache::new(&tx)
        .taproot_key_spend_signature_hash(0, &Prevouts::All(&[funding_txout.clone()]), TapSighashType::Default)?
        .to_byte_array();
    let sig = cosign_keyspend(&minter_sk, &reserve_sk, &f.merkle_root, &sighash);
    // independently verify the 2-of-2 sig under Q before broadcasting
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(sig),
        &sighash,
        &XOnlyPublicKey::from_byte_array(f.output_x)?,
    )?;

    let mut tx2 = tx.clone();
    tx2.input[0].witness = Witness::from_slice(&[sig.to_vec()]);
    let reclaim_txid = env.bcli(&["sendrawtransaction", &hex::encode(bitcoin::consensus::serialize(&tx2))]);
    let reclaim_txid = reclaim_txid.as_str().ok_or_else(|| format!("sendraw: {reclaim_txid}"))?.to_string();
    env.bcli(&["-generate", "2"]);

    let mined = env.bcli(&["getrawtransaction", &reclaim_txid, "true"]);
    assert!(mined["confirmations"].as_i64().unwrap_or(0) >= 1, "musig2 reclaim confirmed");
    let out = mined["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(hex::encode(minter_spk.as_bytes()).as_str()))
        .ok_or("minter vout on-chain")?;
    assert_eq!(
        Amount::from_btc(out["value"].as_f64().unwrap())?.to_sat(),
        reclaim_value,
        "MuSig2 2-of-2 keyspend reclaimed the collateral"
    );
    println!("MUSIG2-RECLAIM: 2-of-2(minter,reserve) keyspend of Q reclaimed {reclaim_value} sats, {reclaim_txid}");
    Ok(())
}

/// redeem_tx Step 2 (spec 07 §3.2, ADR-0005): the LP's collateral `Q`
/// is spent to the **holder** (`X/P` = `face_sats`) + change to the LP,
/// unlocked by the **oracle-adaptor CET the LP pre-signed at issuance**.
/// Same `Q` + adaptor mechanism as the crash CET; the redeem payout
/// replaces the liquidation payout (no reserve/penalty — the holder is
/// redeeming, not being liquidated). Step 3 composes this Q-spend with
/// the TA note→burn leg (settle-to-burn, devnet_burn_settle) in one
/// anchor tx for the full `redeem_tx`.
#[tokio::test]
#[ignore = "requires live devnet bitcoind (make devnet-up)"]
async fn vault_redeem_q_to_holder() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let secp = Secp256k1::new();
    let t = terms();
    const REDEEM_PRICE: u32 = REF_PRICE; // redeem at fair value

    // ---- vault funding output Q (single-leaf {refund}, as crash-settle) ----
    let funding_sk = tagged_hash("devnet/redeem-funding", b"v1");
    let internal_x = SecretKey::from_byte_array(funding_sk)?.x_only_public_key(&secp).0.serialize();
    let refund = refund_leaf_script(4032, &tagged_hash("devnet/minter", b"m"), &tagged_hash("devnet/reserve", b"r"));
    let f = vault_funding_output(&internal_x, &refund);
    let tweaked = keyspend_secret(&funding_sk, &f.merkle_root)?;
    let q = XOnlyPublicKey::from_byte_array(f.output_x)?;
    assert_eq!(SecretKey::from_byte_array(tweaked)?.x_only_public_key(&secp).0, q);

    // ---- OPEN: LP locks the over-collateralised Q ----
    let spk_hex = format!("5120{}", hex::encode(f.output_x));
    let q_addr = env.bcli(&["decodescript", &spk_hex])["address"].as_str().ok_or("addr")?.to_string();
    let open_txid = env
        .bcli(&["sendtoaddress", &q_addr, &format!("{:.8}", t.collateral_sats as f64 / 1e8)])
        .as_str()
        .ok_or("send")?
        .to_string();
    env.bcli(&["-generate", "2"]);
    let raw = env.bcli(&["getrawtransaction", &open_txid, "true"]);
    let vout = raw["vout"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(spk_hex.as_str()))
        .ok_or("Q vout")?;
    let funding_outpoint = OutPoint::new(open_txid.parse()?, vout["n"].as_u64().unwrap() as u32);
    let funding_txout = TxOut {
        value: Amount::from_btc(vout["value"].as_f64().unwrap())?,
        script_pubkey: ScriptBuf::from_hex(&spk_hex)?,
    };
    println!("OPEN: {} sats locked at Q={}", t.collateral_sats, hex::encode(f.output_x));

    // ---- the redeem payout: holder gets X/P, LP gets the change ----
    let holder_spk = new_spk("redeem-holder");
    let lp_spk = new_spk("redeem-lp-change");
    let face = face_sats(t.mint_micro_usd, REDEEM_PRICE); // X/P in sats
    let change = t.collateral_sats - face - FEE_SATS;
    let outs = vec![
        TxOut { value: Amount::from_sat(face), script_pubkey: holder_spk.clone() },
        TxOut { value: Amount::from_sat(change), script_pubkey: lp_spk.clone() },
    ];
    let tx = cet_tx(funding_outpoint, outs);
    let sighash = SighashCache::new(&tx)
        .taproot_key_spend_signature_hash(0, &Prevouts::All(&[funding_txout.clone()]), TapSighashType::Default)?
        .to_byte_array();

    // ---- ISSUANCE-time: the LP pre-signs the redeem CET (adaptor,
    //      anticipating the oracle), before any attestation ----
    let oracle = Oracle::from_seed(&tagged_hash("devnet/redeem-oracle", b"seed"))?;
    let tick = 1_700_000_000u64;
    let ann = oracle.announce(tick)?;
    let bucket = bucket_of(REDEEM_PRICE, M);
    let point = crash_adaptor_point(&ann, &oracle.pubkey, &t, bucket)?;
    let nonce = tagged_hash("devnet/redeem-cet-nonce", &bucket.to_be_bytes());
    let presig = presign(&tweaked, &nonce, &sighash, &point)?;
    assert!(verify_presig(&presig, &f.output_x, &sighash, &point)?, "redeem CET adaptor pre-sig valid");
    println!("PRESIGN: redeem CET pre-signed at issuance (bucket {bucket}; holder {face} + LP change {change})");

    // ---- REDEEM: the public oracle attests → the holder decrypts →
    //      broadcasts the keyspend of Q, alone, no LP at redeem-time ----
    let att = oracle.attest(tick, REDEEM_PRICE)?;
    let secret = bucket_secret(&att, M, bucket)?;
    let sig = decrypt(&presig, &secret)?;
    secp.verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(sig), &sighash, &q)?;
    let mut tx = tx;
    tx.input[0].witness = Witness::from_slice(&[sig.to_vec()]);
    let redeem_txid = env.bcli(&["sendrawtransaction", &hex::encode(bitcoin::consensus::serialize(&tx))]);
    let redeem_txid = redeem_txid.as_str().ok_or_else(|| format!("sendraw: {redeem_txid}"))?.to_string();
    env.bcli(&["-generate", "2"]);

    // ---- confirm: holder got X/P, LP got the change, on-chain ----
    let mined = env.bcli(&["getrawtransaction", &redeem_txid, "true"]);
    assert!(mined["confirmations"].as_i64().unwrap_or(0) >= 1, "redeem CET confirmed");
    let onchain = |spk: &ScriptBuf| -> Option<u64> {
        mined["vout"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(hex::encode(spk.as_bytes()).as_str()))
            .map(|o| Amount::from_btc(o["value"].as_f64().unwrap()).unwrap().to_sat())
    };
    assert_eq!(onchain(&holder_spk), Some(face), "holder receives X/P BTC");
    assert_eq!(onchain(&lp_spk), Some(change), "LP receives the change");
    println!(
        "REDEEM Q-LEG: oracle-adaptor CET key-path spent Q → holder {face} sats (X/P @ ${REDEEM_PRICE}) \
         + LP change {change} sats, broadcast unilaterally (redeem_tx Step 2): {redeem_txid}"
    );
    Ok(())
}
