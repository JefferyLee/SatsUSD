//! The founder-run Rail-0 LP service (MVP; PRD §8 scaffolding).
//!
//! Quote transport is deliberately unstandardized (ADR-0002) — this
//! HTTP/JSON surface is *a* transport, not *the* transport. The
//! normative objects are the spec-02 Quote payload and the Rail-0
//! SwapPlan; everything here either carries or verifies them.
//!
//! ```text
//! GET  /v0/manifest        -> rail manifest + rail_id + lp pubkey
//! POST /v0/quote           -> {amount_micro_usd, payout_address}
//!                             => signed Quote + the LP-side facts the
//!                                user needs to build the anchor template
//! POST /v0/settle          -> {signing_payload, anchor_psbt (b64)}
//!                             => LP-signed PSBT (user finalizes+publishes)
//! ```
//!
//! Trust posture (signet MVP, stated honestly): the LP verifies the
//! BTC level of the PSBT it signs (its input, exact payout/change
//! outputs, anchor dust slots) but does not yet independently verify
//! the asset level of the user's vPSBT before signing — asset
//! receipt is confirmed by tapd post-broadcast. Hardening this
//! (pre-signature vPSBT verification) is tracked for the protocol
//! writeup before mainnet.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::psbt::Psbt;
use bitcoin::{Address, Amount, OutPoint, TxOut};
use satusd_oracle::schnorr::sign_with_nonce;
use satusd_rail::encode::tagged_hash;
use satusd_rail::manifest::RailManifest;
use satusd_rail::quote::Quote;
use satusd_rail0::builder::ANCHOR_DUST_SATS;
use satusd_rail0::plan::SwapPlan;
use satusd_tapd_client::env::NodeEnv;
use satusd_tapd_client::{taprpc, TapChannel, TaprootAssetsClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

pub const LP_NONCE_TAG: &str = "SatUSD/lp/quote-nonce/v1";

pub struct LpConfig {
    pub env: NodeEnv,
    /// BIP-340 key signing quotes (seed-derived, founder scaffold).
    pub lp_sk: [u8; 32],
    pub lp_pubkey: [u8; 32],
    pub asset_id: [u8; 32],
    pub manifest: RailManifest,
    /// Base URL of the public oracle, e.g. "207.148.98.132:9590".
    pub oracle_addr: String,
    /// BTC fee carved out of the LP input (template is fee-exact).
    pub fee_budget_sats: u64,
}

struct QuoteRecord {
    lp_outpoint: OutPoint,
    lp_change: Option<TxOut>,
    user_payout: TxOut,
    expiry_unix_s: u64,
    consumed: bool,
}

pub struct Lp {
    pub cfg: LpConfig,
    tapd: Mutex<TaprootAssetsClient<TapChannel>>,
    quotes: Mutex<HashMap<[u8; 32], QuoteRecord>>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn err(msg: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "error": msg.into() })
}

impl Lp {
    pub async fn new(cfg: LpConfig) -> Result<Arc<Self>, Box<dyn std::error::Error>> {
        let channel = cfg.env.tapd_channel().await?;
        Ok(Arc::new(Self {
            cfg,
            tapd: Mutex::new(TaprootAssetsClient::new(channel)),
            quotes: Mutex::new(HashMap::new()),
        }))
    }

