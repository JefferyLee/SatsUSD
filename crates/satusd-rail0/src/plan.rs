//! The swap plan: the exact economic content of the collapsed
//! LOCK+SETTLE transaction, derived from (manifest, quote) before
//! anything is signed.
//!
//! The plan is what the quote's `lock_template_commitment` commits
//! to for Rail-0: the user verifies every output of the transaction
//! they are about to co-sign against this structure (spec 02 §3.1).

use satusd_rail::encode::{tagged_hash, Encoder};
use satusd_rail::manifest::{OracleSpec, RailManifest, SettlePrimitive};
use satusd_rail::quote::Quote;

pub const LOCK_TEMPLATE_TAG: &str = "SatUSD/rail0-lock-template/v1";

/// Conservative Bitcoin dust floor for the user payout output —
/// below this the network refuses the settlement transaction
/// outright (found the hard way by the first CLI redeem).
pub const DUST_FLOOR_SATS: u64 = 546;

/// Per spec 00 §3.7: fee computations round half up in the
/// protocol's favor.
fn fee_round_half_up(amount: u64, bps: u16) -> u64 {
    ((u128::from(amount) * u128::from(bps) + 5_000) / 10_000) as u64
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    /// Manifest is not a Rail-0 manifest.
    NotRail0,
    /// Quote's rail_id does not match the manifest.
    RailIdMismatch,
    /// Conversion exceeds the manifest's per-conversion cap.
    OverMaxSize,
    /// Net user payout would be zero or negative after fees.
    DustPayout,
}

/// The settlement transaction's economic content. All values in
/// sats except the burn, which is in µUSD (the TA layer's unit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapPlan {
    pub rail_id: [u8; 32],
    /// SatUSD burned to the tapd-native burn key (spec 01 §4).
    pub burn_micro_usd: u64,
    /// Gross BTC value of the conversion at the quoted price.
    pub gross_sats: u64,
    /// Paid per-settlement to named providers; zero for a Rail-0
    /// with no providers (service output omitted when zero).
    pub service_fee_sats: u64,
    /// What the user's BTC output actually carries.
    pub user_sats: u64,
    /// Disclosed for capacity accounting (spec 02 §6.1); applied at
    /// reimbursement, NOT an output of this transaction (ADR-0002:
    /// retain is a reimbursement haircut).
    pub retained_sats: u64,
}

impl SwapPlan {
    pub fn from_quote(manifest: &RailManifest, quote: &Quote) -> Result<Self, PlanError> {
        if manifest.oracle_spec != OracleSpec::None
            || manifest.settle_primitive != SettlePrimitive::AtomicSwap
        {
            return Err(PlanError::NotRail0);
        }
        if quote.rail_id != manifest.rail_id() {
            return Err(PlanError::RailIdMismatch);
        }
        let gross_sats = quote.user_sats();
        if gross_sats > manifest.max_size_sats {
            return Err(PlanError::OverMaxSize);
        }
        let service_fee_sats = fee_round_half_up(gross_sats, manifest.fee_schedule.service_bps);
        let fixed = manifest.fee_schedule.fixed_sats;
        let user_sats = gross_sats
            .checked_sub(service_fee_sats)
            .and_then(|v| v.checked_sub(fixed))
            .filter(|v| *v >= DUST_FLOOR_SATS)
            .ok_or(PlanError::DustPayout)?;
        Ok(SwapPlan {
            rail_id: quote.rail_id,
            burn_micro_usd: quote.amount_micro_usd,
            gross_sats,
            service_fee_sats,
            user_sats,
            retained_sats: fee_round_half_up(gross_sats, manifest.fee_schedule.retain_bps),
        })
    }

