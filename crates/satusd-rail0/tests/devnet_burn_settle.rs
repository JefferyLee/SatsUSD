//! redeem_tx Step 1 — settle-to-burn on the regtest devnet (spec 07
//! §3, ADR-0005, decision (a)): one Bitcoin tx that **burns a SatUSD
//! note** (the TA leg lands on a deterministic, NUMS-derived,
//! provably-unspendable burn sink — client-side validated per spec 07
//! §3.3) **and pays the holder BTC** (here a plain LP input; Step 2
//! swaps it for the vault's pre-signed Q). This proves the core
//! redeem_tx binding: burn ⟺ BTC payout, atomically, in one tx.
//!
//! The burn sink is built exactly as `capture_burn_vector` does
//! (`protocol_sink_script_key` over the asset family); the composed
//! anchor is exactly the proven `devnet_swap` pipeline, with the TA
//! leg directed to the sink instead of a counterparty address.
//!
//! Requires `make devnet-up` + a grouped SatUSD asset. Run with:
//!   cargo test -p satusd-rail0 --test devnet_burn_settle -- --ignored --nocapture

use std::path::PathBuf;

use base64::prelude::*;
use bitcoin::psbt::Psbt;
use bitcoin::{Amount, OutPoint, TxOut};
use satusd_rail::manifest::{FeeSchedule, LpStake};
use satusd_rail::quote::Quote;
use satusd_rail0::builder::{fund, publish, sign_commit, AnchorTemplate};
use satusd_rail0::burn_key::parse_anchor_point;
use satusd_rail0::plan::SwapPlan;
use satusd_rail0::rail::{rail0_manifest, Rail0Params};
use satusd_tapd_client::env::NodeEnv;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};