    /// Latest oracle price in whole USD, decoded from the public
    /// attestation TLV — the LP eats the protocol's own dog food.
    pub fn oracle_price_usd(&self) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let latest = http_get(&self.cfg.oracle_addr, "/v0/latest")?;
        let att_hex = http_get(&self.cfg.oracle_addr, &format!("/v0/attestation/{latest}"))?;
        let tlv = hex::decode(att_hex.trim()).map_err(|_| "attestation not hex")?;
        let att = satusd_oracle::tlv::parse_attestation(&tlv)
            .map_err(|e| format!("attestation parse: {e:?}"))?;
        let digits: Vec<u8> = att
            .outcomes
            .iter()
            .map(|o| if o == "1" { 1u8 } else { 0 })
            .collect();
        Ok(satusd_oracle::event::price_from_digits(&digits))
    }

    pub async fn handle_quote(&self, body: &serde_json::Value) -> serde_json::Value {
        let Some(amount) = body["amount_micro_usd"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
        else {
            return err("amount_micro_usd (decimal string) required");
        };
        let Some(payout_addr) = body["payout_address"].as_str() else {
            return err("payout_address required");
        };
        let network = if self.cfg.env.chain == "signet" {
            bitcoin::Network::Signet
        } else {
            bitcoin::Network::Regtest
        };
        let payout_script = match Address::from_str(payout_addr)
            .map_err(|e| e.to_string())
            .and_then(|a| a.require_network(network).map_err(|e| e.to_string()))
        {
            Ok(a) => a.script_pubkey(),
            Err(e) => return err(format!("bad payout_address: {e}")),
        };

        let price_usd = match self.oracle_price_usd() {
            Ok(p) if p > 0 => p,
            Ok(_) => return err("oracle price is zero"),
            Err(e) => return err(format!("oracle unavailable: {e}")),
        };
        let price_msat_per_usd = 100_000_000_000u64 / u64::from(price_usd);

        let mut quote = Quote {
            rail_id: self.cfg.manifest.rail_id(),
            price_msat_per_usd,
            amount_micro_usd: amount,
            expiry_unix_s: now_unix() + u64::from(self.cfg.manifest.quote_validity_s),
            lock_template_commitment: [0; 32],
        };
        let plan = match SwapPlan::from_quote(&self.cfg.manifest, &quote) {
            Ok(p) => p,
            Err(e) => return err(format!("{e:?}")),
        };
        quote.lock_template_commitment = plan.lock_template_commitment();
        let payload = quote.signing_payload();
        let nonce = tagged_hash(LP_NONCE_TAG, &{
            let mut b = self.cfg.lp_sk.to_vec();
            b.extend_from_slice(&payload);
            b
        });
        let Ok(sig) = sign_with_nonce(&self.cfg.lp_sk, &nonce, &payload) else {
            return err("lp signing failed");
        };

        // --- BTC input selection: lock a UTXO for this quote ---
        let need = plan.user_sats + self.cfg.fee_budget_sats + 1_000;
        let unspent = self.cfg.env.bcli(&["listunspent", "1"]);
        let Some(u) = unspent.as_array().and_then(|a| {
            a.iter().find(|u| {
                (u["amount"].as_f64().unwrap_or(0.0) * 1e8) as u64 >= need
                    && u["spendable"].as_bool().unwrap_or(false)
            })
        }) else {
            return err("lp has no UTXO large enough");
        };
        let txid = u["txid"].as_str().unwrap().to_string();
        let vout = u["vout"].as_u64().unwrap() as u32;
        let value_sats = (u["amount"].as_f64().unwrap() * 1e8).round() as u64;
        let script_hex = u["scriptPubKey"].as_str().unwrap().to_string();
        // The user co-builds an anchor tx that spends this (foreign-to-them)
        // P2TR input; their tapd's fee estimation needs the input's taproot
        // internal key + BIP-32 origin to size it without owning it. Read
        // them from the address descriptor (BIP86 `tr([fp/path]key)#cksum`).
        let lp_addr = u["address"].as_str().unwrap_or("");
        let ai = self.cfg.env.bcli(&["getaddressinfo", lp_addr]);
        let (lp_internal_key, lp_derivation) = ai["desc"]
            .as_str()
            .and_then(parse_tr_descriptor)
            .unwrap_or_default();
        self.cfg.env.bcli(&[
            "lockunspent",
            "false",
            &format!("[{{\"txid\":\"{txid}\",\"vout\":{vout}}}]"),
        ]);
        let lp_outpoint = OutPoint {
            txid: txid.parse().unwrap(),
            vout,
        };

        // Change: exact-fee template (CommitVirtualPsbts gets the
        // anchor dust from the input side; our budget covers vbytes).
        let change_sats = value_sats - plan.user_sats - self.cfg.fee_budget_sats;
        let lp_change = if change_sats >= 546 {
            let addr = self.cfg.env.bcli(&["getrawchangeaddress"]);
            let script = Address::from_str(addr.as_str().unwrap())
                .unwrap()
                .require_network(network)
                .unwrap()
                .script_pubkey();
            Some(TxOut {
                value: Amount::from_sat(change_sats),
                script_pubkey: script,
            })
        } else {
            None
        };

        // --- TA receive address for exactly the quoted units ---
        let lp_ta_addr = {
            let mut tapd = self.tapd.lock().await;
            match tapd
                .new_addr(taprpc::NewAddrRequest {
                    asset_id: self.cfg.asset_id.to_vec(),
                    amt: amount,
                    ..Default::default()
                })
                .await
            {
                Ok(r) => r.into_inner().encoded,
                Err(e) => return err(format!("tapd NewAddr: {e}")),
            }
        };

        let user_payout = TxOut {
            value: Amount::from_sat(plan.user_sats),
            script_pubkey: payout_script,
        };
        let response = serde_json::json!({
            "quote": {
                "rail_id": hex::encode(quote.rail_id),
                "price_msat_per_usd": quote.price_msat_per_usd.to_string(),
                "amount_micro_usd": quote.amount_micro_usd.to_string(),
                "expiry_unix_s": quote.expiry_unix_s.to_string(),
                "lock_template_commitment": hex::encode(quote.lock_template_commitment),
            },
            "signing_payload": hex::encode(payload),
            "lp_sig": hex::encode(sig),
            "lp_pubkey": hex::encode(self.cfg.lp_pubkey),
            "lp_ta_addr": lp_ta_addr,
            "plan": {
                "gross_sats": plan.gross_sats.to_string(),
                "service_fee_sats": plan.service_fee_sats.to_string(),
                "user_sats": plan.user_sats.to_string(),
                "retained_sats": plan.retained_sats.to_string(),
            },
            "lp_btc_input": {
                "outpoint": format!("{lp_outpoint}"),
                "value_sats": value_sats.to_string(),
                "script_pubkey": script_hex,
                "internal_key": lp_internal_key,
                "derivation": lp_derivation,
            },
            "lp_change": lp_change.as_ref().map(|c| serde_json::json!({
                "value_sats": c.value.to_sat().to_string(),
                "script_pubkey": hex::encode(c.script_pubkey.as_bytes()),
            })),
            "fee_budget_sats": self.cfg.fee_budget_sats.to_string(),
        });

        self.quotes.lock().await.insert(
            payload,
            QuoteRecord {
                lp_outpoint,
                lp_change,
                user_payout,
                expiry_unix_s: quote.expiry_unix_s,
                consumed: false,
            },
        );
        response
    }

    pub async fn handle_settle(&self, body: &serde_json::Value) -> serde_json::Value {
        let Some(payload_hex) = body["signing_payload"].as_str() else {
            return err("signing_payload required");
        };
        let Ok(payload) = hex::decode(payload_hex) else {
            return err("signing_payload not hex");
        };
        let Ok(payload): Result<[u8; 32], _> = payload.try_into() else {
            return err("signing_payload must be 32 bytes");
        };
        let Some(psbt_b64) = body["anchor_psbt"].as_str() else {
            return err("anchor_psbt (base64) required");
        };
        use base64::prelude::*;
        let Ok(psbt_bytes) = BASE64_STANDARD.decode(psbt_b64) else {
            return err("anchor_psbt not base64");
        };
        let Ok(psbt) = Psbt::deserialize(&psbt_bytes) else {
            return err("anchor_psbt does not parse");
        };

        let mut quotes = self.quotes.lock().await;
        let Some(rec) = quotes.get_mut(&payload) else {
            return err("unknown quote");
        };
        if rec.consumed {
            return err("quote already settled");
        }
        if now_unix() > rec.expiry_unix_s {
            return err("quote expired");
        }

        // --- BTC-level verification before the LP signs anything ---
        let tx = &psbt.unsigned_tx;
        if !tx
            .input
            .iter()
            .any(|i| i.previous_output == rec.lp_outpoint)
        {
            return err("psbt does not spend the quoted LP input");
        }
        if tx.input.len() != 2 {
            return err("expected exactly 2 inputs (TA anchor + LP)");
        }
        let mut payout_seen = 0;
        let mut change_seen = 0;
        let mut anchor_slots = 0;
        let mut extra_value = 0u64;
        for out in &tx.output {
            if out == &rec.user_payout {
                payout_seen += 1;
            } else if Some(out) == rec.lp_change.as_ref() {
                change_seen += 1;
            } else if out.value.to_sat() == ANCHOR_DUST_SATS {
                anchor_slots += 1;
            } else {
                // tapd's own fee change (the surplus of our fee
                // budget) — tolerated, bounded by what the LP
                // already conceded.
                extra_value += out.value.to_sat();
            }
        }
        if extra_value > self.cfg.fee_budget_sats {
            return err(format!(
                "unaccounted outputs exceed the fee budget: {extra_value} sats"
            ));
        }
        if payout_seen != 1 || change_seen != usize::from(rec.lp_change.is_some()) {
            return err("payout/change outputs do not match the quote");
        }
        if anchor_slots == 0 || anchor_slots > 2 {
            return err("unexpected anchor slot count");
        }

        // Sign only our input via the node wallet.
        let signed = self.cfg.env.bcli(&["walletprocesspsbt", psbt_b64, "true"]);
        let Some(signed_psbt) = signed["psbt"].as_str() else {
            return err("walletprocesspsbt gave no psbt");
        };
        rec.consumed = true;
        serde_json::json!({ "signed_psbt": signed_psbt })
    }

    pub fn manifest_json(&self) -> serde_json::Value {
        let m = &self.cfg.manifest;
        serde_json::json!({
            "manifest": satusd_rail::manifest_json::to_value(m),
            "rail_id": hex::encode(m.rail_id()),
            "asset_id": hex::encode(self.cfg.asset_id),
            "lp_pubkey": hex::encode(self.cfg.lp_pubkey),
            "direction": "redeem",
            "settle_primitive": "atomic_swap",
            "max_size_sats": m.max_size_sats.to_string(),
            "fee_schedule": {
                "retain_bps": m.fee_schedule.retain_bps,
                "service_bps": m.fee_schedule.service_bps,
                "fixed_sats": m.fee_schedule.fixed_sats.to_string(),
            },
            "quote_validity_s": m.quote_validity_s,
            "oracle": { "class": "none", "price_source": format!("http://{}", self.cfg.oracle_addr) },
            "chain": self.cfg.env.chain,
        })
    }
}

