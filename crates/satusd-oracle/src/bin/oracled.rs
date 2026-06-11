//! Minimal oracle daemon: announce ahead, attest at maturity, write
//! TLV hex files into a directory. Devnet-grade — the public HTTP
//! surface is M-B infrastructure (PRD §7).
//!
//! ```text
//! oracled <seed-hex-32-bytes> <out-dir> [price_usd=100000] [cadence_s=1]
//! ```
//!
//! PriceSource is pluggable; this binary ships the fixed source
//! only. Live exchange aggregation per spec 03 §5.1 methodology is
//! the marker provider's job, not this tick oracle's v0.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use satusd_oracle::oracle::Oracle;

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
        .ok_or("usage: oracled <seed-hex> <out-dir> [price] [cadence_s]")?;
    let out_dir = args.get(2).ok_or("missing out-dir")?;
    let price: u32 = args
        .get(3)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(100_000);
    let cadence: u64 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(1);

    let mut seed = [0u8; 32];
    let bytes = hex_decode(seed_hex)?;
    if bytes.len() != 32 {
        return Err("seed must be 32 bytes of hex".into());
    }
    seed.copy_from_slice(&bytes);

    let oracle = Oracle::from_seed(&seed)?;
    let mut source = Fixed(price);
    std::fs::create_dir_all(out_dir)?;
    println!("oracled: pubkey={}", hex_encode(&oracle.pubkey));

    loop {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let tick = now - (now % cadence);

        // Announce two cadences ahead (spec 03 §3.2 lead time).
        let future = tick + 2 * cadence;
        let ann = oracle.announce(future)?;
        write_hex(out_dir, &format!("ann-{future}.hex"), &ann.tlv_bytes)?;

        // Attest the tick that just matured.
        let p = source.price_usd(tick);
        let att = oracle.attest(tick, p)?;
        write_hex(out_dir, &format!("att-{tick}.hex"), &att.tlv_bytes)?;
        println!("tick {tick}: price={p} att={}B", att.tlv_bytes.len());

        std::thread::sleep(Duration::from_secs(cadence));
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
