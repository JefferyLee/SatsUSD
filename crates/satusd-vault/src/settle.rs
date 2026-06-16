//! Crash / maturity settlement payouts (spec 06 §4–5).
//!
//! A settled CET at price `p` splits the collateral three ways:
//! `face + penalty` to the reserve, a `bounty` to whoever broadcast
//! it, the remainder back to the minter. The outputs always sum to
//! the spendable collateral (collateral − fee budget); rounding favors
//! the reserve (spec 00 §3.7). When the price has gapped below the
//! crash bucket far enough that `face + penalty` exceeds the
//! collateral, the reserve is clamped to what's there — the
//! `reserve_shortfall` is the gap the pooled cushion absorbs
//! (ADR-0004 §3, spec 04 §5).

const SATS_PER_BTC: u128 = 100_000_000;
const MICRO_PER_USD: u128 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrashPayout {
    /// To the common reserve (backs the now-orphaned circulating SatUSD).
    pub reserve_sats: u64,
    /// To whoever broadcast the CET — the anyone-can-act incentive.
    pub broadcaster_sats: u64,
    /// Back to the minter (the over-collateralized cushion, less penalty).
    pub minter_sats: u64,
}

impl CrashPayout {
    pub fn total(&self) -> u64 {
        self.reserve_sats + self.broadcaster_sats + self.minter_sats
    }
}

/// Parameters that don't change per-bucket (spec 06 §8 starting values).
#[derive(Clone, Copy, Debug)]
pub struct PayoutParams {
    pub penalty_bps: u32,
    /// Bounty as bps of the penalty (e.g. 1000 = 10%).
    pub bounty_bps_of_penalty: u32,
    pub bounty_cap_sats: u64,
    /// On-chain fee reserved off the top of the settlement.
    pub fee_budget_sats: u64,
}

/// Face value of the vault at price `p`, in sats — what the reserve is
/// owed. Rounds UP (reserve favor).
pub fn face_sats(mint_micro_usd: u64, price_usd: u32) -> u64 {
    if price_usd == 0 {
        return u64::MAX;
    }
    // face_sats = ceil(mint_µUSD * SATS_PER_BTC / (price * MICRO_PER_USD))
    let num = u128::from(mint_micro_usd) * SATS_PER_BTC;
    let den = u128::from(price_usd) * MICRO_PER_USD;
    ((num + den - 1) / den).min(u128::from(u64::MAX)) as u64
}

/// Split a settlement at price `p`. Outputs sum to
/// `collateral − fee_budget`.
pub fn crash_payout(
    collateral_sats: u64,
    mint_micro_usd: u64,
    price_usd: u32,
    p: PayoutParams,
) -> CrashPayout {
    let face = face_sats(mint_micro_usd, price_usd);
    let penalty = (u128::from(face) * u128::from(p.penalty_bps) / 10_000) as u64;
    let mut bounty = (u128::from(penalty) * u128::from(p.bounty_bps_of_penalty) / 10_000) as u64;
    bounty = bounty.min(p.bounty_cap_sats);

    let spendable = collateral_sats.saturating_sub(p.fee_budget_sats);
    bounty = bounty.min(spendable);

    // Reserve target is face + penalty, less the bounty carved out of it;
    // clamp to whatever collateral is left (the gap case).
    let reserve_want = face.saturating_add(penalty).saturating_sub(bounty);
    let reserve = reserve_want.min(spendable - bounty);
    let minter = spendable - bounty - reserve;

    CrashPayout {
        reserve_sats: reserve,
        broadcaster_sats: bounty,
        minter_sats: minter,
    }
}

/// The reserve's shortfall vs the face it is owed (0 unless the price
/// gapped past the cushion). This is the residual gap risk the pooled
/// cushion / NAV floor absorbs.
pub fn reserve_shortfall_sats(payout: &CrashPayout, mint_micro_usd: u64, price_usd: u32) -> u64 {
    face_sats(mint_micro_usd, price_usd).saturating_sub(payout.reserve_sats)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A realistic $1000 vault opened ~150% @ $64k (collateral ≈ 0.0234 BTC).
    const MINT: u64 = 1_000_000_000; // $1000 µUSD
    const COLLATERAL: u64 = 2_343_750; // 150% @ $64000

    fn params() -> PayoutParams {
        PayoutParams {
            penalty_bps: 500,
            bounty_bps_of_penalty: 1_000, // 10% of penalty
            bounty_cap_sats: 5_000,
            fee_budget_sats: 300,
        }
    }

    #[test]
    fn outputs_sum_to_spendable() {
        for price in [46_900u32, 44_000, 40_000, 30_000, 20_000] {
            let po = crash_payout(COLLATERAL, MINT, price, params());
            assert_eq!(po.total(), COLLATERAL - 300, "price {price}: outputs must sum to spendable");
        }
    }

    #[test]
    fn near_liquidation_minter_keeps_cushion() {
        // Just into crash territory (~110% CR ≈ $46.9k): reserve gets
        // face+penalty, minter keeps the thin remaining cushion, no gap.
        let price = 46_900;
        let po = crash_payout(COLLATERAL, MINT, price, params());
        assert!(po.reserve_sats >= face_sats(MINT, price), "reserve covers face");
        assert!(po.minter_sats > 0, "minter keeps the cushion above face+penalty");
        assert_eq!(reserve_shortfall_sats(&po, MINT, price), 0, "no gap near liquidation");
    }

    #[test]
    fn deep_gap_clamps_reserve_and_zeroes_minter() {
        // A violent gap far below the crash bucket: face exceeds the
        // collateral. Reserve takes all it can (minus bounty), minter 0,
        // and the shortfall is the gap the cushion absorbs.
        let price = 20_000; // face ≈ 5M sats > 2.34M collateral
        let po = crash_payout(COLLATERAL, MINT, price, params());
        assert_eq!(po.minter_sats, 0, "no cushion left in a deep gap");
        assert!(reserve_shortfall_sats(&po, MINT, price) > 0, "reserve under-recovers");
        assert_eq!(po.total(), COLLATERAL - 300);
    }

    #[test]
    fn bounty_is_capped() {
        let mut p = params();
        p.bounty_cap_sats = 100;
        let po = crash_payout(COLLATERAL, MINT, 46_900, p);
        assert_eq!(po.broadcaster_sats, 100, "bounty respects the cap");
    }
}
