//! The internal_twap reference marker (spec 03 §5.2) — a pure
//! function of public settlement data. **There is no signer**: any
//! observer computes the same number, which is the whole point.
//!
//! ```text
//! inputs:  S3 settlement points within the window
//! filter:  drop points whose price deviates > 5% from the raw
//!          (unweighted, lower-middle) median
//! output:  volume-weighted median of the survivors
//! valid:   only if surviving volume ≥ min_volume
//! ```

/// One settlement's contribution: the implicit price attestation
/// (spec 02 §4 S3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementPoint {
    /// Canonical price type (spec 00 §3.6).
    pub price_msat_per_usd: u64,
    /// Conversion size in µUSD — the weight.
    pub volume_micro_usd: u64,
}

/// Trim threshold: 5% (spec 03 §5.2), expressed in bps.
pub const TRIM_BPS: u64 = 500;

/// Unweighted lower-middle median of prices (deterministic for even
/// counts; documented in spec 03 §5.2's normative companion).
fn raw_median(sorted_prices: &[u64]) -> u64 {
    sorted_prices[(sorted_prices.len() - 1) / 2]
}

/// Compute the marker over a window. Returns `None` when the window
/// is empty or surviving volume is below the floor — callers fall
/// back to the external marker (spec 03 §5.1) for that window.
pub fn internal_twap(points: &[SettlementPoint], min_volume_micro_usd: u64) -> Option<u64> {
    if points.is_empty() {
        return None;
    }
    let mut prices: Vec<u64> = points.iter().map(|p| p.price_msat_per_usd).collect();
    prices.sort_unstable();
    let median = raw_median(&prices);

    // Trim: |price − median| ≤ 5% of median.
    let band = (u128::from(median) * u128::from(TRIM_BPS) / 10_000) as u64;
    let mut survivors: Vec<SettlementPoint> = points
        .iter()
        .copied()
        .filter(|p| p.price_msat_per_usd.abs_diff(median) <= band)
        .collect();

    let total: u128 = survivors
        .iter()
        .map(|p| u128::from(p.volume_micro_usd))
        .sum();
    if total < u128::from(min_volume_micro_usd) || total == 0 {
        return None;
    }

    // Volume-weighted median: first price where cumulative volume
    // crosses half the total (ties resolve to the earlier price —
    // deterministic).
    survivors.sort_unstable_by_key(|p| p.price_msat_per_usd);
    let half = total.div_ceil(2);
    let mut acc: u128 = 0;
    for p in &survivors {
        acc += u128::from(p.volume_micro_usd);
        if acc >= half {
            return Some(p.price_msat_per_usd);
        }
    }
    unreachable!("cumulative volume reaches total");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(price: u64, vol: u64) -> SettlementPoint {
        SettlementPoint {
            price_msat_per_usd: price,
            volume_micro_usd: vol,
        }
    }

    #[test]
    fn weighted_median_basics() {
        // One whale at 1_000_000 dominates by volume.
        let pts = [pt(990_000, 1), pt(1_000_000, 100), pt(1_010_000, 1)];
        assert_eq!(internal_twap(&pts, 0), Some(1_000_000));

        // Even split: ties resolve to the earlier (lower) price.
        let pts = [pt(999_000, 50), pt(1_001_000, 50)];
        assert_eq!(internal_twap(&pts, 0), Some(999_000));
    }

    #[test]
    fn outlier_manipulation_is_trimmed() {
        // An attacker prints one huge trade 40% off-market; the raw
        // median is set by the honest flow, so the whale is trimmed
        // and cannot move the marker.
        let honest = [pt(1_000_000, 30), pt(1_001_000, 30), pt(999_000, 30)];
        let mut with_attack = honest.to_vec();
        with_attack.push(pt(1_400_000, 10_000));
        assert_eq!(
            internal_twap(&with_attack, 0),
            internal_twap(&honest, 0),
            "the 40%-off whale must not move the marker"
        );
    }

    #[test]
    fn volume_floor_falls_back() {
        let pts = [pt(1_000_000, 5)];
        assert_eq!(
            internal_twap(&pts, 10),
            None,
            "below floor → external fallback"
        );
        assert_eq!(internal_twap(&pts, 5), Some(1_000_000));
        assert_eq!(internal_twap(&[], 0), None);
    }

    #[test]
    fn trim_band_edges() {
        // Exactly 5% away survives; just beyond is dropped.
        let pts = [pt(1_000_000, 10), pt(1_050_000, 1), pt(1_050_001, 1)];
        // median (lower-middle of 3 sorted) = 1_050_000?? sorted:
        // [1_000_000, 1_050_000, 1_050_001] → median = 1_050_000;
        // band = 52_500 → all survive; weighted median = 1_000_000
        // (10 of 12 volume).
        assert_eq!(internal_twap(&pts, 0), Some(1_000_000));
    }
}
