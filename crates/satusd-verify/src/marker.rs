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
    /// The settlement's weight in µUSD. Per spec 03 §5.6 A this is the
    /// **costly-signal-backed** conversion size — the caller caps it at
    /// the rail's epoch capacity and excludes uncosted pure-`settle-to-LP`
    /// wash; it is NOT raw self-reported volume.
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

/// The adaptive-blend marker (spec 03 §5.4): one price that evolves
/// from external to internal as the internal market earns weight.
/// Pure — any observer recomputes it. The spec's `band(h)` is realised
/// here as two bands: `clip_band_bps` (the working clamp on
/// `|marker − external|`, widens with authority) and `halt_band_bps`
/// (the catastrophe fuse on `|internal − external|`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendParams {
    /// Internal weight, 0..=10_000. The CtM estimator (spec 03 §6.5)
    /// produces this and MUST cap it by `CtM_int/(CtM_int+CtM_ext)`;
    /// v0.1 uses the §5.3 step (0 until switchover, then high).
    pub w_bps: u16,
    /// Working clamp: `|marker − external| ≤ clip_band` (bps of external).
    pub clip_band_bps: u32,
    /// Catastrophe fuse: `|internal − external| > halt_band` ⇒ Invalid (bps).
    pub halt_band_bps: u32,
}

/// The blended marker, or `Invalid` when the two sources diverge past
/// the fuse — consumers (spec 04 reimbursement, 06 liquidation) MUST
/// halt on `Invalid` rather than act on a number no source can defend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Price(u64),
    Invalid,
}

fn bps_of(x: u64, bps: u32) -> u64 {
    (u128::from(x) * u128::from(bps) / 10_000) as u64
}

/// Blend `external` and `internal` per spec 03 §5.4. `internal = None`
/// (below the volume floor, §5.2) ⇒ the pure external marker.
pub fn adaptive_marker(external: u64, internal: Option<u64>, p: BlendParams) -> Marker {
    let internal = match internal {
        None => return Marker::Price(external),
        Some(i) => i,
    };
    // Catastrophe fuse: the sources fundamentally disagree.
    if internal.abs_diff(external) > bps_of(external, p.halt_band_bps) {
        return Marker::Invalid;
    }
    // P_marker = w·internal + (1−w)·external, then clipped to the band.
    let w = u128::from(p.w_bps.min(10_000));
    let blended =
        ((w * u128::from(internal) + (10_000 - w) * u128::from(external)) / 10_000) as u64;
    let clip = bps_of(external, p.clip_band_bps);
    let lo = external.saturating_sub(clip);
    let hi = external.saturating_add(clip);
    Marker::Price(blended.clamp(lo, hi))
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

    // ---- adaptive_marker (spec 03 §5.4) ----

    const WIDE: BlendParams = BlendParams { w_bps: 5_000, clip_band_bps: 5_000, halt_band_bps: 2_000 };

    #[test]
    fn no_internal_is_pure_external() {
        // Below the volume floor (None) ⇒ the external marker verbatim.
        assert_eq!(adaptive_marker(1_000_000, None, WIDE), Marker::Price(1_000_000));
    }

    #[test]
    fn weight_endpoints() {
        let p = |w| BlendParams { w_bps: w, clip_band_bps: 5_000, halt_band_bps: 2_000 };
        // w=0 ⇒ external; w=10000 ⇒ internal (band wide enough not to clip).
        assert_eq!(adaptive_marker(1_000_000, Some(1_080_000), p(0)), Marker::Price(1_000_000));
        assert_eq!(adaptive_marker(1_000_000, Some(1_080_000), p(10_000)), Marker::Price(1_080_000));
        // w=5000 ⇒ midpoint.
        assert_eq!(adaptive_marker(1_000_000, Some(1_080_000), p(5_000)), Marker::Price(1_040_000));
    }

    #[test]
    fn clip_band_binds() {
        // Full internal weight, but the working clamp caps the move at
        // ±2% of external even though internal is 8% away.
        let p = BlendParams { w_bps: 10_000, clip_band_bps: 200, halt_band_bps: 2_000 };
        assert_eq!(adaptive_marker(1_000_000, Some(1_080_000), p), Marker::Price(1_020_000));
    }

    #[test]
    fn catastrophe_fuse_halts() {
        // Sources disagree beyond the 2% fuse ⇒ Invalid (consumers halt).
        let p = BlendParams { w_bps: 5_000, clip_band_bps: 5_000, halt_band_bps: 200 };
        assert_eq!(adaptive_marker(1_000_000, Some(1_500_000), p), Marker::Invalid);
        // Just inside the fuse is fine.
        assert_eq!(adaptive_marker(1_000_000, Some(1_020_000), p), Marker::Price(1_010_000));
    }

    #[test]
    fn reflexivity_thin_market_cannot_drag_the_marker() {
        // A thin internal market ⇒ the CtM cap forces a tiny weight
        // (here 2%). Even with internal 8% off-market, the marker
        // barely leaves external — premature internal weight cannot
        // reflexively capture the price (spec 03 §5.4).
        let thin = BlendParams { w_bps: 200, clip_band_bps: 5_000, halt_band_bps: 2_000 };
        match adaptive_marker(1_000_000, Some(1_080_000), thin) {
            Marker::Price(p) => {
                // 1_000_000 + 2% * 80_000 = 1_001_600 — within 0.2% of external.
                assert_eq!(p, 1_001_600);
                assert!(p.abs_diff(1_000_000) <= bps_of(1_000_000, 20));
            }
            Marker::Invalid => panic!("should still produce a price"),
        }
    }
}
