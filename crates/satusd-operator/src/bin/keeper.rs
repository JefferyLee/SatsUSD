//! One-shot keeper (M6/RB-01): fetch the state-node's pending claims; for each
//! `Pending` claim whose `claim_expiry_height` is in the past, POST a
//! `reclaim_stale_claim` transition to free the reservation. Wrap in `watch`/cron
//! for a daemon. Exits non-zero on any reclaim error.
//!
//!   keeper <state_node_url>
//!
//! e.g.  keeper http://127.0.0.1:8088

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;

use satusd_operator::{submit_transition, SubmitError};
use serde_json::{json, Value};

const STATUS_PENDING: u64 = 0;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <state_node_url>", args[0]);
        return ExitCode::from(2);
    }
    let host = parse_host_port(&args[1]);

    // -- Current height from /v1/state --
    let state: Value = match http_get_json(&host, "/v1/state") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[keeper] /v1/state fetch failed: {e}");
            return ExitCode::from(1);
        }
    };
    let current_height = state["l1_anchor_height"].as_u64().unwrap_or(0) as u32;

    // -- Pending claims --
    let claims_doc: Value = match http_get_json(&host, "/v1/pending_claims") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[keeper] /v1/pending_claims fetch failed: {e}");
            return ExitCode::from(1);
        }
    };
    let claims = claims_doc["pending_claims"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    println!(
        "[keeper] current_height={current_height}, pending_claims={}",
        claims.len()
    );

    let mut reclaimed = 0usize;
    let mut errors = 0usize;
    for c in claims {
        let status = c["status"].as_u64().unwrap_or(u64::MAX);
        if status != STATUS_PENDING {
            continue;
        }
        let expiry = c["claim_expiry_height"].as_u64().unwrap_or(u64::MAX) as u32;
        if current_height <= expiry {
            continue; // not yet stale
        }
        let claim_id = c["claim_id"].as_str().unwrap_or("").to_string();
        let body = json!({
            "transition": "reclaim_stale_claim",
            "claim_id": claim_id,
            "current_height": current_height,
        });
        match submit_transition(&host, &body) {
            Ok(_) => {
                println!("[keeper] reclaimed {claim_id} (expired at {expiry})");
                reclaimed += 1;
            }
            Err(SubmitError::Rejected { status, error }) => {
                eprintln!("[keeper] reclaim {claim_id} rejected ({status}): {error}");
                errors += 1;
            }
            Err(e) => {
                eprintln!("[keeper] reclaim {claim_id} error: {e:?}");
                errors += 1;
            }
        }
    }
    println!("[keeper] reclaimed={reclaimed}, errors={errors}");
    if errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn parse_host_port(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

fn http_get_json(host: &str, path: &str) -> Result<Value, String> {
    let mut s = TcpStream::connect(host).map_err(|e| format!("connect: {e}"))?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no header terminator")?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|e| e.to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("no status")?;
    if status != 200 {
        return Err(format!("status {status}"));
    }
    serde_json::from_slice(&raw[split + 4..]).map_err(|e| e.to_string())
}
