//! `satusd` — the holder's CLI (J2 verify-ish, J3 redeem).
//!
//! ```text
//! satusd balance
//! satusd quote  <lp-url> <amount-usd>              # look, don't touch
//! satusd redeem <lp-url> <amount-usd> [--payout <addr>] [--oracle <addr>]
//! ```
//!
//! Network via SATUSD_NET (devnet default | signet); run from the
//! repository root (or set SATUSD_ROOT). The redeem path performs
//! the full user-side verification battery BEFORE co-signing
//! anything: manifest reconstruction → rail_id, SwapPlan →
//! lock_template_commitment, the LP's BIP-340 quote signature, and
//! the quoted price against the public oracle within the rail's own
//! price_dev_bound. Trust the math, not the endpoint.

use std::path::PathBuf;
use std::str::FromStr;

use base64::prelude::*;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, TxOut};
use satusd_lp::{http_get, http_post};
use satusd_rail::manifest_json;
use satusd_rail::quote::Quote;
use satusd_rail0::builder::{fund, publish, sign_commit, AnchorTemplate};
use satusd_rail0::plan::SwapPlan;
use satusd_tapd_client::env::NodeEnv;
use satusd_tapd_client::{taprpc, AssetWalletClient, TaprootAssetsClient};

const DEFAULT_ORACLE: &str = "207.148.98.132:9590";

type Error = Box<dyn std::error::Error>;

fn root() -> PathBuf {
    std::env::var("SATUSD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("cwd"))
}

/// Parse a quote's `lp_btc_input.derivation` ("<fp_hex>/<p1>/<p2>...",
/// hardened components suffixed `h` or `'`) into a BIP-32 key source.
fn parse_origin_str(
    s: &str,
) -> Option<(bitcoin::bip32::Fingerprint, bitcoin::bip32::DerivationPath)> {
    use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint};
    let (fp, path) = s.split_once('/')?;
    let fp_bytes: [u8; 4] = hex::decode(fp).ok()?.try_into().ok()?;
    let mut children = Vec::new();
    for c in path.split('/') {
        let (num, hardened) = match c.strip_suffix(['h', '\'']) {
            Some(n) => (n, true),
            None => (c, false),
        };
        let idx: u32 = num.parse().ok()?;
        children.push(if hardened {
            ChildNumber::from_hardened_idx(idx).ok()?
        } else {
            ChildNumber::from_normal_idx(idx).ok()?
        });
    }
    Some((Fingerprint::from(fp_bytes), DerivationPath::from(children)))
}

/// "12.34" → 12_340_000 µUSD, no floats.
fn parse_usd(s: &str) -> Result<u64, Error> {
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if frac.len() > 6 {
        return Err("more than 6 decimal places".into());
    }
    let int: u64 = if int.is_empty() { 0 } else { int.parse()? };
    let frac: u64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<6}").parse()?
    };
    Ok(int * 1_000_000 + frac)
}

fn oracle_price_usd(addr: &str) -> Result<u32, Error> {
    let latest = http_get(addr, "/v0/latest").map_err(|e| e.to_string())?;
    let att_hex =
        http_get(addr, &format!("/v0/attestation/{}", latest.trim())).map_err(|e| e.to_string())?;
    let tlv = hex::decode(att_hex.trim())?;
    let att = satusd_oracle::tlv::parse_attestation(&tlv).map_err(|e| format!("{e:?}"))?;
    let digits: Vec<u8> = att.outcomes.iter().map(|o| u8::from(o == "1")).collect();
    Ok(satusd_oracle::event::price_from_digits(&digits))
}

async fn balance(env: &NodeEnv) -> Result<(), Error> {
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel);
    let assets = tap
        .list_assets(taprpc::ListAssetRequest::default())
        .await?
        .into_inner();
    let mut total = 0u64;
    for a in &assets.assets {
        let g = a.asset_genesis.as_ref().unwrap();
        if g.name.starts_with("SatUSD") {
            total += a.amount;
            println!(
                "  {} µUSD  (asset {}…)",
                a.amount,
                &hex::encode(&g.asset_id)[..16]
            );
        }
    }
    println!(
        "total: {} µUSD = ${}.{:06}",
        total,
        total / 1_000_000,
        total % 1_000_000
    );
    Ok(())
}

struct VerifiedQuote {
    q: serde_json::Value,
    user_sats: u64,
}

