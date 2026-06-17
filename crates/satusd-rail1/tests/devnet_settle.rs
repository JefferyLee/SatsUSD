//! J4 settle E2E: the full Rail-1 redemption on a live devnet.
//!
//! ```text
//! mint fresh asset → lock it in OUR DLC funding output (J4 landing)
//!   → oracle announces tick T
//!   → asset-level vPSBT funded+signed (spending the funding output)
//!   → per-bucket CETs committed with skip_funding (fully
//!     deterministic anchor txs) → adaptor presign per bucket,
//!     BEFORE the outcome exists
//!   → oracle attests → ONLY the winning bucket's secret decrypts
//!   → decrypted keyspend goes in as tap_key_sig → finalize →
//!     PublishAndLogTransfer → confirmed
//! ```
//!
//! v0 scope notes: two buckets are exercised (winner + loser) to
//! prove per-bucket sighash binding and selective decryption;
//! production presigns all 2^m. The LP's BTC input is signed at
//! settle time (in the full protocol it is locked at LOCK — the
//! lock-side completion is MuSig2 work, the declared upgrade).
//!
//! Run with a live devnet (local in-process oracle), or against
//! signet with the PUBLIC oracle (announcements fetched over HTTP,
//! real maturity wait, attestation fetched after the tick):
//!
//! ```text
//! cargo test -p satusd-rail1 --test devnet_settle -- --ignored --nocapture
//! SATUSD_NET=signet cargo test -p satusd-rail1 --test devnet_settle -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::{Amount, OutPoint, TapSighashType, TxOut};
use satusd_oracle::oracle::{Announcement, Attestation, Oracle};
use satusd_rail::encode::tagged_hash;
use satusd_rail0::builder::{vpsbt_anchor_info, vpsbt_anchor_input, ANCHOR_DUST_SATS};
use satusd_rail1::adaptor::{decrypt, presign, verify_presig, AdaptorSig};
use satusd_rail1::cet::{bucket_adaptor_point, bucket_of, bucket_secret};
use satusd_rail1::funding::{
    funding_output, keyspend_secret, refund_leaf_script, sibling_preimage,
};
use satusd_tapd_client::assetwalletrpc as awrpc;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};
use secp256k1::{Secp256k1, SecretKey};

const FUND_UNITS: u64 = 5_000;
const M: u8 = 4;
const FEE_SATS: u64 = 2_000;
/// Payout base per bucket — sized so a ~0.005 BTC signet UTXO funds it.
const USER_SATS_BASE: u64 = 400_000;
const ORACLE_PRICE: u32 = 100_000;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bcli(args: &[&str]) -> serde_json::Value {
    satusd_tapd_client::env::NodeEnv::from_env(root()).bcli(args)
}

fn tapcli(args: &[&str]) -> String {
    satusd_tapd_client::env::NodeEnv::from_env(root()).tapcli(args)
}

fn compressed_even(x: &[u8; 32]) -> Vec<u8> {
    let mut v = vec![0x02];
    v.extend_from_slice(x);
    v
}

const ORACLE_ADDR: &str = "207.148.98.132:9590";

fn http_get(path: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut st = std::net::TcpStream::connect(ORACLE_ADDR).map_err(|e| e.to_string())?;
    st.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {ORACLE_ADDR}\r\nConnection: close\r\n\r\n");
    st.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut r = String::new();
    st.read_to_string(&mut r).map_err(|e| e.to_string())?;
    let (head, body) = r.split_once("\r\n\r\n").ok_or("malformed")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(head.lines().next().unwrap_or("").to_string());
    }
    Ok(body.trim().to_string())
}

