//! The optimistic bonded dispute layer (ADR-0008, spec 03 §5.8) — for the
//! **unilateral** settlement path only.
//!
//! A bonded proposer asserts the maturity BTC/USD price; a dispute window
//! opens; resolution then:
//! - **undisputed + within band** of the public reference → the asserted
//!   price stands and the FROST cohort signs it (`Accepted`);
//! - **undisputed + out of band** → refuse to attest (`Rejected::OutOfBand`)
//!   — the cross-check is a HARD gate even when nobody disputed, so a
//!   collusive or erroneous assertion that slips through a sleepy watchtower
//!   is still caught;
//! - **disputed** → the price closest to the public reference wins and the
//!   loser's bond is slashed (`Disputed`); if even the winner is out of band,
//!   `Rejected::OutOfBand`;
//! - **before the window closes, undisputed** → `Rejected::WindowStillOpen`
//!   (pending).
//!
//! On `Rejected` the cohort signs nothing; settlement falls to the holder's
//! offline floor / a fresh assertion. The COOPERATIVE path (holder + LP
//! co-sign at maturity, FR-10) bypasses this module entirely and is instant.
//!
//! Pure logic — no chain, clock, or IO. The on-chain bonds, the cohort
//! signing the resolved price, the public-reference feed (`oracled` live
//! multi-venue median / Chainlink / Pyth), and the daemon transport compose
//! on top.

/// A proposer's bonded assertion of an event's maturity price.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assertion {
    /// The dlcspecs event this prices (`SatUSD/BTCUSD/<unix_ts>`).
    pub event_id: String,
    pub price_usd: u32,
    pub bond_sats: u64,
    pub asserted_at_block: u32,
    pub window_blocks: u32,
}

/// A challenger's competing bonded price.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispute {
    pub price_usd: u32,
    pub bond_sats: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// The (winning) price diverges from the public reference beyond the band.
    OutOfBand,
    /// No dispute yet and the window has not closed — resolution is pending.
    WindowStillOpen,
}

/// The outcome of resolving an assertion. On `Accepted`/`Disputed` the cohort
/// signs `price_usd`; on `Rejected` it signs nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Undisputed and within band: the asserted price stands.
    Accepted { price_usd: u32 },
    /// Disputed: `price_usd` (the side closest to the reference) wins; the
    /// loser forfeits `slashed_bond_sats` to the winner / the protocol.
    Disputed { price_usd: u32, slashed_bond_sats: u64 },
    Rejected { reason: RejectReason },
}

/// Cross-check: is `price_usd` within `band_bps` of the public `reference_usd`?
pub fn within_band(price_usd: u32, reference_usd: u32, band_bps: u32) -> bool {
    let diff = u128::from(price_usd.abs_diff(reference_usd));
    let tol = u128::from(reference_usd) * u128::from(band_bps) / 10_000;
    diff <= tol
}

/// Is the dispute window still open at `now_block`?
pub fn window_open(a: &Assertion, now_block: u32) -> bool {
    now_block < a.asserted_at_block.saturating_add(a.window_blocks)
}

fn closer(target: u32, a: u32, b: u32) -> bool {
    // true iff `a` is at least as close to `target` as `b`.
    a.abs_diff(target) <= b.abs_diff(target)
}

