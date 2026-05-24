//! G1 asset-layer finalize: lock a *real* Taproot Asset into the §5.D3 structure
//! through live tapd, then spend the anchor on-chain via the finalize script path.
//!
//! Flow (devnet up + a minted asset required):
//! 1. NewAddr with a FOREIGN script_key (= lock_script_key) + NUMS internal_key
//!    plus our finalize/refund branch as the tapscript_sibling. Open risk: does
//!    tapd accept a foreign script_key + foreign NUMS internal_key?
//! 2. SendAsset the full balance to that address; tapd builds + broadcasts the
//!    anchor tx. We read back taproot_asset_root / merkle_root / internal_key
//!    from the transfer output (sender-side bookkeeping, so a proof-courier
//!    round-trip is not required for our purposes).
//! 3. Rebuild the real anchor tree TapBranch(taproot_asset_root,
//!    branch(finalize, refund)) and verify it matches tapd's output exactly.
//! 4. Spend the anchor via the FINALIZE path on-chain (control block now carries
//!    the extra taproot_asset_root merkle step).
//!
//! Run: cargo run -p satusd-tapd-client --bin g1_lock_asset

use std::path::PathBuf;
use std::str::FromStr;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::key::Keypair;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey, XOnlyPublicKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::{
    absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, TapLeafHash,
    Transaction, TxIn, TxOut, Txid, Witness,
};
use bitcoincore_rpc::{Auth, Client, RpcApi};

use satusd_tapd_client::{connect, taprpc, TaprootAssetsClient};

type Err = Box<dyn std::error::Error>;

const FINALIZE_CSV: u16 = 6;
const REFUND_CSV: u16 = 10;
const REDEEM_INTENT_HASH: [u8; 32] = [0x42; 32];

/// Deterministic secp keypair from a label (so reruns are reproducible).
fn det_keypair(secp: &Secp256k1<bitcoin::secp256k1::All>, label: &str) -> Keypair {
    let mut seed = sha256::Hash::hash(label.as_bytes()).to_byte_array();
    loop {
        if let Ok(sk) = SecretKey::from_slice(&seed) {
            return Keypair::from_secret_key(secp, &sk);
        }
        seed = sha256::Hash::hash(&seed).to_byte_array();
    }
}

/// Prefix an x-only key with the even-Y parity byte to get a 33-byte compressed key.
fn compressed_even(xonly: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(33);
    v.push(0x02);
    v.extend_from_slice(xonly);
    v
}