/// Confirm a wallet txid: regtest mines; signet waits for the chain.
async fn confirm(env: &satusd_tapd_client::env::NodeEnv, txid: &str) {
    if env.chain == "regtest" {
        env.bcli(&["-generate", "2"]);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        return;
    }
    println!("waiting for signet confirmation of {txid}…");
    for _ in 0..120 {
        // node-level lookup (txindex=1): the tx may live in lnd's
        // wallet, not bitcoind's.
        if let Ok(tx) = env.try_bcli(&["getrawtransaction", txid, "true"]) {
            if tx["confirmations"].as_i64().unwrap_or(0) >= 1 {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }
    panic!("timeout waiting for {txid}");
}

#[tokio::test]
#[ignore = "requires live devnet (make devnet-up); SATUSD_NET=signet uses the public oracle"]
async fn j4_settle() -> Result<(), Box<dyn std::error::Error>> {
    let _r = root();
    let env = satusd_tapd_client::env::NodeEnv::from_env(root());
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let secp = Secp256k1::new();

    // ---- 0. fresh asset: guarantees coin selection picks OUR
    //         funding utxo for the CET's asset leg ----
    let ts0 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let name = format!("SatUSD-J4-{ts0}");
    tapcli(&[
        "assets",
        "mint",
        "--type",
        "normal",
        "--name",
        &name,
        "--supply",
        &FUND_UNITS.to_string(),
    ]);
    let fin = tapcli(&["assets", "mint", "finalize"]);
    let signet = env.chain == "signet";
    if signet {
        let batch: serde_json::Value = serde_json::from_str(&fin)?;
        let mint_txid = batch["batch"]["batch_txid"]
            .as_str()
            .ok_or("no batch txid")?;
        confirm(&env, mint_txid).await;
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    } else {
        bcli(&["-generate", "2"]);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner()
        .assets;
    let asset = assets
        .iter()
        .find(|a| a.asset_genesis.as_ref().is_some_and(|g| g.name == name))
        .ok_or("fresh asset not found")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();
    println!("minted {name}: {}", hex::encode(&asset_id));

    // ---- 1. lock it in our DLC funding output ----
    let funding_sk = tagged_hash("devnet/j4-settle-funding", name.as_bytes());
    let (funding_px, _) = SecretKey::from_byte_array(funding_sk)
        .unwrap()
        .x_only_public_key(&secp);
    let internal_x = funding_px.serialize();
    let refund = refund_leaf_script(
        144,
        &tagged_hash("devnet/user-x", b"u"),
        &tagged_hash("devnet/lp-x", b"l"),
    );
    let script_key = wallet
        .next_script_key(awrpc::NextScriptKeyRequest { key_family: 212 })
        .await?
        .into_inner()
        .script_key
        .ok_or("no script key")?;
    let fund_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.clone(),
            amt: FUND_UNITS,
            script_key: Some(script_key),
            internal_key: Some(taprpc::KeyDescriptor {
                raw_key_bytes: compressed_even(&internal_x),
                key_loc: None,
            }),
            tapscript_sibling: sibling_preimage(&refund),
            ..Default::default()
        })
        .await?
        .into_inner();
    let send = tap
        .send_asset(taprpc::SendAssetRequest {
            tap_addrs: vec![fund_addr.encoded],
            ..Default::default()
        })
        .await?
        .into_inner();
    let transfer = send.transfer.ok_or("no transfer")?;
    let fund_anchor_txid = {
        let mut h = transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    };
    confirm(&env, &fund_anchor_txid).await;

    let fund_out = transfer
        .outputs
        .iter()
        .find(|o| {
            o.anchor
                .as_ref()
                .is_some_and(|a| a.internal_key == compressed_even(&internal_x))
        })
        .ok_or("funding output not in transfer")?;
    let anchor = fund_out.anchor.as_ref().unwrap();
    let ta_leaf: [u8; 32] = anchor.taproot_asset_root.clone().try_into().unwrap();
    let fund = funding_output(&internal_x, &ta_leaf, &refund);
    let (fund_txid, fund_vout) = anchor.outpoint.split_once(':').unwrap();
    let funding_outpoint = OutPoint::new(fund_txid.parse()?, fund_vout.parse()?);
    let fund_value = Amount::from_sat(anchor.value as u64);
    let fund_txout = TxOut {
        value: fund_value,
        script_pubkey: bitcoin::ScriptBuf::from_hex(&format!(
            "5120{}",
            hex::encode(fund.output_x)
        ))?,
    };
    println!(
        "funding locked: {funding_outpoint} Q={}",
        hex::encode(fund.output_x)
    );

    // ---- 2. the oracle's commitment to the tick ----
    // devnet: in-process oracle. signet: the LIVE public oracle —
    // announcement fetched over HTTP, outcome genuinely unknown.
    let local_oracle = Oracle::from_seed(&tagged_hash("devnet/j4-oracle", b"seed"))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let tick = now + 90;
    let (ann, oracle_pubkey): (Announcement, [u8; 32]) = if signet {
        let hex_tlv = http_get(&format!("/v0/announcement/{tick}"))
            .map_err(|e| format!("announcement: {e}"))?;
        let tlv = hex::decode(hex_tlv)?;
        let p = satusd_oracle::tlv::parse_announcement(&tlv).map_err(|e| format!("{e:?}"))?;
        println!(
            "public oracle announcement: {} ({} nonces)",
            p.event_id,
            p.nonce_points.len()
        );
        (
            Announcement {
                event_id: p.event_id,
                nonce_points: p.nonce_points,
                tlv_bytes: tlv,
            },
            p.oracle_pubkey,
        )
    } else {
        (local_oracle.announce(tick)?, local_oracle.pubkey)
    };

    // ---- 3. asset-level CET leg: fund + sign ONE vPSBT moving the
    //         asset out of the funding output ----
    let dest_addr = tap
        .new_addr(taprpc::NewAddrRequest {
            asset_id: asset_id.clone(),
            amt: FUND_UNITS,
            ..Default::default()
        })
        .await?
        .into_inner()
        .encoded;
    let funded = wallet
        .fund_virtual_psbt(awrpc::FundVirtualPsbtRequest {
            template: Some(awrpc::fund_virtual_psbt_request::Template::Raw(
                awrpc::TxTemplate {
                    recipients: [(dest_addr, 0u64)].into_iter().collect(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        })
        .await?
        .into_inner();
    let anchor_in = vpsbt_anchor_input(&funded.funded_psbt)?;
    assert_eq!(
        anchor_in.outpoint, funding_outpoint,
        "fresh asset forces selection of the funding utxo"
    );
    let anchor_outs = vpsbt_anchor_info(&funded.funded_psbt)?;
    let signed = wallet
        .sign_virtual_psbt(awrpc::SignVirtualPsbtRequest {
            funded_psbt: funded.funded_psbt.clone(),
        })
        .await?
        .into_inner();

    // ---- 4. LP leg + payout addresses ----
    // Release any wallet locks left by a killed LP daemon (single-
    // user harness assumption; locks don't survive bitcoind restarts
    // but do survive daemon crashes).
    let _ = env.try_bcli(&["lockunspent", "true"]);
    let unspent = bcli(&["listunspent"]);
    let lp_utxo = unspent
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["amount"].as_f64().unwrap() >= 0.0042)
        .ok_or("no LP utxo")?;
    let lp_outpoint = OutPoint::new(
        lp_utxo["txid"].as_str().unwrap().parse()?,
        lp_utxo["vout"].as_u64().unwrap() as u32,
    );
    let lp_txout = TxOut {
        value: Amount::from_btc(lp_utxo["amount"].as_f64().unwrap())?,
        script_pubkey: bitcoin::ScriptBuf::from_hex(lp_utxo["scriptPubKey"].as_str().unwrap())?,
    };
    let payout_script = {
        let a = bcli(&["getnewaddress", "j4-user", "bech32"]);
        bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let lp_change_script = {
        let a = bcli(&["getnewaddress", "j4-lp-change", "bech32"]);
        bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // ---- 5. per-bucket CETs: commit (skip_funding → fully
    //         deterministic) + adaptor presign, BEFORE the outcome ----
    let tweaked = keyspend_secret(&funding_sk, &fund.merkle_root)?;
    let (qx, _) = SecretKey::from_byte_array(tweaked)
        .unwrap()
        .x_only_public_key(&secp);
    assert_eq!(qx.serialize(), fund.output_x);

    struct BucketCet {
        bucket: u32,
        committed_anchor: Vec<u8>,
        vpsbts: Vec<Vec<u8>>,
        passives: Vec<Vec<u8>>,
        presig: AdaptorSig,
        sighash: [u8; 32],
    }
    // Presign EVERY bucket — the outcome does not exist yet (on
    // signet this is literally true: the price is 90 s in the future).
    let mut cets: Vec<BucketCet> = Vec::new();
    for b in 0..(1u32 << M) {
        let user_sats = USER_SATS_BASE + u64::from(b); // distinct per bucket
        let lp_change = lp_txout.value.to_sat() - user_sats - FEE_SATS;

        // Anchor template: [funding, LP] in; [asset slots ‖ payout ‖
        // LP change] out.
        let mut tx_outs: Vec<TxOut> = anchor_outs
            .iter()
            .map(|_| TxOut {
                value: Amount::from_sat(ANCHOR_DUST_SATS),
                script_pubkey: fund_txout.script_pubkey.clone(), // placeholder P2TR
            })
            .collect();
        tx_outs.push(TxOut {
            value: Amount::from_sat(user_sats),
            script_pubkey: bitcoin::ScriptBuf::from_hex(&payout_script)?,
        });
        let change_index = tx_outs.len() as i32;
        tx_outs.push(TxOut {
            value: Amount::from_sat(lp_change),
            script_pubkey: bitcoin::ScriptBuf::from_hex(&lp_change_script)?,
        });
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![
                bitcoin::TxIn {
                    previous_output: funding_outpoint,
                    sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                    ..Default::default()
                },
                bitcoin::TxIn {
                    previous_output: lp_outpoint,
                    sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                    ..Default::default()
                },
            ],
            output: tx_outs,
        };
        let mut template = Psbt::from_unsigned_tx(tx)?;
        template.inputs[0].witness_utxo = Some(fund_txout.clone());
        template.inputs[0].tap_internal_key =
            Some(bitcoin::XOnlyPublicKey::from_slice(&internal_x)?);
        template.inputs[0].tap_merkle_root = Some(bitcoin::taproot::TapNodeHash::from_byte_array(
            fund.merkle_root,
        ));
        template.inputs[1].witness_utxo = Some(lp_txout.clone());
        for (i, info) in anchor_outs.iter().enumerate() {
            satusd_rail0::builder::apply_anchor_out_info(&mut template.outputs[i], info);
        }

        let committed = wallet
            .commit_virtual_psbts(awrpc::CommitVirtualPsbtsRequest {
                virtual_psbts: vec![signed.signed_psbt.clone()],
                passive_asset_psbts: funded.passive_asset_psbts.clone(),
                anchor_psbt: template.serialize(),
                anchor_change_output: Some(
                    awrpc::commit_virtual_psbts_request::AnchorChangeOutput::ExistingOutputIndex(
                        change_index,
                    ),
                ),
                skip_funding: true,
                ..Default::default()
            })
            .await?
            .into_inner();

        // The committed anchor tx is fully determined → its keyspend
        // sighash is the CET message.
        let committed_psbt = Psbt::deserialize(&committed.anchor_psbt)?;
        let mut cache = SighashCache::new(&committed_psbt.unsigned_tx);
        let sighash = cache.taproot_key_spend_signature_hash(
            0,
            &Prevouts::All(&[fund_txout.clone(), lp_txout.clone()]),
            TapSighashType::Default,
        )?;
        let msg: [u8; 32] = sighash.to_byte_array();

        let point = bucket_adaptor_point(&ann, &oracle_pubkey, M, b)?;
        let nonce_base = tagged_hash("devnet/j4-cet-nonce", &b.to_be_bytes());
        let presig = presign(&tweaked, &nonce_base, &msg, &point)?;
        assert!(verify_presig(&presig, &fund.output_x, &msg, &point)?);

        cets.push(BucketCet {
            bucket: b,
            committed_anchor: committed.anchor_psbt,
            vpsbts: committed.virtual_psbts,
            passives: committed.passive_asset_psbts,
            presig,
            sighash: msg,
        });
        println!("bucket {b}: CET committed + presigned (user_sats={user_sats})");
    }

    // ---- 6. the outcome arrives ----
    let att: Attestation = if signet {
        let now2 = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if tick + 2 > now2 {
            println!("waiting {}s for the tick to mature…", tick + 2 - now2);
            tokio::time::sleep(std::time::Duration::from_secs(tick + 2 - now2)).await;
        }
        let mut tlv = None;
        for _ in 0..10 {
            match http_get(&format!("/v0/attestation/{tick}")) {
                Ok(h) => {
                    tlv = Some(hex::decode(h)?);
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_secs(3)).await,
            }
        }
        let tlv = tlv.ok_or("attestation never appeared")?;
        let p = satusd_oracle::tlv::parse_attestation(&tlv).map_err(|e| format!("{e:?}"))?;
        let digits: Vec<u8> = p.outcomes.iter().map(|o| u8::from(o == "1")).collect();
        Attestation {
            event_id: p.event_id,
            price_usd: satusd_oracle::event::price_from_digits(&digits),
            signatures: p.signatures,
            tlv_bytes: tlv,
        }
    } else {
        local_oracle.attest(tick, ORACLE_PRICE)?
    };
    let winner = bucket_of(att.price_usd, M);
    let loser = winner ^ 1;
    println!("attested: {} → bucket {winner}", att.price_usd);

    // Selective decryption: the loser bucket has NO valid secret.
    assert!(bucket_secret(&att, M, loser).is_err());

    let t = bucket_secret(&att, M, winner)?;
    let win = cets.iter().find(|c| c.bucket == winner).unwrap();
    let sig = decrypt(&win.presig, &t)?;
    let q = secp256k1::XOnlyPublicKey::from_byte_array(fund.output_x)?;
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(sig),
        &win.sighash,
        &q,
    )?;

    // ---- 7. assemble + broadcast the winning CET ----
    let mut final_psbt = Psbt::deserialize(&win.committed_anchor)?;
    final_psbt.inputs[0].tap_key_sig = Some(bitcoin::taproot::Signature {
        signature: bitcoin::secp256k1::schnorr::Signature::from_slice(&sig)?,
        sighash_type: TapSighashType::Default,
    });
    let processed = bcli(&[
        "walletprocesspsbt",
        &BASE64_STANDARD.encode(final_psbt.serialize()),
    ]);
    let finalized = bcli(&["finalizepsbt", processed["psbt"].as_str().unwrap(), "false"]);
    assert_eq!(
        finalized["complete"].as_bool(),
        Some(true),
        "both inputs finalize: {finalized}"
    );
    let final_bytes = BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?;

    let resp = wallet
        .publish_and_log_transfer(awrpc::PublishAndLogRequest {
            anchor_psbt: final_bytes,
            virtual_psbts: win.vpsbts.clone(),
            passive_asset_psbts: win.passives.clone(),
            change_output_index: -1,
            ..Default::default()
        })
        .await?
        .into_inner();
    let settle_transfer = resp.transfer.ok_or("no transfer in publish")?;
    println!("CET broadcast: {}", {
        let mut h = settle_transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    });
    let settle_txid = {
        let mut h = settle_transfer.anchor_tx_hash.clone();
        h.reverse();
        hex::encode(h)
    };
    if signet {
        // mempool acceptance is the gate; the chain confirms on its
        // own clock (CET chains on the funding tx in the mempool).
        let entry = env
            .try_bcli(&["getrawtransaction", &settle_txid, "true"])
            .expect("CET visible to the node");
        assert!(entry["txid"].as_str() == Some(settle_txid.as_str()));
        println!("signet CET accepted: {settle_txid}");
    } else {
        bcli(&["-generate", "2"]);
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // ---- 8. confirm: payout on-chain + asset moved ----
    let tx = Psbt::deserialize(&win.committed_anchor)?
        .unsigned_tx
        .clone();
    let payout_present = tx.output.iter().any(|o| {
        o.script_pubkey == bitcoin::ScriptBuf::from_hex(&payout_script).unwrap()
            && o.value.to_sat() == USER_SATS_BASE + u64::from(winner)
    });
    assert!(payout_present, "winning bucket's payout is in the CET");

    let total: u64 = tap
        .list_assets(taprpc::ListAssetRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner()
        .assets
        .iter()
        .filter(|a| a.asset_genesis.as_ref().map(|g| g.asset_id.as_slice()) == Some(&asset_id[..]))
        .map(|a| a.amount)
        .sum();
    assert_eq!(total, FUND_UNITS, "asset conserved through the CET");

    println!(
        "J4 SETTLED: oracle-gated CET spent the DLC funding output by key path — \
         {} units moved + {} sats paid, bucket {winner} of 2^{M}",
        FUND_UNITS,
        USER_SATS_BASE + u64::from(winner)
    );
    Ok(())
}

/// redeem_tx (spec 07, the two-input model — feasibility probe): a holder
/// redeems UNILATERALLY in ONE tx that
///   (A) burns the note (a plain SatUSD UTXO) to the deterministic NUMS
///       burn sink (= devnet_burn_settle), and
///   (Q) pays the holder X/P BTC + change to the LP from the LP's
///       over-collateralised, pure-BTC DLC output, unlocked by the
///       oracle-adaptor CET the LP pre-signed at issuance (= vault
///       vault_redeem_q) — NO LP signature/presence at redeem-time.
/// Reuses j4_settle's composed deterministic CET (skip_funding) + keyspend
/// injection. The collateral leg is oracle-adaptor (not bitcoind-signed
/// like j4's LP input), so the whole redeem is unilateral.
#[tokio::test]
#[ignore = "requires live devnet bitcoind (make devnet-up)"]
async fn redeem_two_input() -> Result<(), Box<dyn std::error::Error>> {
    const COLLATERAL_SATS: u64 = 2_000_000;
    const MINT_MICRO_USD: u64 = 1_000_000_000; // $1,000
    const REDEEM_PRICE: u32 = ORACLE_PRICE; // $100,000
    let env = satusd_tapd_client::env::NodeEnv::from_env(root());
    assert_eq!(env.chain, "regtest", "regtest-only");
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let secp = Secp256k1::new();

    // ===== A — the note: a fresh GROUPED SatUSD UTXO (grouped: the burn
    //       sink is family-keyed) =====
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let name = format!("SatUSD-redeem2-{ts}");
    tapcli(&["assets", "mint", "--type", "normal", "--name", &name, "--supply",
        &FUND_UNITS.to_string(), "--new_grouped_asset"]);
    tapcli(&["assets", "mint", "finalize"]);
    bcli(&["-generate", "2"]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let assets = tap.list_assets(taprpc::ListAssetRequest::default()).await?.into_inner().assets;
    let asset = assets.iter().find(|a| a.asset_genesis.as_ref().is_some_and(|g| g.name == name))
        .ok_or("fresh asset not found")?;
    let asset_id = asset.asset_genesis.as_ref().unwrap().asset_id.clone();
    let group_key: [u8; 33] = asset.asset_group.as_ref().ok_or("no group")?
        .tweaked_group_key.clone().try_into().map_err(|_| "group key len")?;

    // ===== Q — the LP collateral: a pure-BTC DLC output (single-leaf
    //       {refund}); LP pre-signs its oracle-adaptor redeem CET =====
    let funding_sk = tagged_hash("devnet/redeem2-q", name.as_bytes());
    let internal_x = SecretKey::from_byte_array(funding_sk).unwrap().x_only_public_key(&secp).0.serialize();
    let refund = refund_leaf_script(144, &tagged_hash("devnet/redeem2-u", b"u"), &tagged_hash("devnet/redeem2-l", b"l"));
    let merkle_root: [u8; 32] = bitcoin::taproot::TapLeafHash::from_script(&refund, bitcoin::taproot::LeafVersion::TapScript).to_byte_array();
    let q_output_x = satusd_crypto::nums::tap_tweak(&internal_x, &merkle_root);
    let tweaked = keyspend_secret(&funding_sk, &merkle_root)?;
    assert_eq!(SecretKey::from_byte_array(tweaked)?.x_only_public_key(&secp).0.serialize(), q_output_x,
        "keyspend secret controls Q");
    let q_spk = format!("5120{}", hex::encode(q_output_x));
    let q_addr = bcli(&["decodescript", &q_spk])["address"].as_str().ok_or("q addr")?.to_string();
    let q_txid = bcli(&["sendtoaddress", &q_addr, &format!("{:.8}", COLLATERAL_SATS as f64 / 1e8)])
        .as_str().ok_or("send q")?.to_string();
    bcli(&["-generate", "2"]);
    let q_raw = bcli(&["getrawtransaction", &q_txid, "true"]);
    let q_vout = q_raw["vout"].as_array().unwrap().iter()
        .find(|o| o["scriptPubKey"]["hex"].as_str() == Some(q_spk.as_str())).ok_or("q vout")?;
    let q_outpoint = OutPoint::new(q_txid.parse()?, q_vout["n"].as_u64().unwrap() as u32);
    let q_txout = TxOut { value: Amount::from_sat(COLLATERAL_SATS), script_pubkey: bitcoin::ScriptBuf::from_hex(&q_spk)? };
    println!("Q (LP collateral): {} sats at {}", COLLATERAL_SATS, hex::encode(q_output_x));

    // ===== burn sink for A (deterministic NUMS, family-keyed) =====
    let family = satusd_types::derive::asset_family_id(&asset_id.clone().try_into().unwrap(), &group_key, 0);
    let sink = satusd_crypto::nums::protocol_sink_script_key(&family);
    let burn_internal = satusd_crypto::nums::protocol_burn_internal_key(&family);
    let burn_tweak = satusd_crypto::nums::protocol_burn_tweak(&family);
    let burn_addr = tap.new_addr(taprpc::NewAddrRequest {
        asset_id: asset_id.clone(),
        amt: FUND_UNITS,
        script_key: Some(taprpc::ScriptKey {
            pub_key: sink.to_vec(),
            key_desc: Some(taprpc::KeyDescriptor { raw_key_bytes: compressed_even(&burn_internal), key_loc: None }),
            tap_tweak: burn_tweak.to_vec(),
            r#type: taprpc::ScriptKeyType::ScriptKeyScriptPathExternal as i32,
        }),
        internal_key: Some(taprpc::KeyDescriptor { raw_key_bytes: compressed_even(&burn_internal), key_loc: None }),
        skip_proof_courier_conn_check: true,
        ..Default::default()
    }).await?.into_inner();

    // ===== redeem CET: fund A → burn sink; anchor [A, Q] → [burn, holder, change] =====
    let funded = wallet.fund_virtual_psbt(awrpc::FundVirtualPsbtRequest {
        template: Some(awrpc::fund_virtual_psbt_request::Template::Raw(awrpc::TxTemplate {
            recipients: [(burn_addr.encoded.clone(), 0u64)].into_iter().collect(),
            ..Default::default()
        })),
        ..Default::default()
    }).await?.into_inner();
    let anchor_in = vpsbt_anchor_input(&funded.funded_psbt)?; // A's anchor (the note UTXO)
    let anchor_outs = vpsbt_anchor_info(&funded.funded_psbt)?; // burn sink anchor info
    let signed = wallet.sign_virtual_psbt(awrpc::SignVirtualPsbtRequest {
        funded_psbt: funded.funded_psbt.clone(),
    }).await?.into_inner();

    let face = MINT_MICRO_USD * 100_000_000 / (u64::from(REDEEM_PRICE) * 1_000_000); // X/P sats
    let holder_spk = { let a = bcli(&["getnewaddress", "redeem2-holder", "bech32"]);
        bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"].as_str().unwrap().to_string() };
    let lp_spk = { let a = bcli(&["getnewaddress", "redeem2-lp-change", "bech32"]);
        bcli(&["getaddressinfo", a.as_str().unwrap()])["scriptPubKey"].as_str().unwrap().to_string() };

    let a_txout = TxOut { value: Amount::from_sat(anchor_in.value_sats), script_pubkey: anchor_in.script.clone() };
    let n_slots = anchor_outs.len() as u64;
    let total_in = anchor_in.value_sats + COLLATERAL_SATS;
    let change = total_in - ANCHOR_DUST_SATS * n_slots - face - FEE_SATS;

    let mut tx_outs: Vec<TxOut> = anchor_outs.iter().map(|_| TxOut {
        value: Amount::from_sat(ANCHOR_DUST_SATS),
        script_pubkey: anchor_in.script.clone(), // placeholder P2TR; tapd rewrites from anchor info
    }).collect();
    tx_outs.push(TxOut { value: Amount::from_sat(face), script_pubkey: bitcoin::ScriptBuf::from_hex(&holder_spk)? });
    let change_index = tx_outs.len() as i32;
    tx_outs.push(TxOut { value: Amount::from_sat(change), script_pubkey: bitcoin::ScriptBuf::from_hex(&lp_spk)? });

    let tx = bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![
            bitcoin::TxIn { previous_output: anchor_in.outpoint, sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME, ..Default::default() },
            bitcoin::TxIn { previous_output: q_outpoint, sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME, ..Default::default() },
        ],
        output: tx_outs,
    };
    let mut template = Psbt::from_unsigned_tx(tx)?;
    // input 0 = A's TA anchor (asset source → burn sink); lnd signs it
    template.inputs[0].witness_utxo = Some(a_txout.clone());
    template.inputs[0].tap_internal_key = Some(anchor_in.internal_key);
    if let Some(root) = anchor_in.merkle_root {
        template.inputs[0].tap_merkle_root = Some(bitcoin::TapNodeHash::from_byte_array(root));
    }
    // input 1 = Q (pure-BTC collateral); oracle-adaptor keyspend injected later
    template.inputs[1].witness_utxo = Some(q_txout.clone());
    for (i, info) in anchor_outs.iter().enumerate() {
        satusd_rail0::builder::apply_anchor_out_info(&mut template.outputs[i], info);
    }

    let committed = wallet.commit_virtual_psbts(awrpc::CommitVirtualPsbtsRequest {
        virtual_psbts: vec![signed.signed_psbt],
        passive_asset_psbts: funded.passive_asset_psbts.clone(),
        anchor_psbt: template.serialize(),
        anchor_change_output: Some(awrpc::commit_virtual_psbts_request::AnchorChangeOutput::ExistingOutputIndex(change_index)),
        skip_funding: true,
        ..Default::default()
    }).await?.into_inner();

    // ===== LP pre-signs Q's redeem CET (oracle-adaptor), before the outcome =====
    let oracle = Oracle::from_seed(&tagged_hash("devnet/redeem2-oracle", b"seed"))?;
    let tick = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 60;
    let ann = oracle.announce(tick)?;
    let bucket = bucket_of(REDEEM_PRICE, M);
    let committed_psbt = Psbt::deserialize(&committed.anchor_psbt)?;
    let sighash = SighashCache::new(&committed_psbt.unsigned_tx).taproot_key_spend_signature_hash(
        1, // Q is input 1
        &Prevouts::All(&[a_txout.clone(), q_txout.clone()]),
        TapSighashType::Default,
    )?.to_byte_array();
    let point = bucket_adaptor_point(&ann, &oracle.pubkey, M, bucket)?;
    let presig = presign(&tweaked, &tagged_hash("devnet/redeem2-nonce", &bucket.to_be_bytes()), &sighash, &point)?;
    assert!(verify_presig(&presig, &q_output_x, &sighash, &point)?, "Q redeem CET adaptor pre-sig valid");
    println!("PRESIGN: Q redeem CET pre-signed at issuance (bucket {bucket}; holder {face} + LP change {change})");

    // ===== redeem: oracle attests → holder decrypts Q's keyspend, signs A,
    //       broadcasts; NO LP at redeem-time =====
    let att = oracle.attest(tick, REDEEM_PRICE)?;
    let secret = bucket_secret(&att, M, bucket)?;
    let q_sig = decrypt(&presig, &secret)?;
    secp.verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(q_sig),
        &sighash, &secp256k1::XOnlyPublicKey::from_byte_array(q_output_x)?)?;

    let mut final_psbt = committed_psbt.clone();
    final_psbt.inputs[1].tap_key_sig = Some(bitcoin::taproot::Signature {
        signature: bitcoin::secp256k1::schnorr::Signature::from_slice(&q_sig)?,
        sighash_type: TapSighashType::Default,
    });
    // input 0 (A's TA anchor): the holder's lnd signs the merkle-tweaked keyspend
    let processed = bcli(&["walletprocesspsbt", &BASE64_STANDARD.encode(final_psbt.serialize())]);
    let lnd_channel = env.lnd_channel().await?;
    let mut lnd_wallet = satusd_tapd_client::WalletKitClient::new(lnd_channel);
    let lnd_signed = lnd_wallet.sign_psbt(satusd_tapd_client::walletrpc::SignPsbtRequest {
        funded_psbt: BASE64_STANDARD.decode(processed["psbt"].as_str().unwrap())?,
    }).await?.into_inner();
    let finalized = bcli(&["finalizepsbt", &BASE64_STANDARD.encode(&lnd_signed.signed_psbt), "false"]);
    assert_eq!(finalized["complete"].as_bool(), Some(true), "both inputs finalize: {finalized}");
    let final_bytes = BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?;

    let resp = wallet.publish_and_log_transfer(awrpc::PublishAndLogRequest {
        anchor_psbt: final_bytes,
        virtual_psbts: committed.virtual_psbts.clone(),
        passive_asset_psbts: committed.passive_asset_psbts.clone(),
        change_output_index: committed.change_output_index,
        lnd_locked_utxos: committed.lnd_locked_utxos.clone(),
        ..Default::default()
    }).await?.into_inner();
    let transfer = resp.transfer.ok_or("no transfer")?;
    let burned = transfer.outputs.iter().any(|o| o.script_key.ends_with(&sink));
    assert!(burned, "the note's SatUSD must land on the burn sink");
    bcli(&["-generate", "2"]);

    // ===== assertions: burn + holder payout + LP change in ONE tx =====
    let tx = committed_psbt.unsigned_tx.clone();
    let has = |spk: &str, v: u64| tx.output.iter().any(|o|
        o.script_pubkey == bitcoin::ScriptBuf::from_hex(spk).unwrap() && o.value.to_sat() == v);
    assert!(has(&holder_spk, face), "holder gets X/P BTC");
    assert!(has(&lp_spk, change), "LP gets the change");
    println!(
        "REDEEM (two-input) CONFIRMED: note burned to sink + holder paid {face} sats (X/P @ ${REDEEM_PRICE}) \
         + LP change {change} sats, in ONE tx, unlocked by the oracle-adaptor CET — unilateral, no LP at redeem-time."
    );
    Ok(())
}
