//! J4 funding-output landing E2E: real SatUSD lands inside OUR
//! DLC-shaped output — internal key = our funding key, script tree =
//! {tapd's TA commitment leaf, our refund leaf} — and the on-chain
//! output key is byte-identical to our own `funding_output()`
//! reconstruction. This live-validates the proposal-0001 §3.1
//! construction and the sibling-preimage encoding.
//!
//! Requires `make devnet-up` + a grouped asset minted. Run with:
//!
//! ```text
//! cargo test -p satusd-rail1 --test devnet_funding -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::process::Command;

use satusd_rail::encode::tagged_hash;
use satusd_rail1::funding::{
    funding_output, keyspend_secret, refund_leaf_script, sibling_preimage,
};
use satusd_tapd_client::{connect, taprpc, AssetWalletClient, TaprootAssetsClient};
use secp256k1::{Secp256k1, SecretKey};

const FUND_UNITS: u64 = 2_000;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bcli(args: &[&str]) -> serde_json::Value {
    let data = root().join("devnet/data/bitcoind");
    let mut cmd = Command::new("bitcoin-cli");
    cmd.args([
        "-regtest",
        &format!("-datadir={}", data.display()),
        "-rpcuser=satusd",
        "-rpcpassword=satusd",
        "-rpcport=18443",
        "-rpcwallet=regtest",
    ]);
    cmd.args(args);
    let out = cmd.output().expect("bitcoin-cli runs");
    assert!(
        out.status.success(),
        "bitcoin-cli {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(s.trim())
        .unwrap_or_else(|_| serde_json::Value::String(s.trim().to_string()))
}

fn compressed_even(x: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(33);
    v.push(0x02);
    v.extend_from_slice(x);
    v
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) with a grouped asset"]
async fn funding_output_lands_and_q_reconstructs() -> Result<(), Box<dyn std::error::Error>> {
    let r = root();
    let tls = std::fs::read(r.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(r.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);

    // ---- pick the asset ----
    let assets = tap
        .list_assets(taprpc::ListAssetRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| a.amount >= FUND_UNITS && a.asset_genesis.is_some())
        .ok_or("no asset with enough balance")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();

    // ---- our funding key + refund leaf ----
    let secp = Secp256k1::new();
    let funding_sk = tagged_hash("devnet/funding-internal", b"j4");
    let (funding_px, _) = SecretKey::from_byte_array(funding_sk)
        .unwrap()
        .x_only_public_key(&secp);
    let internal_x = funding_px.serialize();
    let refund = refund_leaf_script(
        144,
        &tagged_hash("devnet/user-x", b"u"),
        &tagged_hash("devnet/lp-x", b"l"),
    );
    let sibling = sibling_preimage(&refund);

    // ---- asset script key from tapd (it must be able to sign the
    //      asset-level CET transfers later) ----
    let script_key = wallet
        .next_script_key(satusd_tapd_client::assetwalletrpc::NextScriptKeyRequest {
            key_family: 212,
        })
        .await?
        .into_inner()
        .script_key
        .ok_or("no script key")?;

    // ---- the DLC-shaped address ----
    let addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.clone(),
            amt: FUND_UNITS,
            script_key: Some(script_key),
            internal_key: Some(taprpc::KeyDescriptor {
                raw_key_bytes: compressed_even(&internal_x),
                key_loc: None,
            }),
            tapscript_sibling: sibling,
            ..Default::default()
        })
        .await?
        .into_inner();
    println!(
        "DLC funding addr: {}…",
        &addr.encoded[..40.min(addr.encoded.len())]
    );

    // ---- send the asset into our construction ----
    let send = tap
        .send_asset(taprpc::SendAssetRequest {
            tap_addrs: vec![addr.encoded.clone()],
            ..Default::default()
        })
        .await?
        .into_inner();
    let transfer = send.transfer.ok_or("no transfer")?;
    bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // ---- find the funding output among the transfer outputs:
    //      the one anchored to OUR internal key ----
    let out = transfer
        .outputs
        .iter()
        .find(|o| {
            o.anchor
                .as_ref()
                .is_some_and(|a| a.internal_key == compressed_even(&internal_x))
        })
        .ok_or("no output anchored to our internal key")?;
    let anchor = out.anchor.as_ref().unwrap();
    let (txid_str, vout_str) = anchor.outpoint.split_once(':').ok_or("bad outpoint")?;
    println!("funding outpoint: {}:{}", txid_str, vout_str);

    // ---- THE assertions ----
    // tapd reports the TA commitment root and the full merkle root;
    // our funding_output() must reproduce the merkle root AND the
    // on-chain output key from (internal, TA leaf, refund script).
    let ta_leaf: [u8; 32] = anchor
        .taproot_asset_root
        .clone()
        .try_into()
        .map_err(|_| "taproot_asset_root len")?;
    let ours = funding_output(&internal_x, &ta_leaf, &refund);

    let reported_root: [u8; 32] = anchor
        .merkle_root
        .clone()
        .try_into()
        .map_err(|_| "merkle_root len")?;
    assert_eq!(
        ours.merkle_root, reported_root,
        "branch(TA leaf, refund leaf) must equal tapd's merkle root"
    );

    let txout = bcli(&["gettxout", txid_str, vout_str]);
    let spk = txout["scriptPubKey"]["hex"]
        .as_str()
        .ok_or("outpoint not found")?;
    assert_eq!(
        spk,
        format!("5120{}", hex::encode(ours.output_x)),
        "on-chain P2TR output key must equal our reconstructed Q"
    );

    // And the keyspend secret controls Q (the CET spending key).
    let tweaked = keyspend_secret(&funding_sk, &ours.merkle_root)?;
    let (qx, _) = SecretKey::from_byte_array(tweaked)
        .unwrap()
        .x_only_public_key(&secp);
    assert_eq!(qx.serialize(), ours.output_x);

    println!(
        "J4 funding output validated: {} units of TA inside our DLC output, Q = {}",
        FUND_UNITS,
        hex::encode(ours.output_x)
    );
    Ok(())
}
