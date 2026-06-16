//! Rail-0 ISSUANCE-direction E2E on the regtest devnet (spec 07 §2,
//! ADR-0005): the v0 "buy a note from an LP" path (PRD J1).
//!
//! Two steps, demonstrating what makes issuance distinct from a swap:
//!   1. the LP **mints `$X` fresh SatUSD into the group ON DEMAND**
//!      (group-key reissuance — the supply grows by exactly `$X`), then
//!   2. **delivers it to the buyer in one atomic Bitcoin tx** that also
//!      carries the buyer's BTC payment — the same composition the swap
//!      test proves, with the TA leg in the issue direction (LP→buyer)
//!      and the BTC leg as the buyer's payment to the LP.
//!
//! Single-tapd harness: one tapd plays both the LP (issuer) and the
//! buyer; the BTC legs live in the bitcoind regtest wallet (so the
//! buyer↔LP direction is the framing — the composition mechanics are
//! identical to the two-party case, exactly as in devnet_swap).
//!
//! Requires `make devnet-up` + the grouped SatUSD asset. Run with:
//!
//! ```text
//! cargo test -p satusd-rail0 --test devnet_issuance -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use base64::prelude::*;
use bitcoin::psbt::Psbt;
use bitcoin::{Amount, OutPoint, TxOut};
use satusd_rail::manifest::{Direction, FeeSchedule, LpStake};
use satusd_rail::quote::Quote;
use satusd_rail0::builder::{fund, publish, sign_commit, AnchorTemplate};
use satusd_rail0::burn_key::parse_anchor_point;
use satusd_rail0::plan::SwapPlan;
use satusd_rail0::rail::{rail0_manifest, Rail0Params};
use satusd_tapd_client::env::NodeEnv;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};

