//! Devnet equivalence test: our Rust `derive_burn_key` must produce
//! byte-for-byte the script key tapd assigns to a real `BurnAsset`
//! output. This is the machine check that licenses spec 01 §4's
//! "tapd-native burn semantics" claim.
//!
//! Requires a live devnet (`make devnet-up`) with a grouped SatUSD
//! asset minted. Run with:
//!
//! ```text
//! cargo test -p satusd-rail0 --test devnet_burn_key -- --ignored
//! ```

use std::path::PathBuf;

use satusd_rail0::burn_key::{derive_burn_key, parse_anchor_point, PrevId};
use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};

const BURN_AMOUNT: u64 = 1_000;

fn xonly(key: &[u8]) -> [u8; 32] {
    match key.len() {
        32 => key.try_into().unwrap(),
        33 => key[1..].try_into().unwrap(),
        n => panic!("unexpected script key length {n}"),
    }
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) with a grouped asset"]
async fn burn_key_matches_tapd() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut tap = TaprootAssetsClient::new(channel);

    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| a.amount >= BURN_AMOUNT && a.asset_genesis.is_some())
        .ok_or("no asset with enough balance (mint one first)")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();

    let resp = tap
        .burn_asset(taprpc::BurnAssetRequest {
            asset: Some(taprpc::burn_asset_request::Asset::AssetId(asset_id.clone())),
            amount_to_burn: BURN_AMOUNT,
            confirmation_text: "assets will be destroyed".to_string(),
            note: "rail0 burn-key equivalence test".to_string(),
        })
        .await?
        .into_inner();

    let transfer = resp.burn_transfer.ok_or("no burn_transfer in response")?;
    let input = transfer.inputs.first().ok_or("transfer has no inputs")?;
    let (txid_wire, vout) =
        parse_anchor_point(&input.anchor_point).ok_or("unparseable anchor_point")?;

    let prev = PrevId {
        txid_wire,
        vout,
        asset_id: input
            .asset_id
            .clone()
            .try_into()
            .map_err(|_| "asset_id len")?,
        script_key_xonly: xonly(&input.script_key),
    };
    let ours = derive_burn_key(&prev);

    // The burn output is the one whose script key is NOT a split
    // change key; identify it by matching against our derivation
    // across all outputs and assert exactly one match.
    let matches: Vec<_> = transfer
        .outputs
        .iter()
        .filter(|o| xonly(&o.script_key) == ours)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "exactly one output must carry the derived burn key; outputs: {:?}",
        transfer
            .outputs
            .iter()
            .map(|o| hex::encode(&o.script_key))
            .collect::<Vec<_>>()
    );
    assert_eq!(matches[0].amount, BURN_AMOUNT, "burn output amount");

    println!(
        "tapd burn key == derive_burn_key: {} (amount {})",
        hex::encode(ours),
        BURN_AMOUNT
    );
    Ok(())
}
