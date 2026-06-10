//! Rail-0 manifest construction: the spec-pinned parts of the
//! manifest are fixed here; the LP chooses only the economic knobs.

use satusd_rail::manifest::{
    AutomationTier, Direction, FeeSchedule, LivenessEscape, LpStake, OracleSpec, RailManifest,
    SettlePrimitive, Transition, SPEC_VERSION,
};

/// The economic knobs an LP chooses when launching a Rail-0
/// instance. Everything else about Rail-0 is fixed by spec 02 §7.
#[derive(Clone, Debug)]
pub struct Rail0Params {
    pub asset_id: [u8; 32],
    pub max_size_sats: u64,
    pub fee_schedule: FeeSchedule,
    /// MUST be > 0: Rail-0 settlements draw reserve reimbursement,
    /// and the reimbursement price is bounded against the marker
    /// (spec 02 §2, spec 04 §4).
    pub price_dev_bound_bps: u16,
    pub quote_validity_s: u32,
    pub lp_stake: LpStake,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Rail0Error {
    /// `price_dev_bound_bps` must be non-zero for reimbursing rails.
    ZeroPriceDevBound,
}

/// Build the Rail-0 manifest. The collapsed machine has no LOCK and
/// no reachable REFUND, so the only declared transitions are SETTLE
/// (anyone-can-act: either party broadcasts the co-signed tx) and
/// DISPUTE (anyone-can-act: evidence submission is permissionless).
pub fn rail0_manifest(p: Rail0Params) -> Result<RailManifest, Rail0Error> {
    if p.price_dev_bound_bps == 0 {
        return Err(Rail0Error::ZeroPriceDevBound);
    }
    Ok(RailManifest {
        spec_version: SPEC_VERSION,
        direction: Direction::Redeem,
        asset_id: p.asset_id,
        oracle_spec: OracleSpec::None,
        settle_primitive: SettlePrimitive::AtomicSwap,
        max_size_sats: p.max_size_sats,
        fee_schedule: p.fee_schedule,
        price_dev_bound_bps: p.price_dev_bound_bps,
        quote_validity_s: p.quote_validity_s,
        // Collapsed machine: the settle window is the quote window —
        // nothing exists between acceptance and the atomic tx.
        settle_window_s: p.quote_validity_s,
        refund_delta_blocks: 0,
        lp_stake: p.lp_stake,
        dispute_hook: [0u8; 32],
        liveness_escape: LivenessEscape::None,
        automation_tiers: vec![
            (Transition::Settle, AutomationTier::AnyoneCanAct),
            (Transition::Dispute, AutomationTier::AnyoneCanAct),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Rail0Params {
        Rail0Params {
            asset_id: [0x11; 32],
            max_size_sats: 5_000_000,
            fee_schedule: FeeSchedule {
                retain_bps: 10,
                service_bps: 0,
                fixed_sats: 200,
            },
            price_dev_bound_bps: 100,
            quote_validity_s: 30,
            lp_stake: LpStake {
                amount_sats: 10_000_000,
                slash_conditions: vec![],
            },
        }
    }

    #[test]
    fn zero_dev_bound_rejected() {
        let mut p = params();
        p.price_dev_bound_bps = 0;
        assert_eq!(
            rail0_manifest(p).unwrap_err(),
            Rail0Error::ZeroPriceDevBound
        );
    }

    #[test]
    fn manifest_is_collapsed_and_oracle_free() {
        let m = rail0_manifest(params()).unwrap();
        assert_eq!(m.oracle_spec, OracleSpec::None);
        assert_eq!(m.settle_primitive, SettlePrimitive::AtomicSwap);
        assert_eq!(m.refund_delta_blocks, 0);
        // Economic knobs change the rail identity.
        let mut other = params();
        other.fee_schedule.retain_bps = 11;
        assert_ne!(
            rail0_manifest(params()).unwrap().rail_id(),
            rail0_manifest(other).unwrap().rail_id()
        );
    }
}
