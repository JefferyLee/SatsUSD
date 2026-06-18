//! FR-1 + FR-2 end-to-end (spec 07 §2–§4): mint the option pair and settle
//! it at maturity in ONE unilateral two-input transaction, on the trustless
//! 2-of-2 MuSig2 collateral `Q` from FR-3.
//!
//! Per position:
//!   MINT     a grouped SatUSD TA note `A` (the P leg the buyer holds).
//!   LOCK     over-collateralised BTC in `Q` = taptweak(KeyAgg(LP, holder),
//!            refund_root) — neither party can move it alone (§5).
//!   PRESIGN  the maturity CET as ONE 2-of-2 MuSig2 *adaptor* over the
//!            [A, Q] settle_tx's `Q`-keyspend sighash, for the price bucket,
//!            co-signed by LP + holder at issuance (no outcome yet).
//!   SETTLE   the public oracle attests → anyone adapts the joint adaptor
//!            with the bucket secret → the holder signs `A` → broadcasts:
//!            [A → burn sink, P = min(avail, face) → holder, N = avail−P → LP].
//!            No LP signature, no keeper, at maturity.
//!
//! The P-leg payout exercises `min(S, x)` across both regimes:
//!   x ≥ S (price 100k, bucket 1): P = face (a synthetic dollar), N > 0.
//!   x < S (price  40k, bucket 0): P = all of `Q` (shortfall), N = 0 → the
//!            settle_tx carries no LP change output.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::taproot::{LeafVersion, TapLeafHash};
use bitcoin::{Amount, OutPoint, TapSighashType, TxOut};
use satusd_oracle::oracle::Oracle;
use satusd_rail::encode::tagged_hash;
use satusd_rail0::builder::{apply_anchor_out_info, vpsbt_anchor_info, vpsbt_anchor_input, ANCHOR_DUST_SATS};
use satusd_rail1::cet::{bucket_adaptor_point, bucket_of, bucket_secret};
use satusd_rail1::funding::refund_leaf_script;
use satusd_tapd_client::assetwalletrpc as awrpc;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};
use satusd_vault::musig::{adapt_keyspend, aggregate_internal_x, cosign_keyspend_adaptor};
use secp256k1::{Secp256k1, SecretKey};

const M: u8 = 4;
const FUND_UNITS: u64 = 5_000;
const FEE_SATS: u64 = 2_000;
const DUST: u64 = 330;
const COLLATERAL_SATS: u64 = 2_000_000; // ~$2000 of BTC: over-collateralises a $1000 P leg
const MINT_MICRO_USD: u64 = 1_000_000_000; // $1,000 face

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn env() -> satusd_tapd_client::env::NodeEnv {
    satusd_tapd_client::env::NodeEnv::from_env(root())
}
fn bcli(args: &[&str]) -> serde_json::Value {
    env().bcli(args)
}
fn new_spk(label: &str) -> String {
    let a = bcli(&["getnewaddress", label, "bech32"]);
    bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"].as_str().unwrap().to_string()
}
fn compressed_even(x: &[u8; 32]) -> Vec<u8> {
    let mut v = vec![0x02];
    v.extend_from_slice(x);
    v
}
fn face_sats(price_usd: u32) -> u64 {
    MINT_MICRO_USD * 100_000_000 / (u64::from(price_usd) * 1_000_000)
}