#[tokio::main]
async fn main() -> Result<(), Err> {
    let secp = Secp256k1::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tls = std::fs::read(root.join("devnet/data/tapd/tls.cert"))?;
    let mac = std::fs::read(root.join("devnet/data/tapd/data/regtest/admin.macaroon"))?;
    let channel = connect("127.0.0.1:10029", &tls, &hex::encode(&mac), "localhost").await?;
    let mut tap = TaprootAssetsClient::new(channel);

    let btc = Client::new(
        "http://127.0.0.1:18443/wallet/regtest",
        Auth::UserPass("satusd".into(), "satusd".into()),
    )?;
    let miner = btc.get_new_address(None, None)?.assume_checked();

    // ---- keys: operator + user (we hold both, to sign finalize/refund) ----
    let operator = det_keypair(&secp, "g1-asset-operator");
    let user = det_keypair(&secp, "g1-asset-user");
    let op_xonly: XOnlyPublicKey = operator.x_only_public_key().0;
    let user_xonly: XOnlyPublicKey = user.x_only_public_key().0;
    let user_xonly_bytes = user_xonly.serialize();

    let preimage = sha256::Hash::hash(b"g1-asset-preimage").to_byte_array();
    let payment_hash = sha256::Hash::hash(&preimage).to_byte_array();

    // Bare anchor: only used to obtain the finalize/refund branch + sibling preimage.
    let bare = satusd_lock::build_lock_anchor(
        &payment_hash,
        op_xonly,
        user_xonly,
        FINALIZE_CSV as i64,
        REFUND_CSV as i64,
    );
    let sibling = bare.tapscript_sibling_preimage();

    // Asset-layer lock script key (foreign to tapd) + NUMS anchor internal key.
    // The lock script key is TapTweak(user_asset_refund_key, lock_tweak); tapd
    // needs the internal key (user) + the tweak to register an external key.
    let lock_tweak = satusd_types::derive::lock_tweak(&REDEEM_INTENT_HASH, &payment_hash);
    let lock_script_key =
        satusd_lock::derive_lock_script_key(&user_xonly_bytes, &REDEEM_INTENT_HASH, &payment_hash);
    let nums = satusd_lock::lock_anchor_internal_key().serialize();

    // ---- pick a minted asset (need its id + full balance) ----
    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| a.amount > 0 && a.asset_genesis.is_some())
        .ok_or("no spendable minted asset (mint one first)")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();
    let amount = asset.amount;
    println!("asset_id = {} amount = {amount}", hex::encode(&asset_id));

    // ---- 1) NewAddr with foreign script_key + NUMS internal_key + sibling ----
    let req = taprpc::NewAddrRequest {
        asset_id: asset_id.clone(),
        amt: amount,
        script_key: Some(taprpc::ScriptKey {
            pub_key: lock_script_key.to_vec(),
            key_desc: Some(taprpc::KeyDescriptor {
                raw_key_bytes: compressed_even(&user_xonly_bytes),
                ..Default::default()
            }),
            tap_tweak: lock_tweak.to_vec(),
            r#type: taprpc::ScriptKeyType::ScriptKeyScriptPathExternal as i32,
        }),
        internal_key: Some(taprpc::KeyDescriptor {
            raw_key_bytes: compressed_even(&nums),
            ..Default::default()
        }),
        tapscript_sibling: sibling.clone(),
        skip_proof_courier_conn_check: true,
        ..Default::default()
    };
    let addr = tap.new_addr(req).await?.into_inner();
    println!("\n[1] tapd ACCEPTED foreign script_key + NUMS internal_key:");
    println!("    encoded            = {}", addr.encoded);
    println!(
        "    script_key  echo   = 0x{}",
        hex::encode(&addr.script_key)
    );
    println!(
        "    internal_key echo  = 0x{}",
        hex::encode(&addr.internal_key)
    );
    println!(
        "    taproot_output_key = 0x{}",
        hex::encode(&addr.taproot_output_key)
    );
    let tail32 = |v: &[u8]| v[v.len().saturating_sub(32)..].to_vec();
    assert_eq!(
        tail32(&addr.script_key),
        lock_script_key.to_vec(),
        "script_key echo"
    );
    assert_eq!(
        tail32(&addr.internal_key),
        nums.to_vec(),
        "internal_key echo"
    );
    assert_eq!(addr.tapscript_sibling, sibling, "sibling echo");

    // ---- 2) SendAsset the full balance to our lock address ----
    let resp = tap
        .send_asset(taprpc::SendAssetRequest {
            tap_addrs: vec![addr.encoded.clone()],
            skip_proof_courier_ping_check: true,
            ..Default::default()
        })
        .await?
        .into_inner();
    let transfer = resp.transfer.ok_or("no transfer in SendAsset response")?;

    // Find the output that landed on our lock script key.
    let out = transfer
        .outputs
        .iter()
        .find(|o| o.script_key.ends_with(&lock_script_key))
        .ok_or("lock output not found in transfer")?;
    let oa = out.anchor.as_ref().ok_or("output has no anchor info")?;
    let ta_root: [u8; 32] = oa
        .taproot_asset_root
        .clone()
        .try_into()
        .map_err(|_| "ta_root len")?;
    println!("\n[2] anchor created by tapd:");
    println!("    outpoint           = {}", oa.outpoint);
    println!("    value (sat)        = {}", oa.value);
    println!(
        "    internal_key       = 0x{}",
        hex::encode(&oa.internal_key)
    );
    println!("    taproot_asset_root = 0x{}", hex::encode(ta_root));
    println!(
        "    merkle_root        = 0x{}",
        hex::encode(&oa.merkle_root)
    );
    assert_eq!(
        oa.internal_key,
        compressed_even(&nums),
        "anchor internal key is our NUMS"
    );

    // Confirm the anchor on-chain.
    btc.generate_to_address(1, &miner)?;

    // ---- 3) Rebuild the real (deeper) anchor tree and verify it matches tapd ----
    let anchor = satusd_lock::build_asset_lock_anchor(
        &payment_hash,
        op_xonly,
        user_xonly,
        FINALIZE_CSV as i64,
        REFUND_CSV as i64,
        &ta_root,
    );
    println!("\n[3] our reconstruction (TapBranch(ta_root, branch(finalize,refund))):");
    println!(
        "    rebuilt output_key = 0x{}",
        hex::encode(anchor.output_key_bytes())
    );
    let merkle_root: [u8; 32] = oa
        .merkle_root
        .clone()
        .try_into()
        .map_err(|_| "merkle len")?;
    assert_eq!(
        anchor.spend_info.merkle_root().unwrap().to_byte_array(),
        merkle_root,
        "rebuilt merkle root must equal tapd's merkle_root"
    );
    let pk_script = ScriptBuf::from_bytes(oa.pk_script.clone());
    assert_eq!(
        anchor.script_pubkey(),
        pk_script,
        "rebuilt P2TR must equal tapd's anchor output"
    );
    println!("    ✓ merkle_root and P2TR scriptPubKey match tapd exactly");

    // ---- 4) Spend the anchor via the FINALIZE path on-chain ----
    let (txid_str, vout_str) = oa.outpoint.split_once(':').ok_or("bad outpoint")?;
    let outpoint = OutPoint {
        txid: Txid::from_str(txid_str)?,
        vout: vout_str.parse()?,
    };
    let value = Amount::from_sat(oa.value as u64);
    btc.generate_to_address(FINALIZE_CSV as u64, &miner)?; // mature the CSV

    let control = anchor
        .spend_info
        .control_block(&(anchor.finalize_script.clone(), LeafVersion::TapScript))
        .ok_or("no finalize control block")?;
    println!(
        "\n[4] finalize control block = {} bytes (merkle path = refund_leaf + ta_root)",
        control.serialize().len()
    );

    let dest = btc.get_new_address(None, None)?.assume_checked();
    let fee = Amount::from_sat(400);
    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: Default::default(),
            sequence: Sequence::from_height(FINALIZE_CSV),
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: value - fee,
            script_pubkey: dest.script_pubkey(),
        }],
    };
    let prevout = TxOut {
        value,
        script_pubkey: pk_script,
    };
    let leaf_hash = TapLeafHash::from_script(&anchor.finalize_script, LeafVersion::TapScript);
    let sighash = SighashCache::new(&tx).taproot_script_spend_signature_hash(
        0,
        &Prevouts::All(std::slice::from_ref(&prevout)),
        leaf_hash,
        TapSighashType::Default,
    )?;
    let sig = secp
        .sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &operator)
        .serialize();
    let mut w = Witness::new();
    w.push(sig);
    w.push(preimage);
    w.push(anchor.finalize_script.as_bytes());
    w.push(control.serialize());
    tx.input[0].witness = w;

    let spend_txid: Txid = btc.send_raw_transaction(&tx)?;
    btc.generate_to_address(1, &miner)?;
    println!("    FINALIZE spend confirmed on-chain: {spend_txid}");

    println!("\nG1 asset-layer lock + on-chain finalize spend VERIFIED.");
    Ok(())
}
