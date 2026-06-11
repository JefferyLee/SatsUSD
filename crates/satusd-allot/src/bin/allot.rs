//! Stage-1 allotment runner.
//!
//! ```text
//! allot <input.json>
//! ```
//!
//! Input JSON (assembled from public rail disclosures + chain data):
//!
//! ```json
//! {
//!   "epoch": 42,
//!   "reserve_total_sats": 50000000,
//!   "positions": [
//!     { "rail_id": "<64 hex>", "retained_fees_cum_sats": 0,
//!       "lp_stake_sats": 10000000 }
//!   ]
//! }
//! ```
//!
//! Prints the canonical plan hex + plan hash. A re-runner with the
//! same input MUST produce identical bytes — anything else is
//! challengeable evidence (spec 04 §3 Stage 1).

use satusd_allot::plan::{AllotmentInput, AllotmentPlan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: allot <input.json>")?;
    let input: AllotmentInput = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let plan = AllotmentPlan::compute(&input).map_err(|e| format!("{e:?}"))?;

    println!("epoch          : {}", plan.epoch);
    println!("reserve        : {} sats", plan.reserve_total_sats);
    println!("input_hash     : {}", hex(&plan.input_hash));
    for t in &plan.tranches {
        println!(
            "rail {}… cap={} tranche={}",
            &hex(&t.rail_id)[..16],
            t.capacity_sats,
            t.tranche_sats
        );
    }
    println!("unallocated    : {} sats", plan.unallocated_sats);
    println!("plan (canonical): {}", hex(&plan.canonical_encode()));
    println!("plan_hash      : {}", hex(&plan.plan_hash()));
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
