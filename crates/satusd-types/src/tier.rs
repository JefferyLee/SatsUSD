//! Collateral ratio + emergency tier (PRD §5.D8, DL-24 corrected formula).
//!
//! Units: `reserve_btc_sats` is satoshis, `price_e8` is USD×10^8, SatUSD
//! `supply_atoms` are cents. The DL-24 fix carries the 10^14 dimensional factor
//! the v5.0 formula was missing (it was off by 10^12).

/// Emergency tier (§5.D8). Discriminants frozen 0-based in declaration order
/// (ADR-0016), matching `StateRoot::emergency_tier` (u8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EmergencyTier {
    /// CR ≥ 150%: mint allowed, redeem allowed.
    Healthy = 0,
    /// 130% ≤ CR < 150%: `MINT_FINALIZE` rejected in-circuit; redeem allowed.
    PauseMint = 1,
    /// 110% ≤ CR < 130%: all mint rejected; redeem allowed (dashboard warns).
    Auction = 2,
    /// CR < 110%: all transitions go to manual review; automatic mode stops.
    Settlement = 3,
}

impl EmergencyTier {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Collateral ratio in parts-per-million (§5.D8, DL-24):
/// `cr_ppm = reserve_sats * price_e8 * 10^6 / (supply_atoms * 10^14)`.
///
/// Returns `None` when `supply_atoms == 0` (CR undefined). Panics only on the
/// documented overflow guard (`cr_ppm > u64::MAX`), which is unreachable for
/// realistic inputs.
pub fn collateral_ratio_ppm(reserve_sats: u64, supply_atoms: u64, price_e8: u64) -> Option<u64> {
    if supply_atoms == 0 {
        return None;
    }
    let num: u128 = (reserve_sats as u128)
        .checked_mul(price_e8 as u128)
        .unwrap()
        .checked_mul(1_000_000)
        .unwrap();
    let den: u128 = (supply_atoms as u128).checked_mul(10u128.pow(14)).unwrap();
    let cr_ppm = num / den;
    assert!(cr_ppm <= u64::MAX as u128, "cr_ppm overflow");
    Some(cr_ppm as u64)
}

/// Recompute the emergency tier from reserves, supply, and oracle price (§5.D8).
/// Zero supply is `Healthy` (no outstanding SatUSD to back).
pub fn recompute_tier(reserve_sats: u64, supply_atoms: u64, price_e8: u64) -> EmergencyTier {
    match collateral_ratio_ppm(reserve_sats, supply_atoms, price_e8) {
        None => EmergencyTier::Healthy,
        Some(cr_ppm) if cr_ppm >= 1_500_000 => EmergencyTier::Healthy,
        Some(cr_ppm) if cr_ppm >= 1_300_000 => EmergencyTier::PauseMint,
        Some(cr_ppm) if cr_ppm >= 1_100_000 => EmergencyTier::Auction,
        Some(_) => EmergencyTier::Settlement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRICE_50K: u64 = 5_000_000_000_000; // $50,000 × 10^8
    const SUPPLY_1M: u64 = 100_000_000; // $1M in cents

    // §5.D8 arithmetic-safety table (DL-24). NOTE: the PRD's last row prints
    // 5×10^14, but reserve=1e8, price=5e12, supply=100 yields 5×10^10 (reserve
    // $50k / supply $1 = 50000× = 5e10 ppm); the table cell is a typo.
    #[test]
    fn cr_ppm_matches_d8_table() {
        let cases = [
            (4_000_000_000u64, PRICE_50K, SUPPLY_1M, 2_000_000u64), // 200%
            (3_000_000_000, PRICE_50K, SUPPLY_1M, 1_500_000),       // 150%
            (2_000_000_000, PRICE_50K, SUPPLY_1M, 1_000_000),       // 100%
            (1_000_000_000, PRICE_50K, SUPPLY_1M, 500_000),         // 50%
            (100_000_000, PRICE_50K, 100, 50_000_000_000),          // tiny supply
        ];
        for (reserve, price, supply, want) in cases {
            assert_eq!(
                collateral_ratio_ppm(reserve, supply, price),
                Some(want),
                "reserve={reserve} price={price} supply={supply}"
            );
        }
    }

    #[test]
    fn tier_thresholds() {
        // Exactly on each boundary (ppm) → the higher tier.
        assert_eq!(
            recompute_tier(3_000_000_000, SUPPLY_1M, PRICE_50K),
            EmergencyTier::Healthy
        ); // 150%
        assert_eq!(
            recompute_tier(2_800_000_000, SUPPLY_1M, PRICE_50K),
            EmergencyTier::PauseMint
        ); // 140%
        assert_eq!(
            recompute_tier(2_600_000_000, SUPPLY_1M, PRICE_50K),
            EmergencyTier::PauseMint
        ); // 130%
        assert_eq!(
            recompute_tier(2_400_000_000, SUPPLY_1M, PRICE_50K),
            EmergencyTier::Auction
        ); // 120%
        assert_eq!(
            recompute_tier(2_200_000_000, SUPPLY_1M, PRICE_50K),
            EmergencyTier::Auction
        ); // 110%
        assert_eq!(
            recompute_tier(2_000_000_000, SUPPLY_1M, PRICE_50K),
            EmergencyTier::Settlement
        ); // 100%
    }

    #[test]
    fn zero_supply_is_healthy() {
        assert_eq!(collateral_ratio_ppm(0, 0, PRICE_50K), None);
        assert_eq!(recompute_tier(0, 0, PRICE_50K), EmergencyTier::Healthy);
    }
}
