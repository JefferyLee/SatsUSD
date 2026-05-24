//! G1 connectivity smoke test: connect to the devnet tapd over TLS + macaroon
//! and print GetInfo. Run with the devnet up: `cargo run -p satusd-tapd-client --bin g1_probe`.

use std::path::PathBuf;

use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;

    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut client = TaprootAssetsClient::new(channel);

    let info = client
        .get_info(taprpc::GetInfoRequest {})
        .await?
        .into_inner();
    println!("connected to tapd:");
    println!("  version : {}", info.version);
    println!("  network : {}", info.network);
    println!("  lnd id  : {}", info.lnd_identity_pubkey);
    Ok(())
}