const ISSUE_UNITS: u64 = 5_000; // the $X the LP mints on demand + sells

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn group_supply(assets: &[taprpc::Asset], gk: &[u8]) -> u64 {
    assets
        .iter()
        .filter(|a| {
            a.asset_group
                .as_ref()
                .map(|g| g.tweaked_group_key.as_slice())
                == Some(gk)
        })
        .map(|a| a.amount)
        .sum()
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up) with the grouped SatUSD asset"]
async fn j1_issuance_buy_note() -> Result<(), Box<dyn std::error::Error>> {
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

    // ---- find the SatUSD group (the canonical SatUSD-dev group) ----
    let assets = tap
        .list_assets(taprpc::ListAssetRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .assets;
    let group_key: Vec<u8> = assets
        .iter()
        .filter_map(|a| a.asset_group.as_ref())
        .map(|g| g.tweaked_group_key.clone())
        .find(|gk| !gk.is_empty())
        .ok_or("no grouped asset on this devnet — mint one (see devnet/README.md)")?;
    let gk_hex = hex::encode(&group_key);
    let supply_before = group_supply(&assets, &group_key);
    println!("SatUSD group {}… supply before = {supply_before}", &gk_hex[..16]);

    // ---- 1. ISSUE: the LP mints $X fresh into the group on demand ----
    let ts = env.bcli(&["getblockcount"]).as_u64().unwrap_or(0);
    let mint_name = format!("SatUSD-issue-{ts}");
    env.tapcli(&[
        "assets", "mint", "--type", "normal", "--name", &mint_name, "--supply",
        &ISSUE_UNITS.to_string(), "--grouped_asset", "--group_key", &gk_hex,
    ]);
    env.tapcli(&["assets", "mint", "finalize"]);
    env.bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let assets = tap
        .list_assets(taprpc::ListAssetRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .assets;
    let supply_after = group_supply(&assets, &group_key);
    assert_eq!(
        supply_after,
        supply_before + ISSUE_UNITS,
        "mint-on-demand: the SatUSD group supply must grow by exactly the minted $X"
    );
    println!("ISSUE: group supply {supply_before} → {supply_after} (+{ISSUE_UNITS} minted on demand)");

    // the freshly-minted tranche the buyer will receive
    let minted = assets
        .iter()
        .find(|a| {
            a.asset_genesis.as_ref().map(|g| g.name.as_str()) == Some(mint_name.as_str())
                && a.amount >= ISSUE_UNITS
        })
        .ok_or("freshly-minted tranche not found")?;
    let asset_id: [u8; 32] = minted
        .asset_genesis
        .as_ref()
        .unwrap()
        .asset_id
        .clone()
        .try_into()
        .map_err(|_| "asset_id len")?;

    // ---- the rail (issuance direction) + quote + plan ----
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
    assert_eq!(
        manifest.direction,
        Direction::Mint,
        "Rail-0 is the issuance rail (BTC→note)"
    );
    let quote = Quote {
        rail_id: manifest.rail_id(),
        price_msat_per_usd: 1_000_000,   // $100,000 / BTC
        amount_micro_usd: 1_000_000_000, // $1,000
        expiry_unix_s: u64::MAX,
        lock_template_commitment: [0; 32],
    };
    let plan = SwapPlan::from_quote(&manifest, &quote).unwrap();
    println!("plan: buyer pays {} sats for {} SatUSD units", plan.user_sats, ISSUE_UNITS);

    // ---- 2. DELIVER: one atomic tx — TA leg LP→buyer, BTC buyer→LP ----
    // BTC leg: the buyer funds with a bitcoind UTXO; the LP receives the
    // payment at its own address (the payout output, value = plan price).
    let unspent = env.bcli(&["listunspent"]);
    let buyer_utxo = unspent
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["amount"].as_f64().unwrap() >= 0.05)
        .ok_or("no buyer BTC utxo")?;
    let buyer_outpoint = OutPoint::new(
        buyer_utxo["txid"].as_str().unwrap().parse()?,
        buyer_utxo["vout"].as_u64().unwrap() as u32,
    );
    let buyer_prev_txout = TxOut {
        value: Amount::from_btc(buyer_utxo["amount"].as_f64().unwrap())?,
        script_pubkey: bitcoin::ScriptBuf::from_hex(buyer_utxo["scriptPubKey"].as_str().unwrap())?,
    };
    // the LP's BTC payout address (P2WPKH — a P2TR payout would need its
    // internal key for tapd's exclusion proofs)
    let lp_payout_addr = env.bcli(&["getnewaddress", "lp-payment", "bech32"]);
    let lp_payout_script = env.bcli(&["getaddressinfo", lp_payout_addr.as_str().unwrap()])
        ["scriptPubKey"]
        .as_str()
        .unwrap()
        .to_string();
    let lp_payment = TxOut {
        value: Amount::from_sat(plan.user_sats),
        script_pubkey: bitcoin::ScriptBuf::from_hex(&lp_payout_script)?,
    };

    // TA leg: the buyer's receiving address for the freshly-minted note.
    let buyer_ta_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.to_vec(),
            amt: ISSUE_UNITS,
            ..Default::default()
        })
        .await?
        .into_inner()
        .encoded;

    // fund FIRST; the funded vPSBT tells us the anchor outpoint tapd chose.
    let funded = fund(&mut wallet, &buyer_ta_addr, vec![]).await?;
    println!("funded; anchor outpoint = {}", funded.anchor_input()?.outpoint);

    let template = AnchorTemplate {
        // the BTC input is the buyer's payment; the payout goes to the LP.
        lp_outpoint: buyer_outpoint,
        lp_prev_txout: buyer_prev_txout,
        lp_internal_key: None, // single-node harness: input is local
        lp_key_origin: None,
        user_payout: lp_payment.clone(),
        user_payout_internal_key: None, // P2WPKH payout
        user_payout_key_origin: None,
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
    println!("anchor tx broadcast: {}", {
        let mut h = transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    });
    env.bcli(&["-generate", "2"]);

    // ---- assertions ----
    // BTC leg: the LP's payment output is in the broadcast anchor tx.
    let final_tx: bitcoin::Transaction = {
        let psbt = Psbt::deserialize(&BASE64_STANDARD.decode(final_psbt_b64)?)?;
        psbt.extract_tx()?
    };
    assert!(
        final_tx.output.contains(&lp_payment),
        "the LP's payment output (value {} sats) must be in the anchor tx",
        plan.user_sats
    );

    // TA leg: the minted tranche transferred to the buyer (confirmation
    // registered by tapd). Single-tapd: assert the units survive the
    // transfer (self-addressed), the lower bound.
    let mut delivered = 0u64;
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let after = tap
            .list_assets(taprpc::ListAssetRequest {
                include_leased: true,
                ..Default::default()
            })
            .await?
            .into_inner()
            .assets;
        delivered = after
            .iter()
            .filter(|a| a.asset_genesis.as_ref().map(|g| g.asset_id.as_slice()) == Some(&asset_id))
            .map(|a| a.amount)
            .sum();
        if delivered >= ISSUE_UNITS {
            break;
        }
    }
    assert!(
        delivered >= ISSUE_UNITS,
        "the minted note must be delivered after confirmation: have {delivered}, want ≥ {ISSUE_UNITS}"
    );

    println!(
        "J1 issuance confirmed: LP minted {ISSUE_UNITS} SatUSD on demand and delivered them \
         to the buyer for {} sats in one atomic tx",
        plan.user_sats
    );
    Ok(())
}
