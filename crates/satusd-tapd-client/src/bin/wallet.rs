//! SatUSD reference wallet CLI (MVP, PRD §12.5 / §14.4 M1 demo) — wraps live tapd
//! for the user-facing flows: list balances, receive (make an address), send, and
//! **independently verify a received asset's lineage** with `satusd-ta-proof`
//! (DL-23) instead of trusting tapd's own proof check.
//!
//! Connects to the devnet tapd (same paths as the other bins). Usage:
//!
//!   wallet balance
//!   wallet receive --asset-id <hex> --amt <n>
//!   wallet send --addr <taprt1...>
//!   wallet verify-lineage --asset-id <hex> --script-key <hex>

use std::collections::HashMap;
use std::path::PathBuf;

use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};

type Err = Box<dyn std::error::Error>;

/// Minimal `--flag value` parser into a map.
fn flags(args: &[String]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        if let Some(k) = args[i].strip_prefix("--") {
            m.insert(k.to_string(), args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }
    m
}

async fn client() -> Result<TaprootAssetsClient<satusd_tapd_client::TapChannel>, Err> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    Ok(TaprootAssetsClient::new(channel))
}

#[tokio::main]
async fn main() -> Result<(), Err> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let f = flags(&args);
    let mut tap = client().await?;

    match cmd {
        "balance" => {
            let assets = tap
                .list_assets(taprpc::ListAssetRequest::default())
                .await?
                .into_inner()
                .assets;
            if assets.is_empty() {
                println!("(no assets)");
            }
            for a in assets.iter().filter(|a| a.amount > 0) {
                let g = a.asset_genesis.as_ref();
                let name = g.map(|g| g.name.clone()).unwrap_or_default();
                let id = g.map(|g| hex::encode(&g.asset_id)).unwrap_or_default();
                println!(
                    "{:>14}  {}  {}  sk={}",
                    a.amount,
                    name,
                    id,
                    hex::encode(&a.script_key)
                );
            }
        }

        "receive" => {
            let asset_id = hex::decode(f.get("asset-id").ok_or("--asset-id required")?)?;
            let amt: u64 = f.get("amt").ok_or("--amt required")?.parse()?;
            let addr = tap
                .new_addr(taprpc::NewAddrRequest {
                    asset_id,
                    amt,
                    skip_proof_courier_conn_check: true,
                    ..Default::default()
                })
                .await?
                .into_inner();
            println!("{}", addr.encoded);
        }

        "send" => {
            let addr = f.get("addr").ok_or("--addr required")?.clone();
            let resp = tap
                .send_asset(taprpc::SendAssetRequest {
                    tap_addrs: vec![addr],
                    skip_proof_courier_ping_check: true,
                    ..Default::default()
                })
                .await?
                .into_inner();
            let txid = resp
                .transfer
                .and_then(|t| {
                    t.outputs
                        .first()
                        .and_then(|o| o.anchor.as_ref().map(|a| a.outpoint.clone()))
                })
                .unwrap_or_default();
            println!("sent; anchor outpoint = {txid}");
        }

        "verify-lineage" => {
            let asset_id = hex::decode(f.get("asset-id").ok_or("--asset-id required")?)?;
            let script_key = hex::decode(f.get("script-key").ok_or("--script-key required")?)?;
            let proof = tap
                .export_proof(taprpc::ExportProofRequest {
                    asset_id,
                    script_key,
                    ..Default::default()
                })
                .await?
                .into_inner()
                .raw_proof_file;
            // Independent DL-23 verification — do not trust tapd's own check.
            let file = satusd_ta_proof::parse_proof_file(&proof)
                .map_err(|e| format!("malformed proof.File: {e:?}"))?;
            let proofs = file.parsed().map_err(|e| format!("bad proof TLV: {e:?}"))?;
            for (i, p) in proofs.iter().enumerate() {
                let leaf = p
                    .asset_leaf()
                    .map_err(|e| format!("step {i} leaf: {e:?}"))?;
                println!("  step {i}: amount={}", leaf.amount);
            }
            match satusd_ta_proof::verify_lineage(&proofs) {
                Ok(head) => println!(
                    "✓ lineage VALID — {} steps, head output_key={}",
                    proofs.len(),
                    hex::encode(head.taproot_output_key)
                ),
                Err(e) => {
                    println!("✗ lineage INVALID: {e:?}");
                    std::process::exit(1);
                }
            }
        }

        _ => {
            eprintln!(
                "usage: wallet <balance | receive --asset-id <hex> --amt <n> | \
                 send --addr <taprt1...> | verify-lineage --asset-id <hex> --script-key <hex>>"
            );
            std::process::exit(2);
        }
    }
    Ok(())
}