/// Parse a BIP86 single-key taproot descriptor
/// `tr([<fp>/<path>]<key>)#<cksum>` into `(internal_key_xonly_hex,
/// "<fp>/<path>")`. Returns the x-only (32-byte) internal key hex and
/// the origin string the user's PSBT builder needs. `None` if the
/// descriptor isn't a plain `tr(...)` with a key-origin.
fn parse_tr_descriptor(desc: &str) -> Option<(String, String)> {
    let inner = desc.strip_prefix("tr(")?;
    let inner = inner.split(')').next()?; // drop ")#checksum"
    let rest = inner.strip_prefix('[')?;
    let (origin, key) = rest.split_once(']')?;
    // origin = "<fingerprint>/<path...>"; keep as-is for the builder.
    if origin.split('/').next()?.len() != 8 {
        return None; // not a fingerprint
    }
    let key = key.trim();
    let xonly = match key.len() {
        64 => key.to_string(),
        66 => key[2..].to_string(), // compressed → drop the parity byte
        _ => return None,
    };
    Some((xonly, origin.to_string()))
}

// ---- HTTP plumbing (std-grade, founder-LP scale) ----

pub async fn serve(lp: Arc<Lp>, listener: TcpListener) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let lp = lp.clone();
        tokio::spawn(async move {
            let _ = handle_conn(lp, stream).await;
        });
    }
}

