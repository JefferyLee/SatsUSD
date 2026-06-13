//! J3 composition E2E on the regtest devnet: one Bitcoin transaction
//! carrying the user's SatUSD transfer (TA layer) AND the LP's BTC
//! paying the user (BTC layer).
//!
//! Roles in this single-tapd devnet: the same tapd plays the user's
//! asset wallet and the LP's receiving wallet (the TA leg is a
//! self-addressed transfer — the BTC-level composition mechanics are
//! identical to the two-party case). The LP's BTC leg and the user's
//! payout address live in the bitcoind regtest wallet.
//!
//! Signing pipeline: bitcoind signs+finalizes the LP input; lnd's
//! walletrpc.SignPsbt signs the TA anchor keyspend (a merkle-root-
//! tweaked keyspend, beyond lncli finalize's simple signer);
//! bitcoind's finalizepsbt assembles the final witnesses.
//!
//! Requires `make devnet-up` + a grouped asset minted. Run with:
//!
//! ```text
//! cargo test -p satusd-rail0 --test devnet_swap -- --ignored --nocapture
//! ```

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
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};

const SWAP_ASSET_UNITS: u64 = 5_000;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bcli(args: &[&str]) -> serde_json::Value {
    satusd_tapd_client::env::NodeEnv::from_env(root()).bcli(args)
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) with a grouped asset"]
async fn j3_composed_swap() -> Result<(), Box<dyn std::error::Error>> {
    let _r = root();
    let env = satusd_tapd_client::env::NodeEnv::from_env(root());
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

    // ---- user side: the SatUSD asset ----
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
        .find(|a| a.amount >= SWAP_ASSET_UNITS && a.asset_genesis.is_some())
        .ok_or("no asset with enough balance")?;
    let asset_id: [u8; 32] = asset
        .asset_genesis
        .as_ref()
        .unwrap()
        .asset_id
        .clone()
        .try_into()
        .map_err(|_| "asset_id len")?;
    let balance_before: u64 = assets
        .iter()
        .filter(|a| a.asset_genesis.as_ref().map(|g| g.asset_id.as_slice()) == Some(&asset_id))
        .map(|a| a.amount)
        .sum();

    // ---- the rail, the quote, the plan (J3's economic content) ----
    let manifest = rail0_manifest(Rail0Params {
        asset_id,
        max_size_sats: 5_000_000,
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
    println!(
        "plan: user_sats={} burn_µUSD={}",
        plan.user_sats, plan.burn_micro_usd
    );

    // ---- LP leg: a bitcoind UTXO; user payout address (P2WPKH —
    //      P2TR payouts would need their internal key for tapd's
    //      exclusion proofs) ----
    let unspent = bcli(&["listunspent"]);
    let lp_utxo = unspent
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["amount"].as_f64().unwrap() >= 0.05)
        .ok_or("no LP utxo")?;
    let lp_outpoint = OutPoint::new(
        lp_utxo["txid"].as_str().unwrap().parse()?,
        lp_utxo["vout"].as_u64().unwrap() as u32,
    );
    let lp_prev_txout = TxOut {
        value: Amount::from_btc(lp_utxo["amount"].as_f64().unwrap())?,
        script_pubkey: bitcoin::ScriptBuf::from_hex(lp_utxo["scriptPubKey"].as_str().unwrap())?,
    };
    let payout_addr = bcli(&["getnewaddress", "user-payout", "bech32"]);
    let payout_script = bcli(&["getaddressinfo", payout_addr.as_str().unwrap()])["scriptPubKey"]
        .as_str()
        .unwrap()
        .to_string();
    let user_payout = TxOut {
        value: Amount::from_sat(plan.user_sats),
        script_pubkey: bitcoin::ScriptBuf::from_hex(&payout_script)?,
    };

    // ---- LP's TA receiving address (same tapd, see module docs) ----
    let lp_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.to_vec(),
            amt: SWAP_ASSET_UNITS,
            ..Default::default()
        })
        .await?
        .into_inner()
        .encoded;

    // ---- fund FIRST; the funded vPSBT tells us which anchor
    //      outpoint tapd selected — never guess it from ListUtxos ----
    let funded = fund(&mut wallet, &lp_addr).await?;
    println!(
        "funded; anchor outpoint = {}",
        funded.anchor_input()?.outpoint
    );

    let template = AnchorTemplate {
        lp_outpoint,
        lp_prev_txout,
        lp_internal_key: None, // single-lnd harness: LP input is local
        lp_key_origin: None,
        user_payout: user_payout.clone(),
        extra_outputs: vec![],
    };
    let committed = sign_commit(&mut wallet, funded, &template, 2).await?;
    println!(
        "committed: change_output_index={} anchor_psbt={}B",
        committed.change_output_index,
        committed.anchor_psbt.len()
    );

    // ---- BTC signatures: bitcoind → lnd SignPsbt → finalize ----
    let b64 = |bytes: &[u8]| BASE64_STANDARD.encode(bytes);
    let processed = bcli(&["walletprocesspsbt", &b64(&committed.anchor_psbt)]);

    let lnd_channel = env.lnd_channel().await?;
    let mut lnd_wallet = satusd_tapd_client::WalletKitClient::new(lnd_channel);
    let lnd_signed = lnd_wallet
        .sign_psbt(satusd_tapd_client::walletrpc::SignPsbtRequest {
            funded_psbt: BASE64_STANDARD.decode(processed["psbt"].as_str().unwrap())?,
        })
        .await?
        .into_inner();
    println!("lnd signed inputs: {:?}", lnd_signed.signed_inputs);

    let finalized = bcli(&["finalizepsbt", &b64(&lnd_signed.signed_psbt), "false"]);
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
    println!("anchor tx broadcast: {}", {
        let mut h = transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    });
    bcli(&["-generate", "2"]);

    // ---- assertions ----
    // S2 leg 1: the user payout output is in the broadcast anchor tx.
    let final_tx: bitcoin::Transaction = {
        let psbt = Psbt::deserialize(&BASE64_STANDARD.decode(final_psbt_b64)?)?;
        psbt.extract_tx()?
    };
    assert!(
        final_tx.output.contains(&user_payout),
        "user payout output (value {} sats) must be in the anchor tx",
        plan.user_sats
    );

    // TA leg: wait for tapd to register the confirmation. The change
    // leg (balance_before − SWAP_ASSET_UNITS) must come back; the
    // recipient leg arrives via the proof courier and may lag, so
    // assert the lower bound only.
    let want_change = balance_before - SWAP_ASSET_UNITS;
    let mut total_after = 0u64;
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let assets_after = tap
            .list_assets(taprpc::ListAssetRequest {
                include_leased: true,
                ..Default::default()
            })
            .await?
            .into_inner()
            .assets;
        total_after = assets_after
            .iter()
            .filter(|a| a.asset_genesis.as_ref().map(|g| g.asset_id.as_slice()) == Some(&asset_id))
            .map(|a| a.amount)
            .sum();
        if total_after >= want_change {
            break;
        }
    }
    assert!(
        total_after >= want_change,
        "change leg must return after confirmation: have {total_after}, want ≥ {want_change}"
    );

    println!(
        "J3 composed swap confirmed: TA {} units moved + {} sats paid to user in one tx",
        SWAP_ASSET_UNITS, plan.user_sats
    );
    Ok(())
}