const BURN_UNITS: u64 = 5_000;
const CHAIN_ID: u8 = 0; // regtest

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compressed_even(xonly: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(33);
    v.push(0x02);
    v.extend_from_slice(xonly);
    v
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) with a grouped SatUSD asset"]
async fn settle_to_burn() -> Result<(), Box<dyn std::error::Error>> {
    let env = NodeEnv::from_env(root());
    assert_eq!(env.chain, "regtest", "this test is regtest-only");
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);

    // ---- housekeeping: release stale leases from failed runs ----
    let utxos = tap
        .list_utxos(taprpc::ListUtxosRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .managed_utxos;
    for op in utxos.keys() {
        if let Some((txid, vout)) = parse_anchor_point(op) {
            let _ = wallet
                .remove_utxo_lease(satusd_tapd_client::assetwalletrpc::RemoveUtxoLeaseRequest {
                    outpoint: Some(taprpc::OutPoint {
                        txid: txid.to_vec(),
                        output_index: vout,
                    }),
                })
                .await;
        }
    }

    // ---- the SatUSD note to burn: a GROUPED asset (group key required
    //      for the family-keyed burn sink) ----
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
        .find(|a| {
            a.amount >= BURN_UNITS
                && a.asset_genesis.is_some()
                && a.asset_group.as_ref().is_some_and(|g| !g.tweaked_group_key.is_empty())
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
        .map_err(|_| "group_key len")?;

    // ---- the deterministic, NUMS-derived burn sink (spec 01 §4 /
    //      spec 07 §3.3; same derivation as capture_burn_vector) ----
    let family = satusd_types::derive::asset_family_id(&asset_id, &group_key, CHAIN_ID);
    let sink = satusd_crypto::nums::protocol_sink_script_key(&family);
    let burn_internal = satusd_crypto::nums::protocol_burn_internal_key(&family);
    let burn_tweak = satusd_crypto::nums::protocol_burn_tweak(&family);
    let burn_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.to_vec(),
            amt: BURN_UNITS,
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
        })
        .await?
        .into_inner();
    let tail32 = |v: &[u8]| v[v.len().saturating_sub(32)..].to_vec();
    assert_eq!(tail32(&burn_addr.script_key), sink.to_vec(), "sink echo");
    println!("burn sink script key = {}", hex::encode(sink));

    // ---- rail + quote + plan (the BTC the holder receives) ----
    let manifest = rail0_manifest(Rail0Params {
        asset_id,
        max_size_sats: 5_000_000,
        committed_term: 4032,
        fee_schedule: FeeSchedule {
            retain_bps: 10,
            service_bps: 0,
            fixed_sats: 200,
        },
        price_dev_bound_bps: 100,
        quote_validity_s: 600,
        lp_stake: LpStake {
            amount_sats: 10_000_000,
            slash_conditions: vec![],
        },
    })
    .unwrap();
    let quote = Quote {
        rail_id: manifest.rail_id(),
        price_msat_per_usd: 1_000_000,   // $100,000 / BTC
        amount_micro_usd: 1_000_000_000, // $1,000
        expiry_unix_s: u64::MAX,
        lock_template_commitment: [0; 32],
    };
    let plan = SwapPlan::from_quote(&manifest, &quote).unwrap();
    println!("plan: burn {} µUSD ⟺ holder receives {} sats", plan.burn_micro_usd, plan.user_sats);

    // ---- BTC leg: a plain LP/holder input + the holder's BTC payout
    //      (Step 2 replaces this input with the vault's pre-signed Q) ----
    let unspent = env.bcli(&["listunspent"]);
    let lp_utxo = unspent
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["amount"].as_f64().unwrap() >= 0.05)
        .ok_or("no BTC utxo")?;
    let lp_outpoint = OutPoint::new(
        lp_utxo["txid"].as_str().unwrap().parse()?,
        lp_utxo["vout"].as_u64().unwrap() as u32,
    );
    let lp_prev_txout = TxOut {
        value: Amount::from_btc(lp_utxo["amount"].as_f64().unwrap())?,
        script_pubkey: bitcoin::ScriptBuf::from_hex(lp_utxo["scriptPubKey"].as_str().unwrap())?,
    };
    let payout_addr = env.bcli(&["getnewaddress", "holder-payout", "bech32"]);
    let payout_script = env.bcli(&["getaddressinfo", payout_addr.as_str().unwrap()])
        ["scriptPubKey"]
        .as_str()
        .unwrap()
        .to_string();
    let holder_payout = TxOut {
        value: Amount::from_sat(plan.user_sats),
        script_pubkey: bitcoin::ScriptBuf::from_hex(&payout_script)?,
    };

    // ---- fund the TA leg to the BURN SINK; compose the anchor ----
    let funded = fund(&mut wallet, &burn_addr.encoded, vec![]).await?;
    println!("funded; anchor outpoint = {}", funded.anchor_input()?.outpoint);

    let template = AnchorTemplate {
        lp_outpoint,
        lp_prev_txout,
        lp_internal_key: None,
        lp_key_origin: None,
        user_payout: holder_payout.clone(),
        user_payout_internal_key: None, // P2WPKH payout
        user_payout_key_origin: None,
        extra_outputs: vec![],
    };
    let committed = sign_commit(&mut wallet, funded, &template, 2).await?;
    println!("committed: anchor_psbt={}B", committed.anchor_psbt.len());

    // ---- BTC signatures: bitcoind → lnd SignPsbt → finalize ----
    let b64 = |bytes: &[u8]| BASE64_STANDARD.encode(bytes);
    let processed = env.bcli(&["walletprocesspsbt", &b64(&committed.anchor_psbt)]);
    let lnd_channel = env.lnd_channel().await?;
    let mut lnd_wallet = satusd_tapd_client::WalletKitClient::new(lnd_channel);
    let lnd_signed = lnd_wallet
        .sign_psbt(satusd_tapd_client::walletrpc::SignPsbtRequest {
            funded_psbt: BASE64_STANDARD.decode(processed["psbt"].as_str().unwrap())?,
        })
        .await?
        .into_inner();
    let finalized = env.bcli(&["finalizepsbt", &b64(&lnd_signed.signed_psbt), "false"]);
    assert_eq!(
        finalized["complete"].as_bool(),
        Some(true),
        "all inputs must finalize: {finalized}"
    );
    let final_psbt_b64 = finalized["psbt"].as_str().ok_or("no psbt in finalize")?;
    let final_psbt = BASE64_STANDARD.decode(final_psbt_b64)?;

    // ---- publish + confirm ----
    let resp = publish(&mut wallet, committed, final_psbt).await?;
    let transfer = resp.transfer.ok_or("no transfer in publish response")?;
    // the TA leg must land on the burn sink (a burn, not a transfer to a holder)
    let burned = transfer.outputs.iter().any(|o| o.script_key.ends_with(&sink));
    assert!(burned, "the note's TA leg must land on the burn sink");
    println!("anchor tx broadcast: {}", {
        let mut h = transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    });
    env.bcli(&["-generate", "2"]);

    // ---- assertion: the holder BTC payout is in the same anchor tx ----
    let final_tx: bitcoin::Transaction = {
        let psbt = Psbt::deserialize(&BASE64_STANDARD.decode(final_psbt_b64)?)?;
        psbt.extract_tx()?
    };
    assert!(
        final_tx.output.contains(&holder_payout),
        "holder BTC payout (value {} sats) must be in the burn anchor tx",
        plan.user_sats
    );

    println!(
        "settle-to-burn confirmed: {} SatUSD units burned to the sink + {} sats paid to the holder \
         in one atomic tx (redeem_tx Step 1)",
        BURN_UNITS, plan.user_sats
    );
    Ok(())
}
