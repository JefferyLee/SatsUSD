//! REDEEM_FAST_LOCK / LOCK_REFUND software verifiers + the intent↔lock binding
//! (PRD §5.D10, §5.D17 lock state machine, §8.2).
//!
//! The §5.D17 lock lifecycle is `active → consumed` (finalize) or
//! `active → refunded` (refund), tracked as SMT sets keyed by `lock_record_hash`
//! plus the redemption nullifier set:
//!
//! - LOCK: insert lock_record_hash into lock_record_root.
//! - REFUND: lock active (∈ record, ∉ consumed, ∉ refund); insert into refund.
//!
//! REDEEM_FAST_FINALIZE (the `active → consumed` half) is the §5.D12 claim
//! submit, implemented as the batched `claim::verify_reserve_claim_finalize` with
//! real oracle aggregation + SPV (ADR-0022). The single-redemption finalize that
//! used to live here was retired in favor of that path; this module keeps LOCK,
//! REFUND, and the shared [`check_intent_lock_binding`] helper.

use satusd_crypto::nums::tap_tweak;
use satusd_crypto::smt;
use satusd_crypto::state::state_root_hash;
use satusd_types::derive::{lock_record_hash, lock_tweak, redeem_intent_hash};
use satusd_types::types::{LockRecord, LockRefundRecord, RedeemIntent, StateRoot, TransitionType};

/// Minimum / maximum redemption amount in atoms (§18.3).
pub const MIN_REDEMPTION_ATOMS: u64 = 100;
pub const MAX_REDEMPTION_ATOMS: u64 = 1_000_000_000;
/// BTC HTLC claim-spend confirmation depth required to finalize (DL-22).
pub const BTC_CLAIM_CONFIRMATION_DEPTH: u32 = 6;
/// Regtest refund-safety delta in blocks (§18.3; network-dependent).
pub const REFUND_SAFETY_DELTA: u32 = 24;
/// `mode` value for an operator-routed fast redemption (§5.D2).
pub const MODE_FAST_OPERATOR: u8 = 0;
/// SMT "present" marker value for set membership.
pub const SET_MEMBER: [u8; 32] = [1u8; 32];

/// Why a redeem transition was rejected. Variants map to §13.1 R-tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedeemRejectReason {
    BadStateLinkage,
    WrongTransitionType,
    PostStateMismatch,
    Overflow,
    IntentHashMismatch,
    LockScriptKeyMismatch, // R-11
    AssetFamilyMismatch,
    AmountMismatch,
    AmountOutOfRange,
    WrongMode,
    OperatorIdMissing,
    CsvTooShort, // R-05
    PaymentHashMismatch,
    LineageInvalid, // M3 fake lineage
    LineageProofHashMismatch,
    LockAlreadyExists,
    LockNotActive,
    LockConsumed,     // R-09 (double finalize), R-17 (refund-after-finalize)
    LockRefunded,     // R-17 (finalize-after-refund)
    NullifierUsed,    // R-10
    PreimageMismatch, // R-12
    BurnSinkMismatch,
    BurnInternalKeyMismatch,
    BurnProofInvalid,
    FinalizeAmountMismatch,
    FinalizeOperatorMismatch,
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

/// `prev` advanced by one epoch with linkage fields set. Transition-specific
/// mutations are then applied to produce the new state; `verify_*` compares the
/// whole result to the claimed `new` (so linkage + immutability are covered).
fn next_state(prev: &StateRoot, ttype: TransitionType) -> StateRoot {
    let mut e = prev.clone();
    e.state_epoch = prev.state_epoch + 1;
    e.prev_state_root = state_root_hash(prev);
    e.transition_type = ttype.as_u8();
    e
}

/// `verify_*` = `apply_*` then whole-struct equality with the claimed `new`.
fn verified(
    expected: Result<StateRoot, RedeemRejectReason>,
    new: &StateRoot,
) -> Result<(), RedeemRejectReason> {
    match expected {
        Ok(e) if e == *new => Ok(()),
        Ok(_) => Err(RedeemRejectReason::PostStateMismatch),
        Err(e) => Err(e),
    }
}