/// Fetch + fully verify a quote. Prints the verification battery.
fn fetch_verified_quote(
    lp: &str,
    amount_micro_usd: u64,
    payout_address: &str,
    oracle: &str,
) -> Result<VerifiedQuote, Error> {
    let m = serde_json::from_str::<serde_json::Value>(
        &http_get(lp, "/v0/manifest").map_err(|e| e.to_string())?,
    )?;
    let manifest =
        manifest_json::from_value(&m["manifest"]).map_err(|e| format!("manifest: {e}"))?;
    let rail_id = manifest.rail_id();
    println!("rail_id  : {} (recomputed locally)", hex::encode(rail_id));

    let q = http_post(
        lp,
        "/v0/quote",
        &serde_json::json!({
            "amount_micro_usd": amount_micro_usd.to_string(),
            "payout_address": payout_address,
        }),
    )
    .map_err(|e| e.to_string())?;

    // 1. quote binds to the manifest we verified
    let quote = Quote {
        rail_id,
        price_msat_per_usd: q["quote"]["price_msat_per_usd"]
            .as_str()
            .ok_or("price")?
            .parse()?,
        amount_micro_usd,
        expiry_unix_s: q["quote"]["expiry_unix_s"]
            .as_str()
            .ok_or("expiry")?
            .parse()?,
        lock_template_commitment: <[u8; 32]>::try_from(
            hex::decode(
                q["quote"]["lock_template_commitment"]
                    .as_str()
                    .ok_or("ltc")?,
            )?
            .as_slice(),
        )?,
    };
    let plan = SwapPlan::from_quote(&manifest, &quote).map_err(|e| format!("{e:?}"))?;
    if plan.lock_template_commitment() != quote.lock_template_commitment {
        return Err(
            "lock_template_commitment does not match the recomputed plan — REFUSING".into(),
        );
    }
    println!("plan     : ✓ commitment matches local recomputation");

    // 2. LP signature over the quote payload
    let payload = quote.signing_payload();
    if hex::encode(payload) != q["signing_payload"].as_str().unwrap_or_default() {
        return Err("signing_payload mismatch — REFUSING".into());
    }
    let secp = secp256k1::Secp256k1::verification_only();
    let sig_bytes: [u8; 64] =
        <[u8; 64]>::try_from(hex::decode(q["lp_sig"].as_str().ok_or("lp_sig")?)?.as_slice())?;
    let lp_pk = secp256k1::XOnlyPublicKey::from_byte_array(<[u8; 32]>::try_from(
        hex::decode(q["lp_pubkey"].as_str().ok_or("lp_pubkey")?)?.as_slice(),
    )?)?;
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(sig_bytes),
        &payload,
        &lp_pk,
    )
    .map_err(|_| "LP quote signature INVALID — REFUSING")?;
    println!(
        "lp_sig   : ✓ BIP-340 valid under {}",
        &q["lp_pubkey"].as_str().unwrap()[..16]
    );

    // 3. price sanity vs the public oracle, within the rail's bound
    let oracle_usd = u64::from(oracle_price_usd(oracle)?);
    let quoted_usd = 100_000_000_000 / quote.price_msat_per_usd;
    let dev_bps = quoted_usd.abs_diff(oracle_usd) * 10_000 / oracle_usd;
    if dev_bps > u64::from(manifest.price_dev_bound_bps) {
        return Err(format!(
            "quoted ${quoted_usd} deviates {dev_bps} bps from oracle ${oracle_usd} (bound {}) — REFUSING",
            manifest.price_dev_bound_bps
        )
        .into());
    }
    println!("price    : ✓ ${quoted_usd} within {dev_bps} bps of oracle ${oracle_usd}");

    let user_sats = plan.user_sats;
    println!(
        "you give : {} µUSD (${}.{:06})",
        amount_micro_usd,
        amount_micro_usd / 1_000_000,
        amount_micro_usd % 1_000_000
    );
    println!(
        "you get  : {user_sats} sats (fees: service {} + fixed {})",
        plan.service_fee_sats, manifest.fee_schedule.fixed_sats
    );
    Ok(VerifiedQuote { q, user_sats })
}

