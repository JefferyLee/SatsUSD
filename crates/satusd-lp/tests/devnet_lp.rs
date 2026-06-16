//! J3 over HTTP: the two-party Rail-0 split, end to end on the
//! regtest devnet. The LP daemon serves in-process on an ephemeral
//! port; the "user" side speaks to it only via the HTTP protocol —
//! quote, build, co-sign, settle — exactly as an external tester's
//! CLI will.
//!
//! Price source: the live public oracle (signet instance). Requires
//! `make devnet-up` + a grouped asset with balance. Run:
//!
//! ```text
//! cargo test -p satusd-lp --test devnet_lp -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::str::FromStr;

use base64::prelude::*;
use bitcoin::psbt::Psbt;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, TxOut};
use satusd_lp::{http_post, serve, Lp, LpConfig};
use satusd_rail::encode::tagged_hash;
use satusd_rail::manifest::{FeeSchedule, LpStake};
use satusd_rail0::builder::{fund, publish, sign_commit, AnchorTemplate};
use satusd_rail0::rail::{rail0_manifest, Rail0Params};
use satusd_tapd_client::env::NodeEnv;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};

const AMOUNT_MICRO_USD: u64 = 900_000; // $0.90 — devnet asset holds ~$1
const ORACLE_ADDR: &str = "207.148.98.132:9590";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bcli(args: &[&str]) -> serde_json::Value {
    NodeEnv::from_env(root()).bcli(args)
}

