//! Capture a real §5.D3 / §5.D15 lock-anchor vector: send a grouped SatUSD asset
//! to a SatUSD lock anchor (asset script key = `derive_lock_script_key(intent)`,
//! NUMS anchor internal key, finalize/refund tapscript sibling) through live tapd,
//! confirm the anchor, and assemble the resulting `proof.File`. The exported proof
//! commits the asset to the intent-derived lock script key, so the state node's
//! `check_lineage_lock_binding` (§5.D15) verifies it green end-to-end.
//!
//! The canonical RedeemIntent below is mirrored verbatim by the
//! `apply_redeem_lock` test in `satusd-state::redeem`; any drift makes that test
//! fail loudly (the captured head script key won't match `derive_lock_script_key`).
//!
//!   cargo run -p satusd-tapd-client --bin capture_lock_vector
//!
//! Output: integration/lineage_vectors/lock_anchor.hex

use std::path::PathBuf;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{Secp256k1, SecretKey, XOnlyPublicKey};
use satusd_tapd_client::proof_assembly::{assemble_external_proof_file, split_proof_file};
use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};
use satusd_types::types::RedeemIntent;

type Err = Box<dyn std::error::Error>;

const LOCK_AMOUNT: u64 = 4_000;
const CHAIN_ID: u8 = 0;
const FINALIZE_CSV: i64 = 144;
const REFUND_CSV: i64 = 288;
/// Preimage whose SHA-256 is the canonical intent's payment hash.
const PREIMAGE: [u8; 32] = [0x55; 32];

fn compressed_even(xonly: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(33);
    v.push(0x02);
    v.extend_from_slice(xonly);
    v
}

/// The canonical lock-vector RedeemIntent (must match the redeem.rs test exactly).
/// `family` is the asset family of the grouped devnet asset.
fn canonical_intent(family: [u8; 32]) -> RedeemIntent {
    RedeemIntent {
        version: 1,
        network: CHAIN_ID,
        redemption_id: [0x77; 32],
        satusd_asset_family_id: family,
        amount_satusd_atoms: LOCK_AMOUNT,
        user_btc_refund_pubkey: [0x31; 32],
        user_btc_claim_pubkey: [0x32; 32],
        user_asset_refund_key: satusd_crypto::nums::derive_nums_key("satusd-lock-vector-user", &[]),
        operator_id: Some([0x20; 32]),
        mode: 0, // MODE_FAST_OPERATOR
        payment_hash: satusd_types::sha256(&[&PREIMAGE]),
        asset_lock_csv_delta: 288,
        btc_htlc_csv_delta: 144,
        max_operator_fee_bps: 50,
        l1_anchor_height: 840_000,
        l1_anchor_hash: [0x33; 32],
        expiry_height: 900_000,
        nonce: [0x34; 32],
    }
}

#[tokio::main]
async fn main() -> Result<(), Err> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut tap = TaprootAssetsClient::new(channel);

    // ---- grouped asset + its family ----
    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| {
            a.amount >= LOCK_AMOUNT
                && a.asset_genesis.is_some()
                && a.asset_group
                    .as_ref()
                    .is_some_and(|g| !g.tweaked_group_key.is_empty())
        })
        .ok_or("no grouped asset with enough balance")?;
    let asset_id: [u8; 32] = asset
        .asset_genesis
        .as_ref()
        .unwrap()
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
    let family = satusd_types::derive::asset_family_id(&asset_id, &group_key, CHAIN_ID);

    // ---- intent-derived lock script key + NUMS internal + finalize/refund sibling ----
    let intent = canonical_intent(family);
    let rih = satusd_types::derive::redeem_intent_hash(&intent);
    let payment_hash = intent.payment_hash;
    let lock_script_key =
        satusd_lock::derive_lock_script_key(&intent.user_asset_refund_key, &rih, &payment_hash);
    let lock_tweak = satusd_types::derive::lock_tweak(&rih, &payment_hash);
    let user_xonly = XOnlyPublicKey::from_slice(&intent.user_asset_refund_key)?;
    let secp = Secp256k1::new();
    let op_sk =
        SecretKey::from_slice(&sha256::Hash::hash(b"satusd-lock-vector-op").to_byte_array())?;
    let op_xonly = op_sk.x_only_public_key(&secp).0;
    let bare = satusd_lock::build_lock_anchor(
        &payment_hash,
        op_xonly,
        user_xonly,
        FINALIZE_CSV,
        REFUND_CSV,
    );
    let sibling = bare.tapscript_sibling_preimage();
    let nums = satusd_lock::lock_anchor_internal_key().serialize();

    println!("family_id       = 0x{}", hex::encode(family));
    println!("rih             = 0x{}", hex::encode(rih));
    println!("lock_script_key = 0x{}", hex::encode(lock_script_key));

    // ---- NewAddr to the lock anchor (external script key + NUMS internal + sibling) ----
    let addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.to_vec(),
            amt: LOCK_AMOUNT,
            script_key: Some(taprpc::ScriptKey {
                pub_key: lock_script_key.to_vec(),
                key_desc: Some(taprpc::KeyDescriptor {
                    raw_key_bytes: compressed_even(&intent.user_asset_refund_key),
                    ..Default::default()
                }),
                tap_tweak: lock_tweak.to_vec(),
                r#type: taprpc::ScriptKeyType::ScriptKeyScriptPathExternal as i32,
            }),
            internal_key: Some(taprpc::KeyDescriptor {
                raw_key_bytes: compressed_even(&nums),
                ..Default::default()
            }),
            tapscript_sibling: sibling,
            skip_proof_courier_conn_check: true,
            ..Default::default()
        })
        .await?
        .into_inner();
    let tail32 = |v: &[u8]| v[v.len().saturating_sub(32)..].to_vec();
    assert_eq!(
        tail32(&addr.script_key),
        lock_script_key.to_vec(),
        "script_key echo"
    );

    // ---- SendAsset to the lock anchor ----
    let transfer = tap
        .send_asset(taprpc::SendAssetRequest {
            tap_addrs: vec![addr.encoded.clone()],
            skip_proof_courier_ping_check: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .transfer
        .ok_or("no transfer in SendAsset response")?;
    let lock_out = transfer
        .outputs
        .iter()
        .find(|o| o.script_key.ends_with(&lock_script_key))
        .ok_or("lock output not found")?;
    let oa = lock_out
        .anchor
        .as_ref()
        .ok_or("lock output has no anchor")?;
    println!("lock anchor outpoint = {}", oa.outpoint);
    let lock_transition = lock_out.new_proof_blob.clone();
    let change_script_key = transfer
        .outputs
        .iter()
        .find(|o| o.script_key_is_local && !o.script_key.ends_with(&lock_script_key))
        .ok_or("no local change output")?
        .script_key
        .clone();

    // ---- confirm + assemble ----
    let btc = bitcoincore_rpc::Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        bitcoincore_rpc::Auth::UserPass("satusd".into(), "satusd".into()),
    )?;
    use bitcoincore_rpc::RpcApi;
    let miner = btc.get_new_address(None, None)?.assume_checked();
    btc.generate_to_address(1, &miner)?;

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
    let file = assemble_external_proof_file(&change_file, &lock_transition);
    let out_path = root.join("integration/lineage_vectors/lock_anchor.hex");
    std::fs::write(&out_path, hex::encode(&file))?;
    println!(
        "\nassembled {}-proof lock vector ({} bytes) → {}",
        split_proof_file(&file).len(),
        file.len(),
        out_path.display()
    );
    Ok(())
}