/// §8.2 step 6.1 intent ↔ lock binding (shared by lock/finalize and the batch
/// ReserveClaim verifier).
pub(crate) fn check_intent_lock_binding(
    intent: &RedeemIntent,
    lock: &LockRecord,
    asset_family_id: &[u8; 32],
) -> Result<(), RedeemRejectReason> {
    use RedeemRejectReason::*;
    let rih = redeem_intent_hash(intent);
    ensure!(lock.redeem_intent_hash == rih, IntentHashMismatch);
    let expected_key = tap_tweak(
        &intent.user_asset_refund_key,
        &lock_tweak(&rih, &intent.payment_hash),
    );
    ensure!(lock.lock_script_key == expected_key, LockScriptKeyMismatch);
    ensure!(
        lock.asset_family_id == *asset_family_id,
        AssetFamilyMismatch
    );
    ensure!(
        lock.lock_amount_atoms == intent.amount_satusd_atoms,
        AmountMismatch
    );
    ensure!(
        (MIN_REDEMPTION_ATOMS..=MAX_REDEMPTION_ATOMS).contains(&intent.amount_satusd_atoms),
        AmountOutOfRange
    );
    ensure!(intent.mode == MODE_FAST_OPERATOR, WrongMode);
    ensure!(intent.operator_id.is_some(), OperatorIdMissing);
    ensure!(
        intent.asset_lock_csv_delta
            >= intent
                .btc_htlc_csv_delta
                .checked_add(REFUND_SAFETY_DELTA)
                .ok_or(Overflow)?,
        CsvTooShort
    );
    ensure!(
        lock.payment_hash == intent.payment_hash,
        PaymentHashMismatch
    );
    Ok(())
}

/// REDEEM_FAST_LOCK (§5.D10, §5.D15): submit a LockRecord into `lock_record_root`.
pub struct RedeemLockWitness {
    pub redeem_intent: RedeemIntent,
    pub lock_record: LockRecord,
    /// `lock_record_hash` ∉ prev.lock_record_root.
    pub lock_exclusion_proof: Vec<[u8; 32]>,
    /// TA lineage verifier result (DL-23, modeled as a fact this chunk).
    pub lineage_ok: bool,
    /// SHA256 of the verified lineage proof bytes, written into the LockRecord.
    pub lineage_proof_hash: [u8; 32],
}

/// Build the post-state for REDEEM_FAST_LOCK (executor).
pub fn apply_redeem_lock(
    prev: &StateRoot,
    w: &RedeemLockWitness,
) -> Result<StateRoot, RedeemRejectReason> {
    use RedeemRejectReason::*;
    check_intent_lock_binding(
        &w.redeem_intent,
        &w.lock_record,
        &prev.satusd_asset_family_id,
    )?;

    ensure!(w.lineage_ok, LineageInvalid);
    ensure!(
        w.lock_record.lineage_proof_hash == w.lineage_proof_hash,
        LineageProofHashMismatch
    );

    let lr_hash = lock_record_hash(&w.lock_record);
    ensure!(
        smt::verify_exclusion(&prev.lock_record_root, &lr_hash, &w.lock_exclusion_proof),
        LockAlreadyExists
    );

    let mut expected = next_state(prev, TransitionType::RedeemFastLock);
    expected.lock_record_root =
        smt::root_after_update(&lr_hash, &SET_MEMBER, &w.lock_exclusion_proof);
    Ok(expected)
}

pub fn verify_redeem_lock(
    prev: &StateRoot,
    new: &StateRoot,
    w: &RedeemLockWitness,
) -> Result<(), RedeemRejectReason> {
    verified(apply_redeem_lock(prev, w), new)
}

/// LOCK_REFUND (§5.D17 refund): active → refunded; the locked SatUSD returns to
/// the user (no supply/reserve change).
pub struct LockRefundWitness {
    pub redeem_intent: RedeemIntent,
    pub lock_record: LockRecord,
    pub lock_refund: LockRefundRecord,
    pub lock_membership_proof: Vec<[u8; 32]>,
    pub consumed_exclusion_proof: Vec<[u8; 32]>,
    pub refund_exclusion_proof: Vec<[u8; 32]>,
}

/// Build the post-state for LOCK_REFUND (executor).
pub fn apply_lock_refund(
    prev: &StateRoot,
    w: &LockRefundWitness,
) -> Result<StateRoot, RedeemRejectReason> {
    use RedeemRejectReason::*;
    check_intent_lock_binding(
        &w.redeem_intent,
        &w.lock_record,
        &prev.satusd_asset_family_id,
    )?;

    let lr_hash = lock_record_hash(&w.lock_record);
    ensure!(
        w.lock_refund.lock_record_hash == lr_hash,
        IntentHashMismatch
    );
    ensure!(
        smt::verify_inclusion(
            &prev.lock_record_root,
            &lr_hash,
            &SET_MEMBER,
            &w.lock_membership_proof
        ),
        LockNotActive
    );
    ensure!(
        smt::verify_exclusion(
            &prev.lock_consumed_root,
            &lr_hash,
            &w.consumed_exclusion_proof
        ),
        LockConsumed // R-17 (cannot refund an already-finalized lock)
    );
    ensure!(
        smt::verify_exclusion(&prev.lock_refund_root, &lr_hash, &w.refund_exclusion_proof),
        LockRefunded
    );

    let mut expected = next_state(prev, TransitionType::LockRefund);
    expected.lock_refund_root =
        smt::root_after_update(&lr_hash, &SET_MEMBER, &w.refund_exclusion_proof);
    Ok(expected)
}