async fn handle_conn(lp: Arc<Lp>, mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(8192);
    let mut tmp = [0u8; 4096];
    let (head_end, _) = loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break (pos + 4, n);
        }
        if buf.len() > 1 << 20 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let content_length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    while buf.len() < head_end + content_length {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > (1 << 20) + head_end {
            return Ok(());
        }
    }
    let body: serde_json::Value =
        serde_json::from_slice(&buf[head_end..head_end + content_length.min(buf.len() - head_end)])
            .unwrap_or(serde_json::Value::Null);

    let mut parts = request_line.split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (status, response) = match (method, path) {
        ("GET", "/v0/manifest") => (200, lp.manifest_json()),
        ("POST", "/v0/quote") => (200, lp.handle_quote(&body).await),
        ("POST", "/v0/settle") => (200, lp.handle_settle(&body).await),
        _ => (404, err("not found")),
    };
    let status = if response.get("error").is_some() && status == 200 {
        400
    } else {
        status
    };
    let body = response.to_string();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
}

/// Minimal blocking HTTP POST with a JSON body; returns parsed JSON.
pub fn http_post(
    addr: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::{Read, Write};
    let addr = addr.trim_start_matches("http://");
    let payload = body.to_string();
    let mut s = std::net::TcpStream::connect(addr)?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    s.write_all(request.as_bytes())?;
    let mut response = String::new();
    s.read_to_string(&mut response)?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or("malformed http response")?;
    let parsed: serde_json::Value = serde_json::from_str(body)?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("POST {path}: {}", parsed["error"].as_str().unwrap_or("?")).into());
    }
    Ok(parsed)
}

/// Minimal blocking HTTP GET returning the body (oracle endpoints).
pub fn http_get(
    addr: &str,
    path: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use std::io::{Read, Write};
    let addr = addr.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr)?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(request.as_bytes())?;
    let mut response = String::new();
    s.read_to_string(&mut response)?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or("malformed http response")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("GET {path}: {}", head.lines().next().unwrap_or("")).into());
    }
    Ok(body.to_string())
}
