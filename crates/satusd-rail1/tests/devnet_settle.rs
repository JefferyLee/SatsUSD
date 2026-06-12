//! J4 settle E2E: the full Rail-1 redemption on a live devnet.
//!
//! ```text
//! mint fresh asset → lock it in OUR DLC funding output (J4 landing)
//!   → oracle announces tick T
//!   → asset-level vPSBT funded+signed (spending the funding output)
//!   → per-bucket CETs committed with skip_funding (fully
//!     deterministic anchor txs) → adaptor presign per bucket,
//!     BEFORE the outcome exists
//!   → oracle attests → ONLY the winning bucket's secret decrypts
//!   → decrypted keyspend goes in as tap_key_sig → finalize →
//!     PublishAndLogTransfer → confirmed
//! ```
//!
//! v0 scope notes: two buckets are exercised (winner + loser) to
//! prove per-bucket sighash binding and selective decryption;
//! production presigns all 2^m. The LP's BTC input is signed at
//! settle time (in the full protocol it is locked at LOCK — the
//! lock-side completion is MuSig2 work, the declared upgrade).
//!
//! Run with a live devnet:
//!
//! ```text
//! cargo test -p satusd-rail1 --test devnet_settle -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::{Amount, OutPoint, TapSighashType, TxOut};
use satusd_oracle::oracle::Oracle;
use satusd_rail::encode::tagged_hash;
use satusd_rail0::builder::{vpsbt_anchor_info, vpsbt_anchor_input, ANCHOR_DUST_SATS};
use satusd_rail1::adaptor::{decrypt, presign, verify_presig, AdaptorSig};
use satusd_rail1::cet::{bucket_adaptor_point, bucket_of, bucket_secret};
use satusd_rail1::funding::{
    funding_output, keyspend_secret, refund_leaf_script, sibling_preimage,
};
use satusd_tapd_client::assetwalletrpc as awrpc;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};
use secp256k1::{Secp256k1, SecretKey};

const FUND_UNITS: u64 = 5_000;
const M: u8 = 4;
const FEE_SATS: u64 = 2_000;
const ORACLE_PRICE: u32 = 100_000;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bcli(args: &[&str]) -> serde_json::Value {
    satusd_tapd_client::env::NodeEnv::from_env(root()).bcli(args)
}

fn tapcli(args: &[&str]) -> String {
    satusd_tapd_client::env::NodeEnv::from_env(root()).tapcli(args)
}