/// Resolve an assertion against the public `reference_usd` with a cross-check
/// `band_bps`. `dispute` is the competing claim, if any. `now_block` is the
/// current height (only consulted for the undisputed-pending case).
pub fn resolve(
    a: &Assertion,
    dispute: Option<&Dispute>,
    reference_usd: u32,
    band_bps: u32,
    now_block: u32,
) -> Resolution {
    match dispute {
        // A dispute is resolved immediately against the public reference —
        // the side closest to it wins; the loser's bond is slashed.
        Some(d) => {
            let asserted_wins = closer(reference_usd, a.price_usd, d.price_usd);
            let (winner, slashed_bond) = if asserted_wins {
                (a.price_usd, d.bond_sats)
            } else {
                (d.price_usd, a.bond_sats)
            };
            if within_band(winner, reference_usd, band_bps) {
                Resolution::Disputed { price_usd: winner, slashed_bond_sats: slashed_bond }
            } else {
                // Even the closer side diverges from the public price — refuse.
                Resolution::Rejected { reason: RejectReason::OutOfBand }
            }
        }
        // Undisputed: only final once the window has closed; the cross-check
        // is still a hard gate.
        None => {
            if window_open(a, now_block) {
                Resolution::Rejected { reason: RejectReason::WindowStillOpen }
            } else if within_band(a.price_usd, reference_usd, band_bps) {
                Resolution::Accepted { price_usd: a.price_usd }
            } else {
                Resolution::Rejected { reason: RejectReason::OutOfBand }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assertion(price: u32) -> Assertion {
        Assertion {
            event_id: "SatUSD/BTCUSD/1700000000".into(),
            price_usd: price,
            bond_sats: 1_000_000,
            asserted_at_block: 100,
            window_blocks: 18, // ~3h at 10 min/block
        }
    }

    #[test]
    fn cross_check_band() {
        // 1% band around 100_000 = ±1000.
        assert!(within_band(100_000, 100_000, 100));
        assert!(within_band(101_000, 100_000, 100)); // exactly at the edge
        assert!(within_band(99_000, 100_000, 100));
        assert!(!within_band(101_001, 100_000, 100)); // just over
        assert!(!within_band(98_999, 100_000, 100));
    }

    #[test]
    fn window_gating() {
        let a = assertion(100_000);
        assert!(window_open(&a, 100));
        assert!(window_open(&a, 117)); // last open block
        assert!(!window_open(&a, 118)); // closed
        assert!(!window_open(&a, 200));
    }

    #[test]
    fn undisputed_in_band_after_window_is_accepted() {
        let a = assertion(100_050);
        // window closed (118+), asserted 100_050 within 1% of ref 100_000.
        assert_eq!(resolve(&a, None, 100_000, 100, 120), Resolution::Accepted { price_usd: 100_050 });
    }

    #[test]
    fn undisputed_pending_while_window_open() {
        let a = assertion(100_050);
        assert_eq!(
            resolve(&a, None, 100_000, 100, 105),
            Resolution::Rejected { reason: RejectReason::WindowStillOpen }
        );
    }

    #[test]
    fn undisputed_out_of_band_is_rejected_even_unchallenged() {
        // The cross-check is a hard gate: a divergent price nobody disputed is
        // still refused (defends against a sleepy watchtower).
        let a = assertion(108_000); // 8% above ref 100_000, band 1%
        assert_eq!(
            resolve(&a, None, 100_000, 100, 200),
            Resolution::Rejected { reason: RejectReason::OutOfBand }
        );
    }

    #[test]
    fn disputed_closer_side_wins_and_loser_is_slashed() {
        // Proposer lies high (108k); challenger reports the true 100_050.
        let a = assertion(108_000);
        let d = Dispute { price_usd: 100_050, bond_sats: 2_000_000 };
        // Challenger is closer to ref 100_000 → challenger wins, proposer's
        // bond (1_000_000) is slashed. Resolved immediately (no need to wait).
        assert_eq!(
            resolve(&a, Some(&d), 100_000, 100, 101),
            Resolution::Disputed { price_usd: 100_050, slashed_bond_sats: 1_000_000 }
        );
    }

    #[test]
    fn disputed_honest_proposer_keeps_bond_frivolous_challenger_slashed() {
        let a = assertion(100_050); // honest, near ref
        let d = Dispute { price_usd: 95_000, bond_sats: 2_000_000 }; // frivolous
        assert_eq!(
            resolve(&a, Some(&d), 100_000, 100, 101),
            Resolution::Disputed { price_usd: 100_050, slashed_bond_sats: 2_000_000 }
        );
    }

    #[test]
    fn disputed_both_out_of_band_is_rejected() {
        // Both sides diverge from the public reference → refuse to attest.
        let a = assertion(108_000);
        let d = Dispute { price_usd: 92_000, bond_sats: 2_000_000 };
        assert_eq!(
            resolve(&a, Some(&d), 100_000, 100, 101),
            Resolution::Rejected { reason: RejectReason::OutOfBand }
        );
    }
}
