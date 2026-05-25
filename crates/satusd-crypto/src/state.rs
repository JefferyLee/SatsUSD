//! `state_root_hash` (PRD §6.1): `Poseidon(canonical_encode(StateRoot))`.
//!
//! Completes the field mapping deferred in ADR-0013. The Poseidon fold is the
//! §18.1 `hash_bytes` 31-byte-limb construction (ADR-0013), over the canonical
//! StateRoot encoding.

use satusd_types::{canonical_encode, types::StateRoot};

use crate::poseidon::hash_bytes_be;

/// `state_root_hash = Poseidon(canonical_encode(StateRoot))`, big-endian 32 bytes.
pub fn state_root_hash(state: &StateRoot) -> [u8; 32] {
    hash_bytes_be(&canonical_encode(state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StateRoot {
        StateRoot {
            protocol_version: 1,
            state_epoch: 7,
            prev_state_root: [0x11; 32],
            transition_type: 2,
            satusd_asset_family_id: [0x22; 32],
            sat_usd_supply_atoms: 100_000_000,
            reserve_btc_sats: 3_000_000_000,
            reserved_pending_claim_sats: 0,
            collateral_ratio_ppm: 1_500_000,
            emergency_tier: 0,
            oracle_set_hash: [0x33; 32],
            oracle_set_epoch: 4,
            latest_oracle_epoch_seen: 5,
            latest_oracle_price_e8: 5_000_000_000_000,
            reserve_committee_hash: [0x0d; 32],
            issuer_positions_root: [0x44; 32],
            operator_registry_root: [0x55; 32],
            lock_record_root: [0x66; 32],
            lock_consumed_root: [0x77; 32],
            lock_refund_root: [0x88; 32],
            redemption_nullifier_root: [0x99; 32],
            pending_claim_root: [0xaa; 32],
            live_da_root: [0xbb; 32],
            archival_da_root: [0xcc; 32],
            l1_anchor_hash: [0xdd; 32],
            l1_anchor_height: 840_000,
            l1_anchor_mtp: 1_700_000_000,
            l1_anchor_chain_time: 1_700_000_600,
        }
    }

    #[test]
    fn deterministic_and_field_sensitive() {
        let s = sample();
        let h = state_root_hash(&s);
        assert_eq!(h, state_root_hash(&s));
        let mut s2 = s.clone();
        s2.state_epoch += 1;
        assert_ne!(h, state_root_hash(&s2));
        println!("state_root_hash = 0x{}", hex::encode(h));
    }
}
