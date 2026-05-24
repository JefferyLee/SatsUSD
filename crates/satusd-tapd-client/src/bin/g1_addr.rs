//! G1 asset-layer probe: ask tapd for a receive address that carries our
//! finalize/refund branch as a `tapscript_sibling`, validating the sibling
//! preimage encoding against the live daemon. (script_key/internal_key left to
//! tapd here; custom NUMS internal + foreign script_key is the next step.)
//!
//! Run with devnet up + a minted asset: cargo run -p satusd-tapd-client --bin g1_addr

use std::path::PathBuf;

use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut client = TaprootAssetsClient::new(channel);

    // Pick a minted asset.
    let assets = client
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset_id = assets
        .iter()
        .find_map(|a| a.asset_genesis.as_ref().map(|g| g.asset_id.clone()))
        .ok_or("no minted asset found (mint one first)")?;
    println!("using asset_id = {}", hex::encode(&asset_id));

    // Build our finalize/refund branch and its tapd tapscript_sibling preimage.
    let op = satusd_crypto::nums::derive_nums_key("g1-addr-op", &[]);
    let user = satusd_crypto::nums::derive_nums_key("g1-addr-user", &[]);
    let anchor = satusd_lock::build_lock_anchor_from_bytes(&[0x11; 32], &op, &user, 6, 10)?;
    let sibling = anchor.tapscript_sibling_preimage();
    println!("sibling preimage = 0x{}", hex::encode(&sibling));

    let req = taprpc::NewAddrRequest {
        asset_id,
        amt: 1,
        tapscript_sibling: sibling.clone(),
        ..Default::default()
    };
    let addr = client.new_addr(req).await?.into_inner();

    println!("\ntapd accepted the sibling and returned an address:");
    println!("  encoded            = {}", addr.encoded);
    println!(
        "  taproot_output_key = 0x{}",
        hex::encode(&addr.taproot_output_key)
    );
    println!(
        "  echoed sibling     = 0x{}",
        hex::encode(&addr.tapscript_sibling)
    );
    println!(
        "  sibling round-trips: {}",
        addr.tapscript_sibling == sibling
    );
    Ok(())
}
