//! Oracle daemon: announce ahead (spec 03 §3.2 lead time), attest at
//! maturity, write TLV hex files into a directory — and optionally
//! serve them over a minimal built-in HTTP surface so any
//! independent DLC client can consume announcements/attestations
//! (FR-5, M-B "publicly consumable").
//!
//! ```text
//! oracled <seed-hex-32-bytes> <out-dir> [price_usd=100000] [cadence_s=1] [listen_addr]
//! ```
//!
//! HTTP routes (transport is NOT part of the oracle standard — the
//! TLVs are; this is reference plumbing):
//!
//! ```text
//! GET /v0/pubkey                  -> x-only pubkey, hex
//! GET /v0/announcement/<unix_ts>  -> oracle_announcement TLV, hex
//! GET /v0/attestation/<unix_ts>   -> oracle_attestation TLV, hex
//! GET /v0/latest                  -> latest attested tick, decimal
//! ```
//!
//! The out-dir is the store: files are the daemon's only state, so
//! restarts are safe and the full attestation history stays on disk
//! (equivocation evidence per spec 03 §3.3 needs history).
//!
//! PriceSource is pluggable; this binary ships the fixed source
//! only. Live exchange aggregation per spec 03 §5.1 methodology is
//! the marker provider's job, not this tick oracle's v0.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use satusd_oracle::oracle::Oracle;

/// Announcement lead, in ticks: spec 03 §3.2 requires publication at
/// least `2 × cadence × 60` seconds ahead of maturity.
const LEAD_TICKS: u64 = 120;

trait PriceSource {
    fn price_usd(&mut self, unix_ts: u64) -> u32;
}

struct Fixed(u32);
impl PriceSource for Fixed {
    fn price_usd(&mut self, _ts: u64) -> u32 {
        self.0
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let seed_hex = args
        .get(1)
        .ok_or("usage: oracled <seed-hex> <out-dir> [price] [cadence_s] [listen_addr]")?;
    let out_dir = args.get(2).ok_or("missing out-dir")?.clone();
    let price: u32 = args
        .get(3)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(100_000);
    let cadence: u64 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(1);
    let listen_addr = args.get(5).cloned();

    let mut seed = [0u8; 32];
    let bytes = hex_decode(seed_hex)?;
    if bytes.len() != 32 {
        return Err("seed must be 32 bytes of hex".into());
    }
    seed.copy_from_slice(&bytes);

    let oracle = Oracle::from_seed(&seed)?;
    let mut source = Fixed(price);
    std::fs::create_dir_all(&out_dir)?;
    let pubkey_hex = hex_encode(&oracle.pubkey);
    println!("oracled: pubkey={pubkey_hex}");

    if let Some(addr) = listen_addr {
        let listener = TcpListener::bind(&addr)?;
        println!("oracled: serving http on {addr}");
        let dir = out_dir.clone();
        std::thread::spawn(move || serve(listener, dir, pubkey_hex));
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut next_announce = now - (now % cadence);
    let mut last_attested = next_announce.saturating_sub(cadence);

    loop {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let tick = now - (now % cadence);

        // Keep announcements published out to the spec lead time.
        let horizon = tick + LEAD_TICKS * cadence;
        while next_announce <= horizon {
            let ann = oracle.announce(next_announce)?;
            write_hex(
                &out_dir,
                &format!("ann-{next_announce}.hex"),
                &ann.tlv_bytes,
            )?;
            next_announce += cadence;
        }

        // Attest every tick that matured since the last loop (sleep
        // jitter must not leave gaps in the public history).
        while last_attested < tick {
            let t = last_attested + cadence;
            let p = source.price_usd(t);
            let att = oracle.attest(t, p)?;
            write_hex(&out_dir, &format!("att-{t}.hex"), &att.tlv_bytes)?;
            std::fs::write(Path::new(&out_dir).join("latest.txt"), t.to_string())?;
            println!("tick {t}: price={p} att={}B", att.tlv_bytes.len());
            last_attested = t;
        }

        std::thread::sleep(Duration::from_secs(cadence));
    }
}

// ---- HTTP surface ----

fn serve(listener: TcpListener, dir: String, pubkey_hex: String) {
    for stream in listener.incoming().flatten() {
        let dir = dir.clone();
        let pk = pubkey_hex.clone();
        std::thread::spawn(move || {
            let _ = handle_conn(stream, &dir, &pk);
        });
    }
}

fn handle_conn(mut stream: TcpStream, dir: &str, pubkey_hex: &str) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| {
            let mut parts = line.split(' ');
            match (parts.next(), parts.next()) {
                (Some("GET"), Some(p)) => Some(p.to_string()),
                _ => None,
            }
        })
        .unwrap_or_default();
    let (status, body) = respond(dir, pubkey_hex, &path);
    let reason = if status == 200 { "OK" } else { "Not Found" };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Route a GET path to (status, body). Timestamps are parsed as
/// strict u64 — client input never reaches the filesystem raw.
fn respond(dir: &str, pubkey_hex: &str, path: &str) -> (u16, String) {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        ["v0", "pubkey"] => (200, pubkey_hex.to_string()),
        ["v0", "latest"] => read_file(dir, "latest.txt"),
        ["v0", "announcement", ts] => match ts.parse::<u64>() {
            Ok(t) => read_file(dir, &format!("ann-{t}.hex")),
            Err(_) => (404, "bad timestamp".into()),
        },
        ["v0", "attestation", ts] => match ts.parse::<u64>() {
            Ok(t) => read_file(dir, &format!("att-{t}.hex")),
            Err(_) => (404, "bad timestamp".into()),
        },
        _ => (404, "not found".into()),
    }
}

