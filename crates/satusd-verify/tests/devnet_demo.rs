//! The M-1 trustless core, end to end, as one narrated story (run with
//! `--nocapture`). It ties together everything Phase-1 built:
//!
//!   ACT 1  a decentralised FROST oracle cohort runs DKG → one group key
//!   ACT 2  an LP mints a 2-of-2 MuSig2 option position + pre-signs the
//!          maturity CET as a MuSig2 adaptor anticipating the cohort
//!   ACT 3  the buyer verifies the backing client-side BEFORE paying (FR-6)
//!   ACT 4  at maturity the cohort attests the median; anyone settles the
//!          two-input burn⟺claim unilaterally — no LP, no keeper
//!
//! `satusd-verify` sits at the top of the M-1 dependency graph, so its test
//! can drive all of vault (MuSig2), rail1 (DLC buckets), oracle (FROST),
//! and verify (FR-6) at once. Real on-chain; the assertions are the proof,
//! the prints are the demo.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::taproot::{LeafVersion, TapLeafHash};
use bitcoin::{Amount, OutPoint, TapSighashType, TxOut};
use satusd_oracle::frost::Cohort;
use satusd_oracle::median::{stake_weighted_median, Report};
use satusd_rail::encode::tagged_hash;
use satusd_rail0::builder::{apply_anchor_out_info, vpsbt_anchor_info, vpsbt_anchor_input, ANCHOR_DUST_SATS};
use satusd_rail1::cet::{bucket_adaptor_point, bucket_of, bucket_secret};
use satusd_rail1::funding::refund_leaf_script;
use satusd_tapd_client::assetwalletrpc as awrpc;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};
use satusd_vault::contract::{cr_bps, VaultTerms};
use satusd_vault::musig::{adapt_keyspend, aggregate_internal_x, cosign_keyspend_adaptor};
use satusd_verify::position::{payout_at, verify_backing, verify_cet_adaptor, OptionPosition, QView};
use secp256k1::{Secp256k1, SecretKey};

