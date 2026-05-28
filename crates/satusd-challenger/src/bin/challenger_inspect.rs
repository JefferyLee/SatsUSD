//! One-shot challenger inspector (M5 detection): given a claim's committed
//! `live_da_root`, fetch the bundle from a DA mirror and run `inspect` +
//! `recompute_lineage` on the lineage section. Wrap in `watch`/cron for a daemon.
//! Exits non-zero on any alert.
//!
//!   challenger_inspect <mirror_url> <committed_live_da_root_hex> [<claim_id_hex>]
//!
//! e.g.  challenger_inspect http://127.0.0.1:8089 ab... f3...

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;

use satusd_challenger::{inspect, recompute_lineage, Alert, LINEAGE_SECTION};
use satusd_da::LiveDABundle;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} <mirror_url> <committed_root_hex> [claim_id_hex]",
            args[0]
        );
        return ExitCode::from(2);
    }
    let mirror = parse_host_port(&args[1]);
    let root_hex = &args[2];
    let claim_id = args.get(3).map(|s| s.as_str()).unwrap_or("<unspecified>");
    let Ok(committed) = hex32(root_hex) else {
        eprintln!("bad committed root hex");
        return ExitCode::from(2);
    };

    // -- Fetch the bundle from the mirror --
    let path = format!("/v1/da/live/{root_hex}");
    let body = match http_get(&mirror, &path) {
        Ok(b) => b,
        Err(e) => {
            // No bundle uploaded → DA-01 (run inspect with None to surface that).
            eprintln!("[challenger] mirror fetch failed ({e}); treating as no bundle (DA-01)");
            print_verdict(&inspect(committed, None).alerts, claim_id);
            return ExitCode::from(1);
        }
    };

    // -- Inspect the bundle (DA-01/02/03/06) --
    let Ok((bundle, _declared)) = LiveDABundle::decode(&body) else {
        eprintln!("[challenger] bundle does not decode");
        return ExitCode::from(1);
    };
    let verdict = inspect(committed, Some(&bundle));
    let mut alerts = verdict.alerts;

    // -- Re-run lineage on the lineage section, if present (§14.12 acceptance) --
    if let Some(lineage) = bundle.section(LINEAGE_SECTION) {
        if let Err(a) = recompute_lineage(lineage) {
            alerts.push(a);
        }
    }

    print_verdict(&alerts, claim_id);
    if alerts.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_verdict(alerts: &[Alert], claim_id: &str) {
    if alerts.is_empty() {
        println!("[challenger] claim_id={claim_id}: clean ✓");
    } else {
        eprintln!(
            "[challenger] claim_id={claim_id}: {} alert(s)",
            alerts.len()
        );
        for a in alerts {
            eprintln!("  ! {a:?}");
        }
    }
}

fn parse_host_port(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

fn hex32(s: &str) -> Result<[u8; 32], &'static str> {
    let v = hex::decode(s).map_err(|_| "bad hex")?;
    v.try_into().map_err(|_| "want 32 bytes")
}

fn http_get(host: &str, path: &str) -> Result<Vec<u8>, String> {
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
    Ok(raw[split + 4..].to_vec())
}