fn compressed_even(x: &[u8; 32]) -> Vec<u8> {
    let mut v = vec![0x02];
    v.extend_from_slice(x);
    v
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up)"]
async fn j4_settle() -> Result<(), Box<dyn std::error::Error>> {
    let _r = root();
    let env = satusd_tapd_client::env::NodeEnv::from_env(root());
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let secp = Secp256k1::new();

    // ---- 0. fresh asset: guarantees coin selection picks OUR
    //         funding utxo for the CET's asset leg ----
    let ts0 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let name = format!("SatUSD-J4-{ts0}");
    tapcli(&[
        "assets",
        "mint",
        "--type",
        "normal",
        "--name",
        &name,
        "--supply",
        &FUND_UNITS.to_string(),
    ]);
    tapcli(&["assets", "mint", "finalize"]);
    bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| a.asset_genesis.as_ref().is_some_and(|g| g.name == name))
        .ok_or("fresh asset not found")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();
    println!("minted {name}: {}", hex::encode(&asset_id));

    // ---- 1. lock it in our DLC funding output ----
    let funding_sk = tagged_hash("devnet/j4-settle-funding", name.as_bytes());
    let (funding_px, _) = SecretKey::from_byte_array(funding_sk)
        .unwrap()
        .x_only_public_key(&secp);
    let internal_x = funding_px.serialize();
    let refund = refund_leaf_script(
        144,
        &tagged_hash("devnet/user-x", b"u"),
        &tagged_hash("devnet/lp-x", b"l"),
    );
    let script_key = wallet
        .next_script_key(awrpc::NextScriptKeyRequest { key_family: 212 })
        .await?
        .into_inner()
        .script_key
        .ok_or("no script key")?;
    let fund_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.clone(),
            amt: FUND_UNITS,
            script_key: Some(script_key),
            internal_key: Some(taprpc::KeyDescriptor {
                raw_key_bytes: compressed_even(&internal_x),
                key_loc: None,
            }),
            tapscript_sibling: sibling_preimage(&refund),
            ..Default::default()
        })
        .await?
        .into_inner();
    let send = tap
        .send_asset(taprpc::SendAssetRequest {
            tap_addrs: vec![fund_addr.encoded],
            ..Default::default()
        })
        .await?
        .into_inner();
    let transfer = send.transfer.ok_or("no transfer")?;
    bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let fund_out = transfer
        .outputs
        .iter()
        .find(|o| {
            o.anchor
                .as_ref()
                .is_some_and(|a| a.internal_key == compressed_even(&internal_x))
        })
        .ok_or("funding output not in transfer")?;
    let anchor = fund_out.anchor.as_ref().unwrap();
    let ta_leaf: [u8; 32] = anchor.taproot_asset_root.clone().try_into().unwrap();
    let fund = funding_output(&internal_x, &ta_leaf, &refund);
    let (fund_txid, fund_vout) = anchor.outpoint.split_once(':').unwrap();
    let funding_outpoint = OutPoint::new(fund_txid.parse()?, fund_vout.parse()?);
    let fund_value = Amount::from_sat(anchor.value as u64);
    let fund_txout = TxOut {
        value: fund_value,
        script_pubkey: bitcoin::ScriptBuf::from_hex(&format!(
            "5120{}",
            hex::encode(fund.output_x)
        ))?,
    };
    println!(
        "funding locked: {funding_outpoint} Q={}",
        hex::encode(fund.output_x)
    );

    // ---- 2. oracle announces the tick ----
    let oracle = Oracle::from_seed(&tagged_hash("devnet/j4-oracle", b"seed"))?;
    let tick = ts0 + 60;
    let ann = oracle.announce(tick)?;

    // ---- 3. asset-level CET leg: fund + sign ONE vPSBT moving the
    //         asset out of the funding output ----
    let dest_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.clone(),
            amt: FUND_UNITS,
            ..Default::default()
        })
        .await?
        .into_inner()
        .encoded;
    let funded = wallet
        .fund_virtual_psbt(awrpc::FundVirtualPsbtRequest {
            template: Some(awrpc::fund_virtual_psbt_request::Template::Raw(
                awrpc::TxTemplate {
                    recipients: [(dest_addr, 0u64)].into_iter().collect(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        })
        .await?
        .into_inner();
    let anchor_in = vpsbt_anchor_input(&funded.funded_psbt)?;
    assert_eq!(
        anchor_in.outpoint, funding_outpoint,
        "fresh asset forces selection of the funding utxo"
    );
    let anchor_outs = vpsbt_anchor_info(&funded.funded_psbt)?;
    let signed = wallet
        .sign_virtual_psbt(awrpc::SignVirtualPsbtRequest {
            funded_psbt: funded.funded_psbt.clone(),
        })
        .await?
        .into_inner();

    // ---- 4. LP leg + payout addresses ----
    let unspent = bcli(&["listunspent"]);
    let lp_utxo = unspent
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["amount"].as_f64().unwrap() >= 0.05)
        .ok_or("no LP utxo")?;
    let lp_outpoint = OutPoint::new(
        lp_utxo["txid"].as_str().unwrap().parse()?,
        lp_utxo["vout"].as_u64().unwrap() as u32,
    );
    let lp_txout = TxOut {
        value: Amount::from_btc(lp_utxo["amount"].as_f64().unwrap())?,
        script_pubkey: bitcoin::ScriptBuf::from_hex(lp_utxo["scriptPubKey"].as_str().unwrap())?,
    };
    let payout_script = {
        let a = bcli(&["getnewaddress", "j4-user", "bech32"]);
        bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let lp_change_script = {
        let a = bcli(&["getnewaddress", "j4-lp-change", "bech32"]);
        bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // ---- 5. per-bucket CETs: commit (skip_funding → fully
    //         deterministic) + adaptor presign, BEFORE the outcome ----
    let tweaked = keyspend_secret(&funding_sk, &fund.merkle_root)?;
    let (qx, _) = SecretKey::from_byte_array(tweaked)
        .unwrap()
        .x_only_public_key(&secp);
    assert_eq!(qx.serialize(), fund.output_x);

    let winner = bucket_of(ORACLE_PRICE, M);
    let loser = winner ^ 1;
    struct BucketCet {
        bucket: u32,
        committed_anchor: Vec<u8>,
        vpsbts: Vec<Vec<u8>>,
        passives: Vec<Vec<u8>>,
        presig: AdaptorSig,
        sighash: [u8; 32],
    }
    let mut cets: Vec<BucketCet> = Vec::new();
    for &b in &[winner, loser] {
        let user_sats = 500_000 + u64::from(b); // distinct per bucket
        let lp_change = lp_txout.value.to_sat() - user_sats - FEE_SATS;

        // Anchor template: [funding, LP] in; [asset slots ‖ payout ‖
        // LP change] out.
        let mut tx_outs: Vec<TxOut> = anchor_outs
            .iter()
            .map(|_| TxOut {
                value: Amount::from_sat(ANCHOR_DUST_SATS),
                script_pubkey: fund_txout.script_pubkey.clone(), // placeholder P2TR
            })
            .collect();
        tx_outs.push(TxOut {
            value: Amount::from_sat(user_sats),
            script_pubkey: bitcoin::ScriptBuf::from_hex(&payout_script)?,
        });
        let change_index = tx_outs.len() as i32;
        tx_outs.push(TxOut {
            value: Amount::from_sat(lp_change),
            script_pubkey: bitcoin::ScriptBuf::from_hex(&lp_change_script)?,
        });
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![
                bitcoin::TxIn {
                    previous_output: funding_outpoint,
                    sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                    ..Default::default()
                },
                bitcoin::TxIn {
                    previous_output: lp_outpoint,
                    sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                    ..Default::default()
                },
            ],
            output: tx_outs,
        };
        let mut template = Psbt::from_unsigned_tx(tx)?;
        template.inputs[0].witness_utxo = Some(fund_txout.clone());
        template.inputs[0].tap_internal_key =
            Some(bitcoin::XOnlyPublicKey::from_slice(&internal_x)?);
        template.inputs[0].tap_merkle_root = Some(bitcoin::taproot::TapNodeHash::from_byte_array(
            fund.merkle_root,
        ));
        template.inputs[1].witness_utxo = Some(lp_txout.clone());
        for (i, info) in anchor_outs.iter().enumerate() {
            satusd_rail0::builder::apply_anchor_out_info(&mut template.outputs[i], info);
        }

        let committed = wallet
            .commit_virtual_psbts(awrpc::CommitVirtualPsbtsRequest {
                virtual_psbts: vec![signed.signed_psbt.clone()],
                passive_asset_psbts: funded.passive_asset_psbts.clone(),
                anchor_psbt: template.serialize(),
                anchor_change_output: Some(
                    awrpc::commit_virtual_psbts_request::AnchorChangeOutput::ExistingOutputIndex(
                        change_index,
                    ),
                ),
                skip_funding: true,
                ..Default::default()
            })
            .await?
            .into_inner();

        // The committed anchor tx is fully determined → its keyspend
        // sighash is the CET message.
        let committed_psbt = Psbt::deserialize(&committed.anchor_psbt)?;
        let mut cache = SighashCache::new(&committed_psbt.unsigned_tx);
        let sighash = cache.taproot_key_spend_signature_hash(
            0,
            &Prevouts::All(&[fund_txout.clone(), lp_txout.clone()]),
            TapSighashType::Default,
        )?;
        let msg: [u8; 32] = sighash.to_byte_array();

        let point = bucket_adaptor_point(&ann, &oracle.pubkey, M, b)?;
        let nonce_base = tagged_hash("devnet/j4-cet-nonce", &b.to_be_bytes());
        let presig = presign(&tweaked, &nonce_base, &msg, &point)?;
        assert!(verify_presig(&presig, &fund.output_x, &msg, &point)?);

        cets.push(BucketCet {
            bucket: b,
            committed_anchor: committed.anchor_psbt,
            vpsbts: committed.virtual_psbts,
            passives: committed.passive_asset_psbts,
            presig,
            sighash: msg,
        });
        println!("bucket {b}: CET committed + presigned (user_sats={user_sats})");
    }

    // ---- 6. the outcome arrives ----
    let att = oracle.attest(tick, ORACLE_PRICE)?;
    println!("attested: {} → bucket {winner}", ORACLE_PRICE);

    // Selective decryption: the loser bucket has NO valid secret.
    assert!(bucket_secret(&att, M, loser).is_err());

    let t = bucket_secret(&att, M, winner)?;
    let win = cets.iter().find(|c| c.bucket == winner).unwrap();
    let sig = decrypt(&win.presig, &t)?;
    let q = secp256k1::XOnlyPublicKey::from_byte_array(fund.output_x)?;
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(sig),
        &win.sighash,
        &q,
    )?;

    // ---- 7. assemble + broadcast the winning CET ----
    let mut final_psbt = Psbt::deserialize(&win.committed_anchor)?;
    final_psbt.inputs[0].tap_key_sig = Some(bitcoin::taproot::Signature {
        signature: bitcoin::secp256k1::schnorr::Signature::from_slice(&sig)?,
        sighash_type: TapSighashType::Default,
    });
    let processed = bcli(&[
        "walletprocesspsbt",
        &BASE64_STANDARD.encode(final_psbt.serialize()),
    ]);
    let finalized = bcli(&["finalizepsbt", processed["psbt"].as_str().unwrap(), "false"]);
    assert_eq!(
        finalized["complete"].as_bool(),
        Some(true),
        "both inputs finalize: {finalized}"
    );
    let final_bytes = BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?;

    let resp = wallet
        .publish_and_log_transfer(awrpc::PublishAndLogRequest {
            anchor_psbt: final_bytes,
            virtual_psbts: win.vpsbts.clone(),
            passive_asset_psbts: win.passives.clone(),
            change_output_index: -1,
            ..Default::default()
        })
        .await?
        .into_inner();
    let settle_transfer = resp.transfer.ok_or("no transfer in publish")?;
    println!("CET broadcast: {}", {
        let mut h = settle_transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    });
    bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // ---- 8. confirm: payout on-chain + asset moved ----
    let tx = Psbt::deserialize(&win.committed_anchor)?
        .unsigned_tx
        .clone();
    let payout_present = tx.output.iter().any(|o| {
        o.script_pubkey == bitcoin::ScriptBuf::from_hex(&payout_script).unwrap()
            && o.value.to_sat() == 500_000 + u64::from(winner)
    });
    assert!(payout_present, "winning bucket's payout is in the CET");

    let total: u64 = tap
        .list_assets(taprpc::ListAssetRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .assets
        .iter()
        .filter(|a| a.asset_genesis.as_ref().map(|g| g.asset_id.as_slice()) == Some(&asset_id[..]))
        .map(|a| a.amount)
        .sum();
    assert_eq!(total, FUND_UNITS, "asset conserved through the CET");

    println!(
        "J4 SETTLED: oracle-gated CET spent the DLC funding output by key path — \
         {} units moved + {} sats paid, bucket {winner} of 2^{M}",
        FUND_UNITS,
        500_000 + u64::from(winner)
    );
    Ok(())
}