const M: u8 = 4;
const FUND_UNITS: u64 = 5_000;
const FEE_SATS: u64 = 2_000;
const DUST: u64 = 330;
const COLLATERAL_SATS: u64 = 2_000_000; // ~$2000 of BTC backing a $1000 P leg
const MINT_MICRO_USD: u64 = 1_000_000_000; // $1,000 face
const SETTLE_PRICE: u32 = 100_000; // x ≥ strike → the P leg is a synthetic dollar

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn env() -> satusd_tapd_client::env::NodeEnv {
    satusd_tapd_client::env::NodeEnv::from_env(root())
}
fn bcli(args: &[&str]) -> serde_json::Value {
    env().bcli(args)
}
fn tapcli(args: &[&str]) -> String {
    env().tapcli(args)
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

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up); run with --nocapture for the narration"]
async fn demo_end_to_end_trustless_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let env = env();
    assert_eq!(env.chain, "regtest", "regtest-only");
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let secp = Secp256k1::new();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    println!("\n═══════════ SatUSD M-1 — end-to-end trustless lifecycle ═══════════\n");

    // ───────── ACT 1: the decentralised oracle ─────────
    println!("ACT 1 — the decentralised oracle");
    let cohort = Cohort::keygen(5, 3, &tagged_hash("demo/cohort", &ts.to_be_bytes()))
        .map_err(|e| format!("dkg: {e:?}"))?;
    let quorum = [1u16, 3, 5];
    println!("  5 independent oracles ran a Pedersen DKG (no trusted dealer).");
    println!("  group key PK = {}", hex::encode(cohort.group_pubkey));
    println!("  a 3-of-5 quorum {quorum:?} will attest; no single oracle sets the price.\n");

    // ───────── ACT 2: issuance ─────────
    println!("ACT 2 — issuance: the LP sells a $1000 BTC/USD P-leg position");
    let name = format!("SatUSD-demo-{ts}");
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

    let lp_sk = tagged_hash("demo/lp", name.as_bytes());
    let holder_sk = tagged_hash("demo/holder", name.as_bytes());
    let lp_x = SecretKey::from_byte_array(lp_sk)?.x_only_public_key(&secp).0.serialize();
    let holder_x = SecretKey::from_byte_array(holder_sk)?.x_only_public_key(&secp).0.serialize();
    let lp_pub = SecretKey::from_byte_array(lp_sk)?.public_key(&secp).serialize();
    let holder_pub = SecretKey::from_byte_array(holder_sk)?.public_key(&secp).serialize();
    let internal_x = aggregate_internal_x(&lp_sk, &holder_sk);
    let refund = refund_leaf_script(144, &lp_x, &holder_x);
    let merkle_root: [u8; 32] = TapLeafHash::from_script(&refund, LeafVersion::TapScript).to_byte_array();
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
    println!("  collateral Q = 2-of-2 MuSig2(LP, holder) = {}", hex::encode(q_output_x));
    println!("  {COLLATERAL_SATS} sats locked (~{}% CR @ ${SETTLE_PRICE}); neither party can move Q alone.",
        cr_bps(COLLATERAL_SATS, MINT_MICRO_USD, SETTLE_PRICE) / 100);
    println!("  minted the P-leg note A (a Taproot Asset the buyer holds).");

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
    let avail = anchor_in.value_sats + COLLATERAL_SATS - ANCHOR_DUST_SATS * n_slots - FEE_SATS;
    let face = face_sats(SETTLE_PRICE);
    let p_holder = face.min(avail);
    let n_lp = avail - p_holder;
    let has_change = n_lp > DUST;
    let holder_spk = new_spk("demo-holder");
    let lp_spk = new_spk("demo-lp");

    let mut tx_outs: Vec<TxOut> = anchor_outs.iter().map(|_| TxOut {
        value: Amount::from_sat(ANCHOR_DUST_SATS),
        script_pubkey: anchor_in.script.clone(),
    }).collect();
    tx_outs.push(TxOut { value: Amount::from_sat(p_holder), script_pubkey: bitcoin::ScriptBuf::from_hex(&holder_spk)? });
    let holder_index = (tx_outs.len() - 1) as i32;
    if has_change {
        tx_outs.push(TxOut { value: Amount::from_sat(n_lp), script_pubkey: bitcoin::ScriptBuf::from_hex(&lp_spk)? });
    }
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
    template.inputs[1].witness_utxo = Some(q_txout.clone());
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

    // The LP + holder co-sign the maturity CET, anticipating the cohort.
    let tick = ts + 60;
    let ann = cohort.announce(tick, &quorum).map_err(|e| format!("announce: {e:?}"))?;
    let bucket = bucket_of(SETTLE_PRICE, M);
    let committed_psbt = Psbt::deserialize(&committed.anchor_psbt)?;
    let sighash = SighashCache::new(&committed_psbt.unsigned_tx).taproot_key_spend_signature_hash(
        1,
        &Prevouts::All(&[a_txout.clone(), q_txout.clone()]),
        TapSighashType::Default,
    )?.to_byte_array();
    let point = bucket_adaptor_point(&ann, &cohort.group_pubkey, M, bucket)?;
    let adaptor = cosign_keyspend_adaptor(&lp_sk, &holder_sk, &merkle_root, &sighash, &point.serialize());
    println!("  LP + holder co-signed the maturity CET as ONE MuSig2 adaptor, locked to the cohort's bucket {bucket}.\n");

    // ───────── ACT 3: the buyer verifies before paying (FR-6) ─────────
    println!("ACT 3 — the buyer verifies the backing client-side, BEFORE paying (no server of ours)");
    let position = OptionPosition {
        terms: VaultTerms {
            collateral_sats: COLLATERAL_SATS,
            mint_micro_usd: MINT_MICRO_USD,
            opening_cr_bps: 15_000,
            liq_cr_bps: 11_000,
            checkpoint_interval: 144,
            maturity_height: 1_000_000,
            m: M,
            penalty_bps: 500,
            oracle_event_series: [7u8; 32],
        },
        lp_pub,
        holder_pub,
        refund_csv: 144,
        refund_user_x: lp_x,
        refund_lp_x: holder_x,
    };
    let qview = QView { value_sats: COLLATERAL_SATS, script_pubkey: bitcoin::ScriptBuf::from_hex(&q_spk)? };
    let backing = verify_backing(&position, &qview, SETTLE_PRICE).map_err(|e| format!("backing: {e:?}"))?;
    println!("  ✓ verify_backing: Q is the buyer's own 2-of-2 key, holds {} sats, CR {}bps ≥ {}bps opening.",
        backing.collateral_sats, backing.cr_bps, position.terms.opening_cr_bps);
    let cet_ok = verify_cet_adaptor(&q_output_x, &sighash, &ann, &cohort.group_pubkey, M, bucket, &adaptor);
    assert!(cet_ok, "the pre-signed CET must verify under Q for the cohort's bucket");
    println!("  ✓ verify_cet_adaptor: the pre-signed CET is armed for the cohort's announced bucket, under Q.");
    let (p_preview, n_preview) = payout_at(&position.terms, SETTLE_PRICE);
    println!("  ✓ payout preview @ ${SETTLE_PRICE}: P={p_preview} + N={n_preview} = {} gross (P+N=Q, no liquidation;",
        p_preview + n_preview);
    println!("    the holder's P is paid in full, the LP/N leg absorbs the on-chain fee + anchor dust).\n");

    // ───────── ACT 4: maturity settlement ─────────
    println!("ACT 4 — maturity (the holder could have been offline the whole term)");
    let reports = [
        Report { price_usd: 98_000, stake: 20 },
        Report { price_usd: SETTLE_PRICE, stake: 60 },
        Report { price_usd: 103_000, stake: 20 },
    ];
    let median = stake_weighted_median(&reports).ok_or("median")?;
    assert_eq!(median, SETTLE_PRICE);
    println!("  the 3-of-5 quorum attests the stake-weighted median BTC/USD = ${median} (no single venue sets it).");
    let att = cohort.attest(tick, median, &quorum).map_err(|e| format!("attest: {e:?}"))?;
    let secret = bucket_secret(&att, M, bucket)?;
    let q_sig = adapt_keyspend(&adaptor, &secret);
    secp.verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(q_sig),
        &sighash, &secp256k1::XOnlyPublicKey::from_byte_array(q_output_x)?)?;
    println!("  the public attestation adapts the joint CET → a valid key-path spend of Q (no LP, no keeper).");

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

    let mined = bcli(&["getrawtransaction", &committed_psbt.unsigned_tx.compute_txid().to_string(), "true"]);
    assert!(mined["confirmations"].as_i64().unwrap_or(0) >= 1, "settle confirmed");
    let onchain = |spk: &str| -> Option<u64> {
        mined["vout"].as_array().unwrap().iter()
            .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(spk))
            .map(|o| Amount::from_btc(o["value"].as_f64().unwrap()).unwrap().to_sat())
    };
    assert_eq!(onchain(&holder_spk), Some(p_holder), "holder paid P on-chain");
    if has_change {
        assert_eq!(onchain(&lp_spk), Some(n_lp), "LP paid N on-chain");
    }
    println!("  ONE tx: burned A + holder received P={p_holder} sats (in full) + LP received N={n_lp} sats (net of fee+dust).");
    println!("  txid {}\n", committed_psbt.unsigned_tx.compute_txid());

    println!("✅ decentralised FROST oracle + trustless 2-of-2 collateral + client-verified backing");
    println!("   + unilateral maturity settlement — end to end, no trusted party in the loop.\n");
    Ok(())
}
