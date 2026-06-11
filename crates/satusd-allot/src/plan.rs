//! The allotment plan: rail disclosures in, per-rail tranches out,
//! deterministically.
//!
//! `capacity = α × (retained_fees + stake)` per rail (spec 02 §6.1,
//! α spec-pinned in `satusd-rail`). If the sum of capacities exceeds
//! the reserve, every tranche scales proportionally (floored) — a
//! deterministic rule that favors no rail by ordering.

use satusd_rail::capacity::capacity_sats;
use satusd_rail::encode::{tagged_hash, Encoder};
use serde::Deserialize;

pub const PLAN_TAG: &str = "SatUSD/allotment-plan/v1";
pub const INPUT_TAG: &str = "SatUSD/allotment-input/v1";

/// One rail's public position, aggregated from its disclosures
/// (spec 02 §6.4) and chain data. All values verifiable by anyone.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RailPosition {
    /// Hex rail_id in JSON; bytes internally.
    #[serde(with = "hex_32")]
    pub rail_id: [u8; 32],
    pub retained_fees_cum_sats: u64,
    pub lp_stake_sats: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AllotmentInput {
    pub epoch: u32,
    pub reserve_total_sats: u64,
    pub positions: Vec<RailPosition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tranche {
    pub rail_id: [u8; 32],
    pub capacity_sats: u64,
    /// After proportional scaling (== capacity unless oversubscribed).
    pub tranche_sats: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllotmentPlan {
    pub epoch: u32,
    pub reserve_total_sats: u64,
    /// Hash of the canonical input — re-runners check THIS first.
    pub input_hash: [u8; 32],
    /// Sorted by rail_id.
    pub tranches: Vec<Tranche>,
    pub unallocated_sats: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    DuplicateRailId([u8; 32]),
}

fn canonical_input(epoch: u32, reserve: u64, sorted: &[RailPosition]) -> Vec<u8> {
    let mut e = Encoder::new();
    e.put_u32(epoch);
    e.put_u64(reserve);
    e.put_bigsize(sorted.len() as u64);
    for p in sorted {
        e.put_bytes32(&p.rail_id);
        e.put_u64(p.retained_fees_cum_sats);
        e.put_u64(p.lp_stake_sats);
    }
    e.into_bytes()
}

impl AllotmentPlan {
    /// Compute the plan. Positions are sorted by rail_id internally,
    /// so JSON ordering cannot change the output.
    pub fn compute(input: &AllotmentInput) -> Result<Self, PlanError> {
        let mut sorted = input.positions.clone();
        sorted.sort_by_key(|p| p.rail_id);
        for w in sorted.windows(2) {
            if w[0].rail_id == w[1].rail_id {
                return Err(PlanError::DuplicateRailId(w[0].rail_id));
            }
        }
        let input_hash = tagged_hash(
            INPUT_TAG,
            &canonical_input(input.epoch, input.reserve_total_sats, &sorted),
        );

        let caps: Vec<u64> = sorted
            .iter()
            .map(|p| capacity_sats(p.retained_fees_cum_sats, p.lp_stake_sats))
            .collect();
        let total_cap: u128 = caps.iter().map(|c| u128::from(*c)).sum();
        let reserve = u128::from(input.reserve_total_sats);

        let tranches: Vec<Tranche> = sorted
            .iter()
            .zip(&caps)
            .map(|(p, cap)| {
                let tranche = if total_cap <= reserve || total_cap == 0 {
                    *cap
                } else {
                    // Proportional floor scaling — deterministic,
                    // order-independent, never exceeds the reserve.
                    (u128::from(*cap) * reserve / total_cap) as u64
                };
                Tranche {
                    rail_id: p.rail_id,
                    capacity_sats: *cap,
                    tranche_sats: tranche,
                }
            })
            .collect();
        let allocated: u64 = tranches.iter().map(|t| t.tranche_sats).sum();

        Ok(AllotmentPlan {
            epoch: input.epoch,
            reserve_total_sats: input.reserve_total_sats,
            input_hash,
            unallocated_sats: input.reserve_total_sats.saturating_sub(allocated),
            tranches,
        })
    }

    /// The canonical plan bytes — what re-runners compare.
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.put_u32(self.epoch);
        e.put_u64(self.reserve_total_sats);
        e.put_bytes32(&self.input_hash);
        e.put_bigsize(self.tranches.len() as u64);
        for t in &self.tranches {
            e.put_bytes32(&t.rail_id);
            e.put_u64(t.capacity_sats);
            e.put_u64(t.tranche_sats);
        }
        e.put_u64(self.unallocated_sats);
        e.into_bytes()
    }

    pub fn plan_hash(&self) -> [u8; 32] {
        tagged_hash(PLAN_TAG, &self.canonical_encode())
    }
}

mod hex_32 {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let bytes = (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("expected 32 bytes of hex"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(id: u8, fees: u64, stake: u64) -> RailPosition {
        RailPosition {
            rail_id: [id; 32],
            retained_fees_cum_sats: fees,
            lp_stake_sats: stake,
        }
    }

    #[test]
    fn order_independent_and_deterministic() {
        let a = AllotmentInput {
            epoch: 7,
            reserve_total_sats: 100_000_000,
            positions: vec![pos(2, 1_000, 10_000_000), pos(1, 0, 4_000_000)],
        };
        let b = AllotmentInput {
            positions: vec![pos(1, 0, 4_000_000), pos(2, 1_000, 10_000_000)],
            ..a.clone()
        };
        let pa = AllotmentPlan::compute(&a).unwrap();
        let pb = AllotmentPlan::compute(&b).unwrap();
        assert_eq!(pa, pb);
        assert_eq!(pa.plan_hash(), pb.plan_hash());
        // Capacities per the formula: α = 1/2.
        assert_eq!(pa.tranches[0].capacity_sats, 2_000_000);
        assert_eq!(pa.tranches[1].capacity_sats, 5_000_500);
        assert_eq!(pa.unallocated_sats, 100_000_000 - 7_000_500);
    }

    #[test]
    fn oversubscription_scales_proportionally_and_never_exceeds_reserve() {
        let input = AllotmentInput {
            epoch: 1,
            reserve_total_sats: 3_000_000,
            positions: vec![pos(1, 0, 8_000_000), pos(2, 0, 4_000_000)],
        };
        let p = AllotmentPlan::compute(&input).unwrap();
        // caps: 4M and 2M (total 6M) > reserve 3M → halved.
        assert_eq!(p.tranches[0].tranche_sats, 2_000_000);
        assert_eq!(p.tranches[1].tranche_sats, 1_000_000);
        let total: u64 = p.tranches.iter().map(|t| t.tranche_sats).sum();
        assert!(total <= input.reserve_total_sats);
    }

    #[test]
    fn duplicate_rail_rejected() {
        let input = AllotmentInput {
            epoch: 1,
            reserve_total_sats: 1,
            positions: vec![pos(1, 0, 1), pos(1, 0, 2)],
        };
        assert_eq!(
            AllotmentPlan::compute(&input).unwrap_err(),
            PlanError::DuplicateRailId([1; 32])
        );
    }

    /// Pinned vector: the byte-for-byte reproducibility contract.
    #[test]
    fn pinned_plan_hash() {
        let input = AllotmentInput {
            epoch: 42,
            reserve_total_sats: 50_000_000,
            positions: vec![pos(0x11, 123_456, 10_000_000), pos(0x22, 0, 2_000_000)],
        };
        let p = AllotmentPlan::compute(&input).unwrap();
        assert_eq!(
            hex::encode(p.plan_hash()),
            "fbc61e5d5310af7b3b13df1b12f5a707a59fd406649f312566e661b05470dcfb"
        );
    }
}