fn read_file(dir: &str, name: &str) -> (u16, String) {
    match std::fs::read_to_string(Path::new(dir).join(name)) {
        Ok(s) => (200, s),
        Err(_) => (404, "not found".into()),
    }
}

fn write_hex(dir: &str, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(Path::new(dir).join(name), hex_encode(bytes))
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(Into::into))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("oracled-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn routes() {
        let dir = tmp_dir("routes");
        write_hex(&dir, "ann-1700000000.hex", &[0xab, 0xcd]).unwrap();
        write_hex(&dir, "att-1700000000.hex", &[0x01]).unwrap();
        std::fs::write(Path::new(&dir).join("latest.txt"), "1700000000").unwrap();

        assert_eq!(respond(&dir, "aa", "/v0/pubkey"), (200, "aa".into()));
        assert_eq!(
            respond(&dir, "aa", "/v0/latest"),
            (200, "1700000000".into())
        );
        assert_eq!(
            respond(&dir, "aa", "/v0/announcement/1700000000"),
            (200, "abcd".into())
        );
        assert_eq!(
            respond(&dir, "aa", "/v0/attestation/1700000000"),
            (200, "01".into())
        );
        // Misses and malice.
        assert_eq!(respond(&dir, "aa", "/v0/announcement/1").0, 404);
        assert_eq!(respond(&dir, "aa", "/v0/attestation/abc").0, 404);
        assert_eq!(respond(&dir, "aa", "/v0/announcement/../latest.txt").0, 404);
        assert_eq!(respond(&dir, "aa", "/nope").0, 404);
        assert_eq!(respond(&dir, "aa", "/").0, 404);
    }

    /// End-to-end over real TCP: an "independent client" (raw
    /// socket, no shared code path with the server) fetches an
    /// attestation and verifies the daemon serves the exact TLV hex.
    #[test]
    fn http_roundtrip() {
        let dir = tmp_dir("http");
        let oracle = Oracle::from_seed(&[5u8; 32]).unwrap();
        let att = oracle.attest(1_700_000_777, 64_321).unwrap();
        write_hex(&dir, "att-1700000777.hex", &att.tlv_bytes).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let pk = hex_encode(&oracle.pubkey);
        std::thread::spawn(move || serve(listener, dir, pk));

        let mut stream = TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "GET /v0/attestation/1700000777 HTTP/1.1\r\nHost: x\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        let body = response.split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(body, hex_encode(&att.tlv_bytes));
    }
}
