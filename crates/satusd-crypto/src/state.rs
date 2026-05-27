//! `state_root_hash` (PRD §6.1): `Poseidon(canonical_encode(StateRoot))`.
//!
//! Completes the field mapping deferred in ADR-0013. The Poseidon fold is the
//! §18.1 `hash_bytes` 31-byte-limb construction (ADR-0013), over the canonical
//! StateRoot encoding.

use ark_bn254::Fr;
use satusd_types::{canonical_encode, types::StateRoot};

use crate::poseidon::{fr_from_be_bytes, fr_to_be_bytes, hash_bytes_be, poseidon2};

/// `state_root_hash = Poseidon(canonical_encode(StateRoot))`, big-endian 32 bytes.
pub fn state_root_hash(state: &StateRoot) -> [u8; 32] {
    hash_bytes_be(&canonical_encode(state))
}

fn fe_scalar(x: u64) -> [u8; 32] {
    fr_to_be_bytes(&Fr::from(x))
}

/// A 32-byte value as two 128-bit big-endian limbs (hi, lo), each < 2^128 < Fr so
/// it fits regardless of how the value was derived (Poseidon or SHA256). A
/// Poseidon-derived root (which is < Fr) reconstructs exactly as `hi·2^128 + lo`.
fn fe_split(b: &[u8; 32]) -> [[u8; 32]; 2] {
    [
        fr_to_be_bytes(&fr_from_be_bytes(&b[0..16])),
        fr_to_be_bytes(&fr_from_be_bytes(&b[16..32])),
    ]
}

/// The StateRoot as the ordered BN254 field-element list the M7 monolithic
/// transition circuit commits (ADR-006): each scalar field directly, each 32-byte
/// field as two 128-bit limbs (hi, lo), in struct declaration order — 42 elements,
/// each 32-byte big-endian. This is the *circuit-friendly* representation; the lock
/// roots sit at fixed indices so the circuit can bind a fold output to
/// `hi·2^128 + lo` without spanning 31-byte limb boundaries (the reason a
/// field-level transition circuit was deferred from the byte-oriented
/// `state_root_hash` — ADR-006).
pub fn state_field_elements(s: &StateRoot) -> Vec<[u8; 32]> {
    let mut v: Vec<[u8; 32]> = Vec::with_capacity(42);
    let hash_field = |b: &[u8; 32], v: &mut Vec<[u8; 32]>| {
        let [hi, lo] = fe_split(b);
        v.push(hi);
        v.push(lo);
    };
    v.push(fe_scalar(s.protocol_version as u64));
    v.push(fe_scalar(s.state_epoch));
    hash_field(&s.prev_state_root, &mut v);
    v.push(fe_scalar(s.transition_type as u64));
    hash_field(&s.satusd_asset_family_id, &mut v);
    v.push(fe_scalar(s.sat_usd_supply_atoms));
    v.push(fe_scalar(s.reserve_btc_sats));
    v.push(fe_scalar(s.reserved_pending_claim_sats));
    v.push(fe_scalar(s.collateral_ratio_ppm));
    v.push(fe_scalar(s.emergency_tier as u64));
    hash_field(&s.oracle_set_hash, &mut v);
    v.push(fe_scalar(s.oracle_set_epoch));
    v.push(fe_scalar(s.latest_oracle_epoch_seen));
    v.push(fe_scalar(s.latest_oracle_price_e8));
    hash_field(&s.reserve_committee_hash, &mut v);
    hash_field(&s.issuer_positions_root, &mut v);
    hash_field(&s.operator_registry_root, &mut v);
    hash_field(&s.lock_record_root, &mut v);
    hash_field(&s.lock_consumed_root, &mut v);
    hash_field(&s.lock_refund_root, &mut v);
    hash_field(&s.redemption_nullifier_root, &mut v);
    hash_field(&s.pending_claim_root, &mut v);
    hash_field(&s.live_da_root, &mut v);
    hash_field(&s.archival_da_root, &mut v);
    hash_field(&s.l1_anchor_hash, &mut v);
    v.push(fe_scalar(s.l1_anchor_height as u64));
    v.push(fe_scalar(s.l1_anchor_mtp));
    v.push(fe_scalar(s.l1_anchor_chain_time));
    v
}

/// Poseidon-over-fields state commitment (ADR-006, M7): `acc_0 = 0`,
/// `acc_i = Poseidon2(acc_{i-1}, field_i)` over [`state_field_elements`].
/// Parallel to (not a replacement for) the byte-oriented `state_root_hash`.
pub fn state_commit_fields(s: &StateRoot) -> [u8; 32] {
    let mut acc = Fr::from(0u64);
    for fe in state_field_elements(s) {
        acc = poseidon2(acc, fr_from_be_bytes(&fe));
    }
    fr_to_be_bytes(&acc)
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

    #[test]
    fn field_commit_shape_and_sensitivity() {
        let s = sample();
        // 14 scalars + 14 × 2 limbs = 42 field elements (declaration order).
        assert_eq!(state_field_elements(&s).len(), 42);

        // lock_consumed_root sits at field indices [25, 26] as (hi, lo).
        let fes = state_field_elements(&s);
        assert_eq!(fes[25][16..32], s.lock_consumed_root[0..16]); // hi from high bytes
        assert_eq!(fes[26][16..32], s.lock_consumed_root[16..32]); // lo from low bytes

        let c = state_commit_fields(&s);
        assert_eq!(c, state_commit_fields(&s));
        let mut s2 = s.clone();
        s2.lock_consumed_root[0] ^= 0xff;
        assert_ne!(c, state_commit_fields(&s2));
    }
}