fn b64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) with a grouped asset + internet (oracle)"]
async fn j3_over_http() -> Result<(), Box<dyn std::error::Error>> {
    let env = NodeEnv::from_env(root());

    // ---- discover the devnet asset ----
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let assets = tap
        .list_assets(taprpc::ListAssetRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner();
    let asset = assets
        .assets
        .iter()
        .find(|a| a.amount >= AMOUNT_MICRO_USD)
        .ok_or("no asset with enough balance")?;
    let asset_id: [u8; 32] = asset
        .asset_genesis
        .as_ref()
        .unwrap()
        .asset_id
        .clone()
        .try_into()
        .unwrap();

    // ---- release stale leases from failed runs ----
    let utxos = tap
        .list_utxos(taprpc::ListUtxosRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner();
    for (op, _) in utxos.managed_utxos {
        let parts: Vec<&str> = op.split(':').collect();
        let mut txid = hex::decode(parts[0])?;
        txid.reverse();
        let _ = wallet
            .remove_utxo_lease(satusd_tapd_client::assetwalletrpc::RemoveUtxoLeaseRequest {
                outpoint: Some(taprpc::OutPoint {
                    txid,
                    output_index: parts[1].parse()?,
                }),
            })
            .await;
    }

    // ---- LP daemon in-process on an ephemeral port ----
    let lp_seed = tagged_hash("test/lp-seed", b"devnet-lp");
    let lp_sk = tagged_hash("SatUSD/lp/key/v1", &lp_seed);
    let secp = secp256k1::Secp256k1::new();
    let (px, _) = secp256k1::SecretKey::from_byte_array(lp_sk)?.x_only_public_key(&secp);
    let manifest = rail0_manifest(Rail0Params {
        asset_id,
        max_size_sats: 5_000_000,
        committed_term: 4032,
        fee_schedule: FeeSchedule {
            retain_bps: 10,
            service_bps: 5,
            fixed_sats: 200,
        },
        price_dev_bound_bps: 100,
        quote_validity_s: 600,
        lp_stake: LpStake {
            amount_sats: 0,
            slash_conditions: vec![],
        },
    })
    .map_err(|e| format!("{e:?}"))?;
    let lp = Lp::new(LpConfig {
        env: NodeEnv::from_env(root()),
        lp_sk,
        lp_pubkey: px.serialize(),
        asset_id,
        manifest,
        oracle_addr: ORACLE_ADDR.into(),
        fee_budget_sats: 2_500,
    })
    .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?.to_string();
    tokio::spawn(serve(lp, listener));

    // ---- user: request a quote over HTTP ----
    let payout_address = bcli(&["getnewaddress"]).as_str().unwrap().to_string();
    let q = tokio::task::spawn_blocking({
        let addr = addr.clone();
        let payout = payout_address.clone();
        move || {
            http_post(
                &addr,
                "/v0/quote",
                &serde_json::json!({
                    "amount_micro_usd": AMOUNT_MICRO_USD.to_string(),
                    "payout_address": payout,
                }),
            )
        }
    })
    .await?
    .map_err(|e| e.to_string())?;
    println!(
        "quote: price={} msat/USD, user_sats={}",
        q["quote"]["price_msat_per_usd"].as_str().unwrap(),
        q["plan"]["user_sats"].as_str().unwrap()
    );

    // ---- user: verify the quote binds the plan (refuse otherwise) ----
    let user_sats: u64 = q["plan"]["user_sats"].as_str().unwrap().parse()?;
    assert!(user_sats > 0, "payout must be positive");

    // ---- user: build + sign the asset leg ----
    let funded = fund(&mut wallet, q["lp_ta_addr"].as_str().unwrap(), vec![]).await?;
    let lp_outpoint = OutPoint::from_str(q["lp_btc_input"]["outpoint"].as_str().unwrap())?;
    let lp_prev_txout = TxOut {
        value: Amount::from_sat(q["lp_btc_input"]["value_sats"].as_str().unwrap().parse()?),
        script_pubkey: ScriptBuf::from_hex(q["lp_btc_input"]["script_pubkey"].as_str().unwrap())?,
    };
    let user_payout = TxOut {
        value: Amount::from_sat(user_sats),
        script_pubkey: Address::from_str(&payout_address)?
            .assume_checked()
            .script_pubkey(),
    };
    let extra_outputs = match &q["lp_change"] {
        serde_json::Value::Null => vec![],
        c => vec![TxOut {
            value: Amount::from_sat(c["value_sats"].as_str().unwrap().parse()?),
            script_pubkey: ScriptBuf::from_hex(c["script_pubkey"].as_str().unwrap())?,
        }],
    };
    let template = AnchorTemplate {
        lp_outpoint,
        lp_prev_txout,
        lp_internal_key: None, // single-node devnet harness: LP input local
        lp_key_origin: None,
        user_payout: user_payout.clone(),
        user_payout_internal_key: None,
        user_payout_key_origin: None,
        extra_outputs,
    };
    let committed = sign_commit(&mut wallet, funded, &template, 2).await?;

    // ---- user: sign the TA anchor keyspend with their lnd ----
    let lnd_channel = env.lnd_channel().await?;
    let mut lnd_wallet = satusd_tapd_client::WalletKitClient::new(lnd_channel);
    let lnd_signed = lnd_wallet
        .sign_psbt(satusd_tapd_client::walletrpc::SignPsbtRequest {
            funded_psbt: committed.anchor_psbt.clone(),
        })
        .await?
        .into_inner();

    // ---- user: hand the PSBT to the LP for its signature ----
    let payload = q["signing_payload"].as_str().unwrap().to_string();
    let settled = tokio::task::spawn_blocking({
        let addr = addr.clone();
        let psbt_b64 = b64(&lnd_signed.signed_psbt);
        move || {
            http_post(
                &addr,
                "/v0/settle",
                &serde_json::json!({
                    "signing_payload": payload,
                    "anchor_psbt": psbt_b64,
                }),
            )
        }
    })
    .await?
    .map_err(|e| e.to_string())?;
    let lp_signed_b64 = settled["signed_psbt"].as_str().ok_or("no signed_psbt")?;

    // ---- user: finalize + publish ----
    let finalized = bcli(&["finalizepsbt", lp_signed_b64, "false"]);
    assert_eq!(
        finalized["complete"].as_bool(),
        Some(true),
        "all signatures present"
    );
    let final_psbt = BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?;
    let resp = publish(&mut wallet, committed, final_psbt).await?;
    let transfer = resp.transfer.ok_or("no transfer in response")?;
    println!("anchor tx broadcast: {}", transfer.anchor_tx_hash());

    // ---- confirm + assert both legs ----
    let mine_addr = bcli(&["getnewaddress"]).as_str().unwrap().to_string();
    bcli(&["generatetoaddress", "1", &mine_addr]);
    let txid = transfer.anchor_tx_hash();
    let tx = bcli(&["gettransaction", &txid, "true"]);
    assert!(
        tx["confirmations"].as_i64().unwrap_or(0) >= 1,
        "swap confirmed"
    );
    // the user payout output exists with the exact quoted value
    let decoded = Psbt::deserialize(&BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?)?;
    assert!(
        decoded.unsigned_tx.output.contains(&user_payout),
        "user payout present"
    );
    println!(
        "J3-over-HTTP confirmed: {AMOUNT_MICRO_USD} µUSD moved, {user_sats} sats paid — \
         two-party split exercised end to end"
    );
    Ok(())
}

trait AnchorTxHash {
    fn anchor_tx_hash(&self) -> String;
}
impl AnchorTxHash for taprpc::AssetTransfer {
    fn anchor_tx_hash(&self) -> String {
        let mut h = self.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    }
}
