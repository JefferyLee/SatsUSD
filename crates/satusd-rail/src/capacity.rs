//! The capacity formula and the no-profitable-rug invariant
//! (spec 02 §6.1–6.2, ADR-0002).
//!
//! `capacity = α × (retained_fees + lp_stake)` with α a spec-pinned
//! constant — never a manifest field. Kept in integer arithmetic.

/// α = ALPHA_NUM / ALPHA_DEN = 1/2 (spec-level constant).
pub const ALPHA_NUM: u128 = 1;
pub const ALPHA_DEN: u128 = 2;

/// Per-epoch cap, in sats, on a rail's draw from the common reserve.
/// Floored — rounding favors the reserve (spec 00 §3.7).
pub fn capacity_sats(retained_fees_cum_sats: u64, lp_stake_sats: u64) -> u64 {
    let sunk = u128::from(retained_fees_cum_sats) + u128::from(lp_stake_sats);
    ((sunk * ALPHA_NUM) / ALPHA_DEN) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worked_example_from_adr_0002() {
        // New rail: 0.1 BTC stake, zero history → 0.05 BTC capacity.
        assert_eq!(capacity_sats(0, 10_000_000), 5_000_000);
    }

    #[test]
    fn no_profitable_rug_invariant() {
        // capacity < sunk cost for every non-zero position, across
        // magnitudes including the u64 extremes.
        let probes = [
            (0u64, 1u64),
            (1, 0),
            (1, 1),
            (12_345, 678_901),
            (u64::MAX, 0),
            (0, u64::MAX),
            (u64::MAX, u64::MAX),
        ];
        for (fees, stake) in probes {
            let cap = u128::from(capacity_sats(fees, stake));
            let sunk = u128::from(fees) + u128::from(stake);
            assert!(cap < sunk, "rug must be EV-negative: cap={cap} sunk={sunk}");
        }
    }

    #[test]
    fn zero_position_zero_capacity() {
        assert_eq!(capacity_sats(0, 0), 0);
    }
}
