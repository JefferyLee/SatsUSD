//! `RailManifest` (spec 02 §2) and its self-certifying `rail_id`.
//!
//! The manifest is immutable: changing any field yields a different
//! rail. Field order below is the normative canonical-encoding
//! order; enum discriminants are declaration order from 0
//! (spec 00 §3.3).

use crate::encode::{tagged_hash, Encoder};

pub const RAIL_ID_TAG: &str = "SatUSD/rail-manifest/v1";

/// Spec 02 version this manifest targets.
pub const SPEC_VERSION: u16 = 1;

/// Spec 02 §2 (ADR-0005): `Mint` (= issue: BTC→note) is the v0 rail
/// direction; v0 redemption is the unilateral DLC (spec 07 §3), not a
/// rail. `Redeem`/`Both` are the deferred reserve-era conversion
/// directions. Discriminants unchanged: Redeem=0, Mint=1, Both=2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Redeem,
    Mint,
    Both,
}

/// Spec 03 §3.1 — dlcspecs-class event template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventTemplate {
    pub pair: String,
    pub cadence_s: u32,
    pub base: u16,
    pub nb_digits: u16,
    pub is_signed: bool,
    pub precision: i32,
    pub unit: String,
}

impl EventTemplate {
    /// Spec 03 §3.1 defaults for BTC/USD ticks.
    pub fn btcusd_default() -> Self {
        Self {
            pair: "BTCUSD".into(),
            cadence_s: 1,
            base: 2,
            nb_digits: 20,
            is_signed: false,
            precision: 0,
            unit: "USD/BTC".into(),
        }
    }

    fn encode(&self, e: &mut Encoder) {
        e.put_string(&self.pair);
        e.put_u32(self.cadence_s);
        e.put_u16(self.base);
        e.put_u16(self.nb_digits);
        e.put_bool(self.is_signed);
        e.put_i32(self.precision);
        e.put_string(&self.unit);
    }
}

/// Spec 03 §1 — the oracle class registry. Discriminants 0..=3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OracleSpec {
    None,
    Dlcspecs {
        k: u16,
        n: u16,
        oracle_pubkeys: Vec<[u8; 32]>,
        event_template: EventTemplate,
    },
    Optimistic {
        window_blocks: u32,
        bond_sats: u64,
    },
    InternalTwap {
        window_blocks: u32,
        min_volume_usd: u64,
    },
}

