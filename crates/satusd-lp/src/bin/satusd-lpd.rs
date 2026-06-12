//! The founder-run Rail-0 LP daemon (PRD §8 scaffolding row).
//!
//! ```text
//! satusd-lpd <lp-seed-hex> <asset-id-hex> <listen-addr> [oracle-addr]
//! ```
//!
//! Network selection via SATUSD_NET (devnet default | signet); run
//! from the repository root. Quotes price off the public oracle's
//! latest attestation.

use std::path::PathBuf;

use satusd_lp::{serve, Lp, LpConfig};
use satusd_rail::encode::tagged_hash;
use satusd_rail::manifest::{FeeSchedule, LpStake};
use satusd_rail0::rail::{rail0_manifest, Rail0Params};
use satusd_tapd_client::env::NodeEnv;

const SEED_TO_KEY_TAG: &str = "SatUSD/lp/key/v1";

fn hex32(s: &str) -> Result<[u8; 32], String> {
    let v = hex::decode(s).map_err(|e| e.to_string())?;
    v.try_into().map_err(|_| "expected 32 bytes".to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let seed = hex32(
        args.get(1)
            .ok_or("usage: satusd-lpd <seed> <asset-id> <listen> [oracle]")?,
    )?;
    let asset_id = hex32(args.get(2).ok_or("missing asset-id")?)?;
    let listen = args.get(3).ok_or("missing listen addr")?.clone();
    let oracle_addr = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| "207.148.98.132:9590".into());

    let lp_sk = tagged_hash(SEED_TO_KEY_TAG, &seed);
    let secp = secp256k1::Secp256k1::new();
    let (px, _) = secp256k1::SecretKey::from_byte_array(lp_sk)?.x_only_public_key(&secp);

    let manifest = rail0_manifest(Rail0Params {
        asset_id,
        max_size_sats: 5_000_000,
        fee_schedule: FeeSchedule {
            retain_bps: 10,
            service_bps: 5,
            fixed_sats: 200,
        },
        price_dev_bound_bps: 100,
        quote_validity_s: 600,
        lp_stake: LpStake {
            amount_sats: 0, // signet scaffold: stake mechanics land with spec 05 drill
            slash_conditions: vec![],
        },
    })
    .map_err(|e| format!("manifest: {e:?}"))?;

    let env = NodeEnv::from_env(
        std::env::var("SATUSD_ROOT")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?),
    );
    let lp = Lp::new(LpConfig {
        env,
        lp_sk,
        lp_pubkey: px.serialize(),
        asset_id,
        manifest,
        oracle_addr,
        fee_budget_sats: 2_500,
    })
    .await?;

    println!(
        "satusd-lpd: rail_id={}",
        hex::encode(lp.cfg.manifest.rail_id())
    );
    println!("satusd-lpd: lp_pubkey={}", hex::encode(lp.cfg.lp_pubkey));
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    println!(
        "satusd-lpd: listening on {listen} (chain={})",
        lp.cfg.env.chain
    );
    serve(lp, listener).await;
    Ok(())
}
