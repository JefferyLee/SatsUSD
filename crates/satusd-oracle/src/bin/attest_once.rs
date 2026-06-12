//! One-shot attestation: load a seed, sign one `(unix_ts, price)` and
//! write the `oracle_attestation` TLV hex to stdout (and optionally to a
//! file). The planter half of the FR-10 slash drill (spec 05 §4): run
//! twice with the same seed and ts but different prices to mint a
//! self-certifying equivocation pair.
//!
//! ```text
//! attest-once <seed-hex-32> <unix_ts> <price_usd> [out-file]
//! ```
//!
//! Prints the oracle pubkey on stderr so the operator can match the
//! signing identity without scraping the TLV.

use std::process::ExitCode;

use satusd_oracle::oracle::Oracle;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(seed_hex) = args.get(1) else {
        eprintln!("usage: {} <seed-hex-32> <unix_ts> <price_usd> [out-file]", args[0]);
        return ExitCode::from(2);
    };
    let Some(ts_arg) = args.get(2) else {
        eprintln!("missing unix_ts");
        return ExitCode::from(2);
    };
    let Some(price_arg) = args.get(3) else {
        eprintln!("missing price_usd");
        return ExitCode::from(2);
    };
    let out_file = args.get(4);

    let seed_bytes = match hex_decode(seed_hex) {
        Ok(b) if b.len() == 32 => b,
        _ => {
            eprintln!("seed must be 32 bytes of hex");
            return ExitCode::from(2);
        }
    };
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_bytes);

    let ts: u64 = match ts_arg.parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("bad unix_ts");
            return ExitCode::from(2);
        }
    };
    let price: u32 = match price_arg.parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("bad price_usd");
            return ExitCode::from(2);
        }
    };

    let oracle = match Oracle::from_seed(&seed) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oracle init: {e}");
            return ExitCode::from(1);
        }
    };
    let att = match oracle.attest(ts, price) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("attest: {e}");
            return ExitCode::from(1);
        }
    };

    let tlv_hex = hex_encode(&att.tlv_bytes);
    eprintln!("pubkey={}", hex_encode(&oracle.pubkey));
    eprintln!("event_id={} ts={ts} price={price}", att.event_id);
    if let Some(path) = out_file {
        if let Err(e) = std::fs::write(path, &tlv_hex) {
            eprintln!("write {path}: {e}");
            return ExitCode::from(1);
        }
        eprintln!("wrote {path} ({} bytes hex)", tlv_hex.len());
    }
    println!("{tlv_hex}");
    ExitCode::SUCCESS
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, &'static str> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length");
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|_| "bad hex"))
        .collect()
}