impl OracleSpec {
    fn encode(&self, e: &mut Encoder) {
        match self {
            OracleSpec::None => e.put_u8(0),
            OracleSpec::Dlcspecs {
                k,
                n,
                oracle_pubkeys,
                event_template,
            } => {
                e.put_u8(1);
                e.put_u16(*k);
                e.put_u16(*n);
                e.put_bigsize(oracle_pubkeys.len() as u64);
                for pk in oracle_pubkeys {
                    e.put_bytes32(pk);
                }
                event_template.encode(e);
            }
            OracleSpec::Optimistic {
                window_blocks,
                bond_sats,
            } => {
                e.put_u8(2);
                e.put_u32(*window_blocks);
                e.put_u64(*bond_sats);
            }
            OracleSpec::InternalTwap {
                window_blocks,
                min_volume_usd,
            } => {
                e.put_u8(3);
                e.put_u32(*window_blocks);
                e.put_u64(*min_volume_usd);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlePrimitive {
    AtomicSwap,
    DlcTaproot,
    OptimisticClaim,
}

/// Spec 02 §2 — retain accrues to the common reserve and feeds the
/// capacity formula; service pays named per-settlement providers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeeSchedule {
    pub retain_bps: u16,
    pub service_bps: u16,
    pub fixed_sats: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LpStake {
    pub amount_sats: u64,
    /// Hash commitments to the slash-condition programs (spec 05).
    pub slash_conditions: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LivenessEscape {
    None,
    Tlock { beacon: [u8; 32], delay_s: u32 },
    DecayScript { params: Vec<u8> },
}

impl LivenessEscape {
    fn encode(&self, e: &mut Encoder) {
        match self {
            LivenessEscape::None => e.put_u8(0),
            LivenessEscape::Tlock { beacon, delay_s } => {
                e.put_u8(1);
                e.put_bytes32(beacon);
                e.put_u32(*delay_s);
            }
            LivenessEscape::DecayScript { params } => {
                e.put_u8(2);
                e.put_varbytes(params);
            }
        }
    }
}

/// Phase transitions a manifest declares automation tiers for
/// (spec 02 §5). Discriminants are the map keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Transition {
    Lock,
    Settle,
    Refund,
    Dispute,
}

/// Spec 02 §5 — where each transition's liveness comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationTier {
    Consensus = 1,
    AnyoneCanAct = 2,
    FederationCrypto = 3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RailManifest {
    pub spec_version: u16,
    pub direction: Direction,
    pub asset_id: [u8; 32],
    pub oracle_spec: OracleSpec,
    pub settle_primitive: SettlePrimitive,
    pub max_size_sats: u64,
    /// The LP's committed holding / DLC-maturity term, in blocks, for
    /// notes issued on this rail (spec 02 §2, spec 07 §5; maturity =
    /// LP term; 0 = redeem-only, no fixed maturity).
    pub committed_term: u32,
    pub fee_schedule: FeeSchedule,
    /// Max deviation (bps) of the settled/reimbursement price from
    /// the epoch reference marker. MUST be > 0 for rails drawing
    /// reserve reimbursement (spec 02 §2).
    pub price_dev_bound_bps: u16,
    pub quote_validity_s: u32,
    pub settle_window_s: u32,
    pub refund_delta_blocks: u32,
    pub lp_stake: LpStake,
    /// Commitment to the dispute program (spec 05); all-zero = none.
    pub dispute_hook: [u8; 32],
    pub liveness_escape: LivenessEscape,
    /// Sorted by `Transition` discriminant (spec 00 §3.3: maps as
    /// sorted (key, value) arrays).
    pub automation_tiers: Vec<(Transition, AutomationTier)>,
}

impl RailManifest {
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.put_u16(self.spec_version);
        e.put_u8(self.direction as u8);
        e.put_bytes32(&self.asset_id);
        self.oracle_spec.encode(&mut e);
        e.put_u8(self.settle_primitive as u8);
        e.put_u64(self.max_size_sats);
        e.put_u32(self.committed_term);
        e.put_u16(self.fee_schedule.retain_bps);
        e.put_u16(self.fee_schedule.service_bps);
        e.put_u64(self.fee_schedule.fixed_sats);
        e.put_u16(self.price_dev_bound_bps);
        e.put_u32(self.quote_validity_s);
        e.put_u32(self.settle_window_s);
        e.put_u32(self.refund_delta_blocks);
        e.put_u64(self.lp_stake.amount_sats);
        e.put_bigsize(self.lp_stake.slash_conditions.len() as u64);
        for c in &self.lp_stake.slash_conditions {
            e.put_bytes32(c);
        }
        e.put_bytes32(&self.dispute_hook);
        self.liveness_escape.encode(&mut e);
        let mut tiers = self.automation_tiers.clone();
        tiers.sort_by_key(|(t, _)| *t);
        e.put_bigsize(tiers.len() as u64);
        for (t, tier) in tiers {
            e.put_u8(t as u8);
            e.put_u8(tier as u8);
        }
        e.into_bytes()
    }

    /// Self-certifying identity: change any field, get another rail.
    pub fn rail_id(&self) -> [u8; 32] {
        tagged_hash(RAIL_ID_TAG, &self.canonical_encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rail0_fixture() -> RailManifest {
        RailManifest {
            spec_version: SPEC_VERSION,
            direction: Direction::Redeem,
            asset_id: [0x11; 32],
            oracle_spec: OracleSpec::None,
            settle_primitive: SettlePrimitive::AtomicSwap,
            max_size_sats: 5_000_000,
            committed_term: 4032,
            fee_schedule: FeeSchedule {
                retain_bps: 10,
                service_bps: 5,
                fixed_sats: 200,
            },
            price_dev_bound_bps: 100,
            quote_validity_s: 30,
            settle_window_s: 600,
            refund_delta_blocks: 144,
            lp_stake: LpStake {
                amount_sats: 10_000_000,
                slash_conditions: vec![[0x22; 32]],
            },
            dispute_hook: [0u8; 32],
            liveness_escape: LivenessEscape::None,
            automation_tiers: vec![
                (Transition::Settle, AutomationTier::AnyoneCanAct),
                (Transition::Refund, AutomationTier::Consensus),
            ],
        }
    }

    fn rail1_fixture() -> RailManifest {
        RailManifest {
            spec_version: SPEC_VERSION,
            direction: Direction::Redeem,
            asset_id: [0x11; 32],
            oracle_spec: OracleSpec::Dlcspecs {
                k: 1,
                n: 1,
                oracle_pubkeys: vec![[0x33; 32]],
                event_template: EventTemplate::btcusd_default(),
            },
            settle_primitive: SettlePrimitive::DlcTaproot,
            max_size_sats: 2_000_000,
            committed_term: 4032,
            fee_schedule: FeeSchedule {
                retain_bps: 10,
                service_bps: 30,
                fixed_sats: 500,
            },
            price_dev_bound_bps: 100,
            quote_validity_s: 5,
            settle_window_s: 60,
            refund_delta_blocks: 288,
            lp_stake: LpStake {
                amount_sats: 10_000_000,
                slash_conditions: vec![[0x22; 32]],
            },
            dispute_hook: [0x44; 32],
            liveness_escape: LivenessEscape::Tlock {
                beacon: [0x55; 32],
                delay_s: 600,
            },
            automation_tiers: vec![
                (Transition::Lock, AutomationTier::AnyoneCanAct),
                (Transition::Settle, AutomationTier::AnyoneCanAct),
                (Transition::Refund, AutomationTier::Consensus),
                (Transition::Dispute, AutomationTier::AnyoneCanAct),
            ],
        }
    }

    #[test]
    fn rail_id_is_deterministic_and_field_sensitive() {
        let m = rail0_fixture();
        assert_eq!(m.rail_id(), rail0_fixture().rail_id());

        let mut changed = rail0_fixture();
        changed.fee_schedule.retain_bps = 11;
        assert_ne!(
            m.rail_id(),
            changed.rail_id(),
            "any field change = new rail"
        );
    }

    #[test]
    fn automation_tier_order_does_not_matter() {
        let mut shuffled = rail0_fixture();
        shuffled.automation_tiers.reverse();
        assert_eq!(rail0_fixture().rail_id(), shuffled.rail_id());
    }

    /// Pinned vectors (spec 00 §3): these hex strings are the
    /// cross-language ground truth; the TS reference must reproduce
    /// them byte-for-byte.
    #[test]
    fn pinned_rail_id_vectors() {
        assert_eq!(
            hex::encode(rail0_fixture().rail_id()),
            "a415b3e100465775b4d521f34ab53d1d6c44a1ccc4f53d0f76f75fd50c06f9bb"
        );
        assert_eq!(
            hex::encode(rail1_fixture().rail_id()),
            "b571ed06f42eacf1326d56574221c4d1a782aa24cc9540439cf1a80965ab8dd4"
        );
    }
}
