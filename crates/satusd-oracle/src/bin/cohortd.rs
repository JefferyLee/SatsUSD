//! `cohortd` — one participant of a distributed FROST attestation cohort
//! (spec 03 §5.7, FR-5). Transport is a shared directory (see
//! [`satusd_oracle::cohortd`]); every node runs the same binary with its own
//! index + seed. The aggregated `ann-/att-` TLVs are byte-identical to a
//! single `oracled`, so any DLC client consumes the cohort as one key.
//!
//! ```text
//! cohortd round1   <i> <n> <t> <seed-hex32> <dir>
//! cohortd finalize <i> <n> <t> <dir>                       # -> prints group pubkey
//! cohortd announce <i> <seed-hex32> <dir> <ts>
//! cohortd attest   <i> <seed-hex32> <dir> <ts> <price> <quorum-csv>
//! cohortd aggregate    <dir> <ts> <price> <quorum-csv> <group-pubkey-hex32>
//! ```
//!
//! A full run: every participant `round1`, then every participant `finalize`;
//! per event every quorum member `announce` then `attest`, then any node
//! `aggregate`.

use std::path::Path;

use satusd_oracle::cohortd;
use satusd_oracle::frost::FrostError;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let cmd = a.get(1).map(String::as_str).unwrap_or("");
    let err = |e: FrostError| format!("{e:?}");

    match cmd {
        "round1" => {
            let (i, n, t) = (u16p(&a, 2)?, u16p(&a, 3)?, u16p(&a, 4)?);
            let seed = cohortd::parse_hex32(arg(&a, 5)?).map_err(err)?;
            cohortd::dkg_round1(Path::new(arg(&a, 6)?), i, n, t, &seed).map_err(err)?;
            println!("round1: participant {i} published commitment + {n} shares");
        }
        "finalize" => {
            let (i, n, t) = (u16p(&a, 2)?, u16p(&a, 3)?, u16p(&a, 4)?);
            let key = cohortd::dkg_finalize(Path::new(arg(&a, 5)?), i, n, t).map_err(err)?;
            println!("finalize: participant {i} group_pubkey={}", hex(&key.group_pubkey));
        }
        "announce" => {
            let i = u16p(&a, 2)?;
            let seed = cohortd::parse_hex32(arg(&a, 3)?).map_err(err)?;
            let ts: u64 = arg(&a, 5)?.parse().map_err(|_| "bad ts".to_string())?;
            cohortd::announce_commit(Path::new(arg(&a, 4)?), i, &seed, ts).map_err(err)?;
            println!("announce: participant {i} committed nonces for tick {ts}");
        }
        "attest" => {
            let i = u16p(&a, 2)?;
            let seed = cohortd::parse_hex32(arg(&a, 3)?).map_err(err)?;
            let ts: u64 = arg(&a, 5)?.parse().map_err(|_| "bad ts".to_string())?;
            let price: u32 = arg(&a, 6)?.parse().map_err(|_| "bad price".to_string())?;
            let quorum = cohortd::quorum_csv(arg(&a, 7)?).map_err(err)?;
            cohortd::attest_share(Path::new(arg(&a, 4)?), i, &seed, ts, price, &quorum).map_err(err)?;
            println!("attest: participant {i} shared signature for tick {ts} @ ${price}");
        }
        "aggregate" => {
            let ts: u64 = arg(&a, 3)?.parse().map_err(|_| "bad ts".to_string())?;
            let price: u32 = arg(&a, 4)?.parse().map_err(|_| "bad price".to_string())?;
            let quorum = cohortd::quorum_csv(arg(&a, 5)?).map_err(err)?;
            let gpk = cohortd::parse_hex32(arg(&a, 6)?).map_err(err)?;
            cohortd::aggregate(Path::new(arg(&a, 2)?), ts, price, &quorum, &gpk).map_err(err)?;
            println!("aggregate: wrote ann-{ts}.hex + att-{ts}.hex");
        }
        _ => return Err("usage: cohortd <round1|finalize|announce|attest|aggregate> ...".into()),
    }
    Ok(())
}

fn arg<'a>(a: &'a [String], i: usize) -> Result<&'a str, String> {
    a.get(i).map(String::as_str).ok_or_else(|| format!("missing argument {i}"))
}
fn u16p(a: &[String], i: usize) -> Result<u16, String> {
    arg(a, i)?.parse().map_err(|_| format!("argument {i} must be a u16"))
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