    fn canonical_encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.put_bytes32(&self.rail_id);
        e.put_u64(self.burn_micro_usd);
        e.put_u64(self.gross_sats);
        e.put_u64(self.service_fee_sats);
        e.put_u64(self.user_sats);
        e.put_u64(self.retained_sats);
        e.into_bytes()
    }

    /// What `Quote.lock_template_commitment` MUST equal for Rail-0.
    /// The user recomputes this from the manifest + quote and
    /// refuses to co-sign on mismatch.
    pub fn lock_template_commitment(&self) -> [u8; 32] {
        tagged_hash(LOCK_TEMPLATE_TAG, &self.canonical_encode())
    }

    /// S2 conservation check against observed settlement outputs:
    /// the burn carries exactly the quoted µUSD, the user output
    /// exactly the planned sats.
    pub fn matches_settlement(&self, burned_micro_usd: u64, user_output_sats: u64) -> bool {
        burned_micro_usd == self.burn_micro_usd && user_output_sats == self.user_sats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rail::{rail0_manifest, Rail0Params};
    use satusd_rail::manifest::{FeeSchedule, LpStake};

    fn manifest() -> RailManifest {
        rail0_manifest(Rail0Params {
            asset_id: [0x11; 32],
            max_size_sats: 5_000_000,
            committed_term: 4032,
            fee_schedule: FeeSchedule {
                retain_bps: 10,
                service_bps: 5,
                fixed_sats: 200,
            },
            price_dev_bound_bps: 100,
            quote_validity_s: 30,
            lp_stake: LpStake {
                amount_sats: 10_000_000,
                slash_conditions: vec![],
            },
        })
        .unwrap()
    }

    fn quote(m: &RailManifest) -> Quote {
        Quote {
            rail_id: m.rail_id(),
            // $100,000/BTC → 10^11 / 100_000 = 1_000_000 msat/USD.
            price_msat_per_usd: 1_000_000,
            // $1,000.
            amount_micro_usd: 1_000_000_000,
            expiry_unix_s: 1_700_000_000,
            lock_template_commitment: [0; 32],
        }
    }

    #[test]
    fn fee_math_worked_example() {
        let m = manifest();
        let p = SwapPlan::from_quote(&m, &quote(&m)).unwrap();
        // $1,000 at $100k/BTC = 1_000_000 sats gross.
        assert_eq!(p.gross_sats, 1_000_000);
        // service 5 bps → 500 sats; fixed 200.
        assert_eq!(p.service_fee_sats, 500);
        assert_eq!(p.user_sats, 1_000_000 - 500 - 200);
        // retain 10 bps → 1000 sats, disclosed not deducted here.
        assert_eq!(p.retained_sats, 1_000);
        assert_eq!(p.burn_micro_usd, 1_000_000_000);
    }

    #[test]
    fn fee_rounding_favors_protocol() {
        // 9_999 sats at 5 bps = 4.9995 → rounds half-up to 5.
        assert_eq!(fee_round_half_up(9_999, 5), 5);
        // exact half: 10_000 × 5 bps = 5.0 → 5; 11_000 × 5 = 5.5 → 6.
        assert_eq!(fee_round_half_up(10_000, 5), 5);
        assert_eq!(fee_round_half_up(11_000, 5), 6);
    }

    #[test]
    fn rejects_wrong_rail_and_oversize_and_dust() {
        let m = manifest();
        let mut q = quote(&m);
        q.rail_id = [9; 32];
        assert_eq!(
            SwapPlan::from_quote(&m, &q).unwrap_err(),
            PlanError::RailIdMismatch
        );

        let mut big = quote(&m);
        big.amount_micro_usd = 10_000_000_000; // $10,000 → 10M sats > cap
        assert_eq!(
            SwapPlan::from_quote(&m, &big).unwrap_err(),
            PlanError::OverMaxSize
        );

        let mut dust = quote(&m);
        dust.amount_micro_usd = 700_000; // $0.70 → 700 sats; 700−500−200 < 546 dust floor
        assert_eq!(
            SwapPlan::from_quote(&m, &dust).unwrap_err(),
            PlanError::DustPayout
        );
    }

    #[test]
    fn lock_template_commitment_binds_the_economics() {
        let m = manifest();
        let p = SwapPlan::from_quote(&m, &quote(&m)).unwrap();
        let c1 = p.lock_template_commitment();

        let mut tampered = p.clone();
        tampered.user_sats -= 1;
        assert_ne!(c1, tampered.lock_template_commitment());

        // Deterministic across recomputation.
        let p2 = SwapPlan::from_quote(&m, &quote(&m)).unwrap();
        assert_eq!(c1, p2.lock_template_commitment());
    }

    #[test]
    fn s2_settlement_match() {
        let m = manifest();
        let p = SwapPlan::from_quote(&m, &quote(&m)).unwrap();
        assert!(p.matches_settlement(p.burn_micro_usd, p.user_sats));
        assert!(!p.matches_settlement(p.burn_micro_usd - 1, p.user_sats));
        assert!(!p.matches_settlement(p.burn_micro_usd, p.user_sats + 1));
    }
}