/// One option position's full life: mint pair → lock 2-of-2 MuSig2 `Q` →
/// pre-sign the MuSig2-adaptor maturity CET → settle unilaterally at the
/// attested `price_usd`. `label`/`tag` keep keys + asset names unique.
async fn run_option(price_usd: u32, tag: &str) -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "regtest-only");
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let secp = Secp256k1::new();

    // ===== A — the P-leg note: a fresh GROUPED SatUSD UTXO =====
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let name = format!("SatUSD-opt-{tag}-{ts}");
    tapcli(&["assets", "mint", "--type", "normal", "--name", &name, "--supply",
        &FUND_UNITS.to_string(), "--new_grouped_asset"]);
    tapcli(&["assets", "mint", "finalize"]);
    bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let assets = tap.list_assets(taprpc::ListAssetRequest::default()).await?.into_inner().assets;
    let asset = assets.iter().find(|a| a.asset_genesis.as_ref().is_some_and(|g| g.name == name))
        .ok_or("fresh asset not found")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();
    let group_key: [u8; 33] = asset.asset_group.as_ref().ok_or("no group")?
        .tweaked_group_key.clone().try_into().map_err(|_| "group key len")?;

    // ===== Q — 2-of-2 MuSig2(LP, holder) over-collateralised pure-BTC DLC
    //       output, single-leaf {refund} (2-of-2 holder+LP CSV) =====
    let lp_sk = tagged_hash("devnet/opt-lp", name.as_bytes());
    let holder_sk = tagged_hash("devnet/opt-holder", name.as_bytes());
    let lp_x = SecretKey::from_byte_array(lp_sk)?.x_only_public_key(&secp).0.serialize();
    let holder_x = SecretKey::from_byte_array(holder_sk)?.x_only_public_key(&secp).0.serialize();
    let internal_x = aggregate_internal_x(&lp_sk, &holder_sk); // P = KeyAgg(LP, holder)
    let refund = refund_leaf_script(144, &lp_x, &holder_x);
    let merkle_root: [u8; 32] =
        TapLeafHash::from_script(&refund, LeafVersion::TapScript).to_byte_array();
    let q_output_x = satusd_crypto::nums::tap_tweak(&internal_x, &merkle_root);
    let q_spk = format!("5120{}", hex::encode(q_output_x));
    let q_addr = bcli(&["decodescript", &q_spk])["address"].as_str().ok_or("q addr")?.to_string();
    let q_txid = bcli(&["sendtoaddress", &q_addr, &format!("{:.8}", COLLATERAL_SATS as f64 / 1e8)])
        .as_str().ok_or("send q")?.to_string();
    bcli(&["-generate", "2"]);
    let q_raw = bcli(&["getrawtransaction", &q_txid, "true"]);
    let q_vout = q_raw["vout"].as_array().unwrap().iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(q_spk.as_str())).ok_or("q vout")?;
    let q_outpoint = OutPoint::new(q_txid.parse()?, q_vout["n"].as_u64().unwrap() as u32);
    let q_txout = TxOut { value: Amount::from_sat(COLLATERAL_SATS), script_pubkey: bitcoin::ScriptBuf::from_hex(&q_spk)? };
    // Custody gap closed: Q is the joint key, not either party's own.
    assert_ne!(q_output_x, lp_x, "Q is not the LP's key");
    assert_ne!(q_output_x, holder_x, "Q is not the holder's key");
    println!("[{tag}] Q (2-of-2 MuSig2 collateral): {COLLATERAL_SATS} sats at {}", hex::encode(q_output_x));

    // ===== burn sink for A (deterministic NUMS, family-keyed) =====
    let family = satusd_types::derive::asset_family_id(&asset_id.clone().try_into().unwrap(), &group_key, 0);
    let sink = satusd_crypto::nums::protocol_sink_script_key(&family);
    let burn_internal = satusd_crypto::nums::protocol_burn_internal_key(&family);
    let burn_tweak = satusd_crypto::nums::protocol_burn_tweak(&family);
    let burn_addr = tap.new_addr(taprpc::NewAddrRequest {
        asset_id: asset_id.clone(),
        amt: FUND_UNITS,
        script_key: Some(taprpc::ScriptKey {
            pub_key: sink.to_vec(),
            key_desc: Some(taprpc::KeyDescriptor { raw_key_bytes: compressed_even(&burn_internal), key_loc: None }),
            tap_tweak: burn_tweak.to_vec(),
            r#type: taprpc::ScriptKeyType::ScriptKeyScriptPathExternal as i32,
        }),
        internal_key: Some(taprpc::KeyDescriptor { raw_key_bytes: compressed_even(&burn_internal), key_loc: None }),
        skip_proof_courier_conn_check: true,
        ..Default::default()
    }).await?.into_inner();

    // ===== settle_tx: fund A → burn sink; anchor [A, Q] → [burn, P, N?] =====
    let funded = wallet.fund_virtual_psbt(awrpc::FundVirtualPsbtRequest {
        template: Some(awrpc::fund_virtual_psbt_request::Template::Raw(awrpc::TxTemplate {
            recipients: [(burn_addr.encoded.clone(), 0u64)].into_iter().collect(),
            ..Default::default()
        })),
        ..Default::default()
    }).await?.into_inner();
    let anchor_in = vpsbt_anchor_input(&funded.funded_psbt)?;
    let anchor_outs = vpsbt_anchor_info(&funded.funded_psbt)?;
    let signed = wallet.sign_virtual_psbt(awrpc::SignVirtualPsbtRequest {
        funded_psbt: funded.funded_psbt.clone(),
    }).await?.into_inner();

    let a_txout = TxOut { value: Amount::from_sat(anchor_in.value_sats), script_pubkey: anchor_in.script.clone() };
    let n_slots = anchor_outs.len() as u64;
    let total_in = anchor_in.value_sats + COLLATERAL_SATS;
    let avail = total_in - ANCHOR_DUST_SATS * n_slots - FEE_SATS; // BTC for P + N

    // The option payout: P = min(avail, face); N = avail − P. P + N ≡ avail
    // (no liquidation). N = 0 (shortfall) ⇒ no LP change output.
    let face = face_sats(price_usd);
    let p_holder = face.min(avail);
    let n_lp = avail - p_holder;
    let has_change = n_lp > DUST;

    let holder_spk = new_spk(&format!("opt-{tag}-holder"));
    let lp_spk = new_spk(&format!("opt-{tag}-lp"));

    let mut tx_outs: Vec<TxOut> = anchor_outs.iter().map(|_| TxOut {
        value: Amount::from_sat(ANCHOR_DUST_SATS),
        script_pubkey: anchor_in.script.clone(), // placeholder; tapd rewrites from anchor info
    }).collect();
    tx_outs.push(TxOut { value: Amount::from_sat(p_holder), script_pubkey: bitcoin::ScriptBuf::from_hex(&holder_spk)? });
    let holder_index = (tx_outs.len() - 1) as i32;
    if has_change {
        tx_outs.push(TxOut { value: Amount::from_sat(n_lp), script_pubkey: bitcoin::ScriptBuf::from_hex(&lp_spk)? });
    }
    // The anchor change output: the LP/N output when it exists, else the
    // holder output (skip_funding ⇒ tapd does not alter its value).
    let change_index = if has_change { holder_index + 1 } else { holder_index };

    let tx = bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![
            bitcoin::TxIn { previous_output: anchor_in.outpoint, sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME, ..Default::default() },
            bitcoin::TxIn { previous_output: q_outpoint, sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME, ..Default::default() },
        ],
        output: tx_outs,
    };
    let mut template = Psbt::from_unsigned_tx(tx)?;
    template.inputs[0].witness_utxo = Some(a_txout.clone());
    template.inputs[0].tap_internal_key = Some(anchor_in.internal_key);
    if let Some(r) = anchor_in.merkle_root {
        template.inputs[0].tap_merkle_root = Some(bitcoin::TapNodeHash::from_byte_array(r));
    }
    template.inputs[1].witness_utxo = Some(q_txout.clone()); // Q: MuSig2-adaptor keyspend injected later
    for (i, info) in anchor_outs.iter().enumerate() {
        apply_anchor_out_info(&mut template.outputs[i], info);
    }

    let committed = wallet.commit_virtual_psbts(awrpc::CommitVirtualPsbtsRequest {
        virtual_psbts: vec![signed.signed_psbt],
        passive_asset_psbts: funded.passive_asset_psbts.clone(),
        anchor_psbt: template.serialize(),
        anchor_change_output: Some(awrpc::commit_virtual_psbts_request::AnchorChangeOutput::ExistingOutputIndex(change_index)),
        skip_funding: true,
        ..Default::default()
    }).await?.into_inner();

    // ===== ISSUANCE: LP + holder co-sign Q's maturity CET as ONE 2-of-2
    //       MuSig2 adaptor, before the outcome =====
    let oracle = Oracle::from_seed(&tagged_hash("devnet/opt-oracle", name.as_bytes()))?;
    let tick = ts + 60;
    let ann = oracle.announce(tick)?;
    let bucket = bucket_of(price_usd, M);
    let committed_psbt = Psbt::deserialize(&committed.anchor_psbt)?;
    let sighash = SighashCache::new(&committed_psbt.unsigned_tx).taproot_key_spend_signature_hash(
        1, // Q is input 1
        &Prevouts::All(&[a_txout.clone(), q_txout.clone()]),
        TapSighashType::Default,
    )?.to_byte_array();
    let point = bucket_adaptor_point(&ann, &oracle.pubkey, M, bucket)?;
    let adaptor = cosign_keyspend_adaptor(&lp_sk, &holder_sk, &merkle_root, &sighash, &point.serialize());
    println!("[{tag}] PRESIGN(2-of-2): maturity CET co-signed by LP+holder as one MuSig2 adaptor \
        (bucket {bucket}; P={p_holder} + N={n_lp})");

    // ===== MATURITY: oracle attests → adapt the joint adaptor → holder
    //       signs A → broadcasts; NO LP, NO keeper =====
    let att = oracle.attest(tick, price_usd)?;
    let secret = bucket_secret(&att, M, bucket)?;
    let q_sig = adapt_keyspend(&adaptor, &secret);
    secp.verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(q_sig),
        &sighash, &secp256k1::XOnlyPublicKey::from_byte_array(q_output_x)?)?;

    let mut final_psbt = committed_psbt.clone();
    final_psbt.inputs[1].tap_key_sig = Some(bitcoin::taproot::Signature {
        signature: bitcoin::secp256k1::schnorr::Signature::from_slice(&q_sig)?,
        sighash_type: TapSighashType::Default,
    });
    let processed = bcli(&["walletprocesspsbt", &BASE64_STANDARD.encode(final_psbt.serialize())]);
    let lnd_channel = env.lnd_channel().await?;
    let mut lnd_wallet = satusd_tapd_client::WalletKitClient::new(lnd_channel);
    let lnd_signed = lnd_wallet.sign_psbt(satusd_tapd_client::walletrpc::SignPsbtRequest {
        funded_psbt: BASE64_STANDARD.decode(processed["psbt"].as_str().unwrap())?,
    }).await?.into_inner();
    let finalized = bcli(&["finalizepsbt", &BASE64_STANDARD.encode(&lnd_signed.signed_psbt), "false"]);
    assert_eq!(finalized["complete"].as_bool(), Some(true), "both inputs finalize: {finalized}");
    let final_bytes = BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?;

    let resp = wallet.publish_and_log_transfer(awrpc::PublishAndLogRequest {
        anchor_psbt: final_bytes,
        virtual_psbts: committed.virtual_psbts.clone(),
        passive_asset_psbts: committed.passive_asset_psbts.clone(),
        change_output_index: committed.change_output_index,
        lnd_locked_utxos: committed.lnd_locked_utxos.clone(),
        ..Default::default()
    }).await?.into_inner();
    let transfer = resp.transfer.ok_or("no transfer")?;
    assert!(transfer.outputs.iter().any(|o| o.script_key.ends_with(&sink)),
        "the note's SatUSD must land on the burn sink");
    bcli(&["-generate", "2"]);

    // ===== assertions: burn ⟺ payout in ONE tx; P + N conserve avail =====
    let tx = committed_psbt.unsigned_tx.clone();
    let has = |spk: &str, v: u64| tx.output.iter().any(|o|
        o.script_pubkey == bitcoin::ScriptBuf::from_hex(spk).unwrap() && o.value.to_sat() == v);
    assert!(has(&holder_spk, p_holder), "holder gets P = min(avail, face)");
    if has_change {
        assert!(has(&lp_spk, n_lp), "LP gets N = avail − P");
    } else {
        assert!(!tx.output.iter().any(|o| o.script_pubkey == bitcoin::ScriptBuf::from_hex(&lp_spk).unwrap()),
            "shortfall: no LP change output");
    }
    assert_eq!(p_holder + n_lp, avail, "P + N = Q's distributable collateral (no liquidation)");
    let regime = if face <= avail { "x>=S: P=face (synthetic dollar)" } else { "x<S: P=all Q (shortfall)" };
    println!("[{tag}] OPTION SETTLE @ ${price_usd} [{regime}]: burned A + holder {p_holder} sats \
        + LP {n_lp} sats, ONE unilateral tx on 2-of-2 MuSig2 Q — no LP, no keeper at maturity.");
    Ok(())
}

fn tapcli(args: &[&str]) -> String {
    env().tapcli(args)
}

/// Both option-payout regimes, run sequentially (devnet tests share one
/// tapd/lnd wallet — they must not run in parallel):
///   x ≥ S (price $100k, bucket 1): the P leg is a synthetic dollar — it
///     receives `face` BTC value, the N/LP leg keeps the over-collateral cushion.
///   x < S (price  $40k, bucket 0): the P leg tracks BTC down — it receives
///     ALL of `Q` (a shortfall below the strike), with no LP change output.
#[tokio::test]
#[ignore = "requires live devnet (make devnet-up)"]
async fn option_pair_mint_and_settle_both_regimes() -> Result<(), Box<dyn std::error::Error>> {
    run_option(100_000, "hi").await?; // x >= S: synthetic dollar
    run_option(40_000, "lo").await?; // x < S: shortfall
    Ok(())
}
