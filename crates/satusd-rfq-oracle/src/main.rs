//! SatUSD RFQ price oracle — serves litd's `priceoraclerpc.PriceOracle`
//! interface backed by the SatUSD oracle (3-venue median) at
//! `:9590`. An LN edge node points its
//! `taproot-assets.experimental.rfq.priceoracleaddress` here so its
//! RFQ quotes track SatUSD's own oracle price (fork-① decision: "RFQ
//! uses oracle"), with an optional edge spread.
//!
//! ```text
//! satusd-rfq-oracle <listen_addr> [oracle_addr] [spread_bps]
//!   listen_addr  e.g. 127.0.0.1:8095   (rfqrpc://<this> in litd)
//!   oracle_addr  default 207.148.98.132:9590
//!   spread_bps   default 0 (edge spread per direction, basis points)
//! ```
//!
//! SatUSD has decimal_display 6: $1 = 1,000,000 atoms. The "subject
//! asset units per BTC" rate litd wants is therefore
//! `price_usd × 1_000_000`. Payment asset is BTC, whose rate is the
//! proto-fixed 100 billion (msat per BTC).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use tonic::{transport::Server, Request, Response, Status};

pub mod pb {
    tonic::include_proto!("priceoraclerpc");
}

use pb::price_oracle_server::{PriceOracle, PriceOracleServer};
use pb::{
    query_asset_rates_response, AssetRates, FixedPoint, QueryAssetRatesOkResponse,
    QueryAssetRatesRequest, QueryAssetRatesResponse, TransactionType,
};

/// SatUSD atoms per US dollar (decimal_display 6).
const ATOMS_PER_USD: u64 = 1_000_000;
/// Milli-satoshi per BTC — the proto's fixed payment-asset (BTC) rate.
const MSAT_PER_BTC: &str = "100000000000";
/// How long a returned rate is valid.
const RATE_TTL_SECS: u64 = 60;

struct SatusdOracle {
    oracle_addr: String,
    spread_bps: u64,
}

impl SatusdOracle {
    /// Latest BTC/USD from the SatUSD oracle, decoded from its signed
    /// attestation TLV (same path the LP and SDK use).
    fn price_usd(&self) -> Result<u32, String> {
        let latest = http_get(&self.oracle_addr, "/v0/latest")?;
        let ts = latest.trim();
        let att_hex = http_get(&self.oracle_addr, &format!("/v0/attestation/{ts}"))?;
        let tlv = hex::decode(att_hex.trim()).map_err(|_| "attestation not hex".to_string())?;
        let att = satusd_oracle::tlv::parse_attestation(&tlv)
            .map_err(|e| format!("attestation parse: {e:?}"))?;
        let digits: Vec<u8> = att
            .outcomes
            .iter()
            .map(|o| if o == "1" { 1u8 } else { 0 })
            .collect();
        let price = satusd_oracle::event::price_from_digits(&digits);
        if price == 0 {
            return Err("oracle price is zero".into());
        }
        Ok(price)
    }
}

#[tonic::async_trait]
impl PriceOracle for SatusdOracle {
    async fn query_asset_rates(
        &self,
        req: Request<QueryAssetRatesRequest>,
    ) -> Result<Response<QueryAssetRatesResponse>, Status> {
        let r = req.into_inner();
        let price = self
            .price_usd()
            .map_err(|e| Status::unavailable(format!("satusd oracle: {e}")))?;

        // Units of SatUSD per BTC at the oracle's fair price.
        let base = u64::from(price) * ATOMS_PER_USD;

        // Edge spread: a PURCHASE (the edge buys SatUSD so a user can
        // pay a BTC invoice with it) quotes MORE units per BTC — the
        // payer gives up a touch more asset; a SALE quotes fewer. The
        // oracle sets the fair price; the spread is the edge's margin.
        let delta = base * self.spread_bps / 10_000;
        let tx_type =
            TransactionType::try_from(r.transaction_type).unwrap_or(TransactionType::Purchase);
        let units_per_btc = match tx_type {
            TransactionType::Sale => base.saturating_sub(delta),
            _ => base + delta,
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let rates = AssetRates {
            subject_asset_rate: Some(FixedPoint {
                coefficient: units_per_btc.to_string(),
                scale: 0,
            }),
            payment_asset_rate: Some(FixedPoint {
                coefficient: MSAT_PER_BTC.to_string(),
                scale: 0,
            }),
            expiry_timestamp: now + RATE_TTL_SECS,
        };
        Ok(Response::new(QueryAssetRatesResponse {
            result: Some(query_asset_rates_response::Result::Ok(
                QueryAssetRatesOkResponse {
                    asset_rates: Some(rates),
                },
            )),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let listen = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:8095".to_string());
    let oracle_addr = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "207.148.98.132:9590".to_string());
    let spread_bps: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

    let svc = SatusdOracle {
        oracle_addr: oracle_addr.clone(),
        spread_bps,
    };
    // Fail fast if the oracle is unreachable at boot (clearer than a
    // first-quote failure later).
    match svc.price_usd() {
        Ok(p) => println!("satusd-rfq-oracle: oracle {oracle_addr} ok, BTC/USD={p}, spread={spread_bps}bps"),
        Err(e) => eprintln!("satusd-rfq-oracle: WARNING oracle {oracle_addr} unreachable at boot: {e}"),
    }

    let addr = listen.parse()?;
    println!("satusd-rfq-oracle: serving priceoraclerpc on {listen}");
    Server::builder()
        .add_service(PriceOracleServer::new(svc))
        .serve(addr)
        .await?;
    Ok(())
}

// Minimal blocking HTTP GET (the oracle endpoints are tiny text bodies).
fn http_get(addr: &str, path: &str) -> Result<String, String> {
    let addr = addr.trim_start_matches("http://");
    let mut s = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let (head, body) = raw.split_once("\r\n\r\n").ok_or("malformed http response")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("GET {path}: {}", head.lines().next().unwrap_or("")));
    }
    Ok(body.to_string())
}
