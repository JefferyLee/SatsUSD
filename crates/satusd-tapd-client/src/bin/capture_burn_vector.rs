//! Capture a real §5.D16 universe burn vector: send a grouped SatUSD asset to the
//! protocol burn sink (a NUMS-derived script key + NUMS internal key, both keyed
//! by the asset family) through live tapd, confirm the anchor, and export the
//! resulting `proof.File`. The exported proof is a genuine burn to the protocol
//! sink, so the state node's `check_burn_proof_binding` (D16) verifies it green.
//!
//! Run (devnet up + a grouped asset minted): the asset family is computed from the
//! group's tweaked group key + genesis id, exactly as the lineage verifier does.
//!
//!   cargo run -p satusd-tapd-client --bin capture_burn_vector
//!
//! Output: integration/lineage_vectors/burn_to_sink.hex

use std::path::PathBuf;

use satusd_tapd_client::proof_assembly::{assemble_external_proof_file, split_proof_file};
use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};

type Err = Box<dyn std::error::Error>;

const BURN_AMOUNT: u64 = 4_000;
const CHAIN_ID: u8 = 0; // regtest

fn compressed_even(xonly: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(33);
    v.push(0x02);
    v.extend_from_slice(xonly);
    v
}

#[tokio::main]
async fn main() -> Result<(), Err> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut tap = TaprootAssetsClient::new(channel);

    // ---- pick a grouped asset with enough balance ----
    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| {
            a.amount >= BURN_AMOUNT
                && a.asset_genesis.is_some()
                && a.asset_group
                    .as_ref()
                    .is_some_and(|g| !g.tweaked_group_key.is_empty())
        })
        .ok_or("no grouped asset with enough balance (mint a grouped asset first)")?;
    let genesis = asset.asset_genesis.as_ref().unwrap();
    let asset_id: [u8; 32] = genesis
        .asset_id
        .clone()
        .try_into()
        .map_err(|_| "asset_id len")?;
    let group_key: [u8; 33] = asset
        .asset_group
        .as_ref()
        .unwrap()
        .tweaked_group_key
        .clone()
        .try_into()
        .map_err(|_| "tweaked_group_key len")?;

    // ---- asset family + protocol burn sink (§5.D1 / §5.D16), exactly as the
    //      state node lineage verifier computes them ----
    let family = satusd_types::derive::asset_family_id(&asset_id, &group_key, CHAIN_ID);
    let sink = satusd_crypto::nums::protocol_sink_script_key(&family);
    let burn_internal = satusd_crypto::nums::protocol_burn_internal_key(&family);
    let burn_tweak = satusd_crypto::nums::protocol_burn_tweak(&family);
    println!("asset_id     = 0x{}", hex::encode(asset_id));
    println!("group_key    = 0x{}", hex::encode(group_key));
    println!("family_id    = 0x{}", hex::encode(family));
    println!("sink_key     = 0x{}", hex::encode(sink));
    println!("burn_internal= 0x{}", hex::encode(burn_internal));

    // ---- NewAddr to the protocol sink: external script key (NUMS internal +
    //      burn tweak) + NUMS anchor internal key, no tapscript sibling ----
    let req = taprpc::NewAddrRequest {
        asset_id: asset_id.to_vec(),
        amt: BURN_AMOUNT,
        script_key: Some(taprpc::ScriptKey {
            pub_key: sink.to_vec(),
            key_desc: Some(taprpc::KeyDescriptor {
                raw_key_bytes: compressed_even(&burn_internal),
                ..Default::default()
            }),
            tap_tweak: burn_tweak.to_vec(),
            r#type: taprpc::ScriptKeyType::ScriptKeyScriptPathExternal as i32,
        }),
        internal_key: Some(taprpc::KeyDescriptor {
            raw_key_bytes: compressed_even(&burn_internal),
            ..Default::default()
        }),
        skip_proof_courier_conn_check: true,
        ..Default::default()
    };
    let addr = tap.new_addr(req).await?.into_inner();
    let tail32 = |v: &[u8]| v[v.len().saturating_sub(32)..].to_vec();
    assert_eq!(tail32(&addr.script_key), sink.to_vec(), "sink echo");
    assert_eq!(
        tail32(&addr.internal_key),
        burn_internal.to_vec(),
        "internal echo"
    );
    println!("\ntapd ACCEPTED the protocol-sink address; sending {BURN_AMOUNT} units...");

    // ---- SendAsset to the sink + find the burn output's stored script key ----
    let resp = tap
        .send_asset(taprpc::SendAssetRequest {
            tap_addrs: vec![addr.encoded.clone()],
            skip_proof_courier_ping_check: true,
            ..Default::default()
        })
        .await?
        .into_inner();
    let transfer = resp.transfer.ok_or("no transfer in SendAsset response")?;
    let sink_out = transfer
        .outputs
        .iter()
        .find(|o| o.script_key.ends_with(&sink))
        .ok_or("burn output not found in transfer")?;
    let oa = sink_out
        .anchor
        .as_ref()
        .ok_or("burn output has no anchor")?;
    println!(
        "burn anchor outpoint = {}  value = {}",
        oa.outpoint, oa.value
    );
    let sink_transition = sink_out.new_proof_blob.clone();
    if sink_transition.is_empty() {
        return Err("no transition proof for the sink output".into());
    }
    // The change output is local (tapd owns it) and shares the identical input
    // lineage; its proof.File minus its final transition gives that lineage.
    let change_out = transfer
        .outputs
        .iter()
        .find(|o| o.script_key_is_local && !o.script_key.ends_with(&sink))
        .ok_or("no local change output to source the input lineage from")?;
    let change_script_key = change_out.script_key.clone();

    // ---- mine to confirm the anchor, then assemble the sink's proof.File ----
    let btc = bitcoincore_rpc::Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        bitcoincore_rpc::Auth::UserPass("satusd".into(), "satusd".into()),
    )?;
    use bitcoincore_rpc::RpcApi;
    let miner = btc.get_new_address(None, None)?.assume_checked();
    btc.generate_to_address(1, &miner)?;

    // tapd archives the proof a moment after the anchor confirms; retry briefly.
    let mut change_file = Vec::new();
    for attempt in 0..15 {
        match tap
            .export_proof(taprpc::ExportProofRequest {
                asset_id: asset_id.to_vec(),
                script_key: change_script_key.clone(),
                ..Default::default()
            })
            .await
        {
            Ok(r) => {
                change_file = r.into_inner().raw_proof_file;
                break;
            }
            Err(e) if attempt == 14 => return Err(e.into()),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(800)).await,
        }
    }
    let file = assemble_external_proof_file(&change_file, &sink_transition);
    let n_proofs = split_proof_file(&file).len();

    let out_path = root.join("integration/lineage_vectors/burn_to_sink.hex");
    std::fs::write(&out_path, hex::encode(&file))?;
    println!(
        "\nassembled {}-proof file ({} bytes) → {}",
        n_proofs,
        file.len(),
        out_path.display()
    );
    Ok(())
}