async fn redeem(
    env: &NodeEnv,
    lp: &str,
    amount_micro_usd: u64,
    payout: Option<String>,
    oracle: &str,
) -> Result<(), Error> {
    let payout_address = match payout {
        Some(a) => a,
        None => env.bcli(&["getnewaddress"]).as_str().unwrap().to_string(),
    };
    println!("payout   : {payout_address}");
    let v = fetch_verified_quote(lp, amount_micro_usd, &payout_address, oracle)?;
    let q = &v.q;

    // Build + sign the asset leg with OUR tapd. Stale leases from
    // previously failed attempts would lock the wallet for ~10 min —
    // release them first (single-user CLI assumption).
    let channel = env.tapd_channel().await?;
    let mut tap = TaprootAssetsClient::new(channel.clone());
    let mut wallet = AssetWalletClient::new(channel);
    let utxos = tap
        .list_utxos(taprpc::ListUtxosRequest {
            include_leased: true,
            ..Default::default()
        })
        .await?
        .into_inner();
    for (op, _) in utxos.managed_utxos {
        let parts: Vec<&str> = op.split(':').collect();
        let mut txid = hex::decode(parts[0])?;
        txid.reverse();
        let _ = wallet
            .remove_utxo_lease(satusd_tapd_client::assetwalletrpc::RemoveUtxoLeaseRequest {
                outpoint: Some(taprpc::OutPoint {
                    txid,
                    output_index: parts[1].parse()?,
                }),
            })
            .await;
    }
    let funded = fund(&mut wallet, q["lp_ta_addr"].as_str().ok_or("lp_ta_addr")?).await?;
    let template = AnchorTemplate {
        lp_outpoint: OutPoint::from_str(q["lp_btc_input"]["outpoint"].as_str().ok_or("outpoint")?)?,
        lp_prev_txout: TxOut {
            value: Amount::from_sat(
                q["lp_btc_input"]["value_sats"]
                    .as_str()
                    .ok_or("v")?
                    .parse()?,
            ),
            script_pubkey: ScriptBuf::from_hex(
                q["lp_btc_input"]["script_pubkey"].as_str().ok_or("spk")?,
            )?,
        },
        lp_internal_key: q["lp_btc_input"]["internal_key"]
            .as_str()
            .filter(|s| !s.is_empty())
            .and_then(|s| bitcoin::XOnlyPublicKey::from_str(s).ok()),
        lp_key_origin: q["lp_btc_input"]["derivation"]
            .as_str()
            .filter(|s| !s.is_empty())
            .and_then(parse_origin_str),
        user_payout: TxOut {
            value: Amount::from_sat(v.user_sats),
            script_pubkey: Address::from_str(&payout_address)?
                .assume_checked()
                .script_pubkey(),
        },
        extra_outputs: match &q["lp_change"] {
            serde_json::Value::Null => vec![],
            c => vec![TxOut {
                value: Amount::from_sat(c["value_sats"].as_str().ok_or("chg")?.parse()?),
                script_pubkey: ScriptBuf::from_hex(c["script_pubkey"].as_str().ok_or("chgspk")?)?,
            }],
        },
    };
    let committed = sign_commit(&mut wallet, funded, &template, 2).await?;
    println!("asset leg: ✓ signed + committed");

    // Anchor keyspend with our lnd.
    let lnd = env.lnd_channel().await?;
    let mut lnd_wallet = satusd_tapd_client::WalletKitClient::new(lnd);
    let lnd_signed = lnd_wallet
        .sign_psbt(satusd_tapd_client::walletrpc::SignPsbtRequest {
            funded_psbt: committed.anchor_psbt.clone(),
        })
        .await?
        .into_inner();
    println!("anchor   : ✓ keyspend signed (lnd)");

    // LP co-signs over HTTP.
    let settled = http_post(
        lp,
        "/v0/settle",
        &serde_json::json!({
            "signing_payload": q["signing_payload"].as_str().unwrap(),
            "anchor_psbt": BASE64_STANDARD.encode(&lnd_signed.signed_psbt),
        }),
    )
    .map_err(|e| e.to_string())?;
    let lp_signed = settled["signed_psbt"].as_str().ok_or("no signed_psbt")?;
    println!("lp       : ✓ co-signed");

    let finalized = env.bcli(&["finalizepsbt", lp_signed, "false"]);
    if finalized["complete"].as_bool() != Some(true) {
        return Err("finalize incomplete — missing signatures".into());
    }
    let final_psbt = BASE64_STANDARD.decode(finalized["psbt"].as_str().unwrap())?;
    let resp = publish(&mut wallet, committed, final_psbt).await?;
    let mut txid = resp.transfer.ok_or("no transfer")?.anchor_tx_hash;
    txid.reverse();
    println!("BROADCAST: {}", hex::encode(txid));
    println!(
        "redeemed {} µUSD for {} sats — watch for confirmation.",
        amount_micro_usd, v.user_sats
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    let env = NodeEnv::from_env(root());
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    match args.get(1).map(String::as_str) {
        Some("balance") => balance(&env).await,
        Some("quote") => {
            let lp = args
                .get(2)
                .ok_or("usage: satusd quote <lp-url> <amount-usd>")?;
            let amount = parse_usd(args.get(3).ok_or("missing amount")?)?;
            let payout = env.bcli(&["getnewaddress"]).as_str().unwrap().to_string();
            let oracle = flag("--oracle").unwrap_or_else(|| DEFAULT_ORACLE.into());
            fetch_verified_quote(lp, amount, &payout, &oracle)?;
            println!("(quote only — nothing signed, nothing spent)");
            Ok(())
        }
        Some("redeem") => {
            let lp = args
                .get(2)
                .ok_or("usage: satusd redeem <lp-url> <amount-usd>")?
                .clone();
            let amount = parse_usd(args.get(3).ok_or("missing amount")?)?;
            let oracle = flag("--oracle").unwrap_or_else(|| DEFAULT_ORACLE.into());
            redeem(&env, &lp, amount, flag("--payout"), &oracle).await
        }
        _ => {
            eprintln!("usage: satusd balance | quote <lp> <usd> | redeem <lp> <usd> [--payout <addr>] [--oracle <addr>]");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_usd;

    #[test]
    fn usd_parsing() {
        assert_eq!(parse_usd("0.90").unwrap(), 900_000);
        assert_eq!(parse_usd("12.34").unwrap(), 12_340_000);
        assert_eq!(parse_usd("1").unwrap(), 1_000_000);
        assert_eq!(parse_usd("0.000001").unwrap(), 1);
        assert_eq!(parse_usd(".5").unwrap(), 500_000);
        assert!(parse_usd("0.0000001").is_err());
    }
}