pub fn verify_lock_refund(
    prev: &StateRoot,
    new: &StateRoot,
    w: &LockRefundWitness,
) -> Result<(), RedeemRejectReason> {
    verified(apply_lock_refund(prev, w), new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_crypto::nums::derive_nums_key;
    use satusd_crypto::smt::SparseMerkleTree;
    use satusd_types::derive::redeem_intent_hash as rih_of;
    use satusd_types::types::OutPoint;

    const PRICE_50K: u64 = 5_000_000_000_000;
    const FAMILY: [u8; 32] = [0x01; 32];
    const OPERATOR: [u8; 32] = [0x20; 32];
    const PREIMAGE: [u8; 32] = [0x55; 32];

    fn payment_hash() -> [u8; 32] {
        satusd_types::sha256(&[&PREIMAGE])
    }

    fn intent() -> RedeemIntent {
        RedeemIntent {
            version: 1,
            network: 0,
            redemption_id: [0x30; 32],
            satusd_asset_family_id: FAMILY,
            amount_satusd_atoms: 100_000_000, // $1M
            user_btc_refund_pubkey: [0x31; 32],
            user_btc_claim_pubkey: [0x32; 32],
            user_asset_refund_key: derive_nums_key("redeem-user-refund", &[]),
            operator_id: Some(OPERATOR),
            mode: MODE_FAST_OPERATOR,
            payment_hash: payment_hash(),
            asset_lock_csv_delta: 288,
            btc_htlc_csv_delta: 144,
            max_operator_fee_bps: 50,
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x33; 32],
            expiry_height: 900_000,
            nonce: [0x34; 32],
        }
    }

    fn lock_record(intent: &RedeemIntent) -> LockRecord {
        let rih = rih_of(intent);
        LockRecord {
            lock_record_version: 1,
            redeem_intent_hash: rih,
            lock_anchor_outpoint: OutPoint {
                txid: [0x40; 32],
                vout: 1,
            },
            lock_anchor_txid: [0x40; 32],
            lock_script_key: tap_tweak(
                &intent.user_asset_refund_key,
                &lock_tweak(&rih, &intent.payment_hash),
            ),
            lock_amount_atoms: intent.amount_satusd_atoms,
            asset_family_id: FAMILY,
            asset_lock_csv_delta: intent.asset_lock_csv_delta,
            payment_hash: intent.payment_hash,
            lineage_proof_hash: [0x99; 32],
            lineage_verified_by: vec![[0xaa; 32]],
            anchor_inclusion_height: 840_001,
        }
    }

    fn base_state(supply: u64, reserve: u64) -> StateRoot {
        let empty = SparseMerkleTree::new().root();
        StateRoot {
            protocol_version: 1,
            state_epoch: 10,
            prev_state_root: [0; 32],
            transition_type: 0,
            satusd_asset_family_id: FAMILY,
            sat_usd_supply_atoms: supply,
            reserve_btc_sats: reserve,
            reserved_pending_claim_sats: 0,
            collateral_ratio_ppm: 0,
            emergency_tier: 0,
            oracle_set_hash: [0x02; 32],
            oracle_set_epoch: 3,
            latest_oracle_epoch_seen: 3,
            latest_oracle_price_e8: PRICE_50K,
            reserve_committee_hash: [0x0d; 32],
            issuer_positions_root: [0x03; 32],
            operator_registry_root: [0x04; 32],
            lock_record_root: empty,
            lock_consumed_root: empty,
            lock_refund_root: empty,
            redemption_nullifier_root: empty,
            pending_claim_root: [0x09; 32],
            live_da_root: [0x0a; 32],
            archival_da_root: [0x0b; 32],
            l1_anchor_hash: [0x0c; 32],
            l1_anchor_height: 840_000,
            l1_anchor_mtp: 1_700_000_000,
            l1_anchor_chain_time: 1_700_000_600,
        }
    }

    /// A tree containing the given members (key=element, value=SET_MEMBER).
    fn set_tree(members: &[[u8; 32]]) -> SparseMerkleTree {
        let mut t = SparseMerkleTree::new();
        for m in members {
            t.insert(*m, &SET_MEMBER);
        }
        t
    }

    fn with_epoch(prev: &StateRoot, ttype: TransitionType) -> StateRoot {
        let mut n = prev.clone();
        n.state_epoch = prev.state_epoch + 1;
        n.prev_state_root = state_root_hash(prev);
        n.transition_type = ttype.as_u8();
        n
    }

    // ---- REDEEM_FAST_LOCK ----
    #[test]
    fn lock_happy_path() {
        let it = intent();
        let lr = lock_record(&it);
        let lr_hash = lock_record_hash(&lr);
        let prev = base_state(200_000_000, 4_000_000_000);
        let proof = set_tree(&[]).prove(&lr_hash); // exclusion against empty record set

        let mut new = with_epoch(&prev, TransitionType::RedeemFastLock);
        new.lock_record_root = smt::root_after_update(&lr_hash, &SET_MEMBER, &proof);

        let w = RedeemLockWitness {
            redeem_intent: it,
            lock_record: lr,
            lock_exclusion_proof: proof,
            lineage_ok: true,
            lineage_proof_hash: [0x99; 32],
        };
        verify_redeem_lock(&prev, &new, &w).expect("lock ok");
    }

    #[test]
    fn lock_fake_lineage_rejected() {
        let it = intent();
        let lr = lock_record(&it);
        let lr_hash = lock_record_hash(&lr);
        let prev = base_state(200_000_000, 4_000_000_000);
        let proof = set_tree(&[]).prove(&lr_hash);
        let new = with_epoch(&prev, TransitionType::RedeemFastLock);
        let w = RedeemLockWitness {
            redeem_intent: it,
            lock_record: lr,
            lock_exclusion_proof: proof,
            lineage_ok: false,
            lineage_proof_hash: [0x99; 32],
        };
        assert_eq!(
            verify_redeem_lock(&prev, &new, &w),
            Err(RedeemRejectReason::LineageInvalid)
        );
    }

    // ---- LOCK_REFUND ----
    #[test]
    fn refund_happy_path() {
        let it = intent();
        let lr = lock_record(&it);
        let lr_hash = lock_record_hash(&lr);
        let record_tree = set_tree(&[lr_hash]);
        let empty = set_tree(&[]);
        let mut prev = base_state(200_000_000, 4_000_000_000);
        prev.lock_record_root = record_tree.root();

        let refund_proof = empty.prove(&lr_hash);
        let mut new = with_epoch(&prev, TransitionType::LockRefund);
        new.lock_refund_root = smt::root_after_update(&lr_hash, &SET_MEMBER, &refund_proof);

        let w = LockRefundWitness {
            redeem_intent: it,
            lock_record: lr.clone(),
            lock_refund: LockRefundRecord {
                lock_record_hash: lr_hash,
                refund_anchor_txid: [0x80; 32],
                refund_anchor_outpoint: OutPoint {
                    txid: [0x80; 32],
                    vout: 0,
                },
                user_signature: [0x81; 64],
                refund_height: 850_000,
                asset_returned_to: lr.lock_script_key,
            },
            lock_membership_proof: record_tree.prove(&lr_hash),
            consumed_exclusion_proof: empty.prove(&lr_hash),
            refund_exclusion_proof: refund_proof,
        };
        verify_lock_refund(&prev, &new, &w).expect("refund ok");
    }

    #[test]
    fn refund_after_finalize_rejected() {
        // R-17: lock already consumed ⇒ cannot refund.
        let it = intent();
        let lr = lock_record(&it);
        let lr_hash = lock_record_hash(&lr);
        let record_tree = set_tree(&[lr_hash]);
        let consumed = set_tree(&[lr_hash]);
        let empty = set_tree(&[]);
        let mut prev = base_state(200_000_000, 4_000_000_000);
        prev.lock_record_root = record_tree.root();
        prev.lock_consumed_root = consumed.root();

        let new = with_epoch(&prev, TransitionType::LockRefund);
        let w = LockRefundWitness {
            redeem_intent: it,
            lock_record: lr.clone(),
            lock_refund: LockRefundRecord {
                lock_record_hash: lr_hash,
                refund_anchor_txid: [0x80; 32],
                refund_anchor_outpoint: OutPoint {
                    txid: [0x80; 32],
                    vout: 0,
                },
                user_signature: [0x81; 64],
                refund_height: 850_000,
                asset_returned_to: lr.lock_script_key,
            },
            lock_membership_proof: record_tree.prove(&lr_hash),
            consumed_exclusion_proof: consumed.prove(&lr_hash),
            refund_exclusion_proof: empty.prove(&lr_hash),
        };
        assert_eq!(
            verify_lock_refund(&prev, &new, &w),
            Err(RedeemRejectReason::LockConsumed)
        );
    }
}
