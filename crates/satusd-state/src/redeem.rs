//! REDEEM_FAST_LOCK / REDEEM_FAST_FINALIZE / LOCK_REFUND software verifiers
//! (PRD §5.D10, §5.D17 lock state machine, §8.2).
//!
//! The §5.D17 lock lifecycle is `active → consumed` (finalize) or
//! `active → refunded` (refund), tracked as three SMT sets keyed by
//! `lock_record_hash` plus the redemption nullifier set:
//!
//! - LOCK: insert lock_record_hash into lock_record_root.
//! - FINALIZE: lock active (∈ record, ∉ consumed, ∉ refund) + nullifier unused;
//!   insert into consumed + nullifier; burn supply, debit reserve.
//! - REFUND: lock active; insert into refund.
//!
//! Scope (this chunk): single redemption per transition; the accounting + D17 SMT
//! state machine is exact. External proofs — TA lineage (DL-23), BTC payout SPV
//! confirmation (DL-22/D14), universe burn proof (D16), oracle aggregation (D7) —
//! are carried as already-verified witness facts; their cryptographic
//! establishment and multi-redemption batching are later chunks.

use satusd_crypto::nums::{protocol_burn_internal_key, protocol_sink_script_key, tap_tweak};
use satusd_crypto::smt;
use satusd_crypto::state::state_root_hash;
use satusd_types::derive::{
    lock_record_hash, lock_tweak, redeem_intent_hash, redemption_nullifier,
};
use satusd_types::tier;
use satusd_types::types::{
    BtcHtlcPayoutRecord, LockFinalizeRecord, LockRecord, LockRefundRecord, RedeemIntent, StateRoot,
    TransitionType,
};

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
const SET_MEMBER: [u8; 32] = [1u8; 32];

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
    LockConsumed,            // R-09 (double finalize), R-17 (refund-after-finalize)
    LockRefunded,            // R-17 (finalize-after-refund)
    NullifierUsed,           // R-10
    PreimageMismatch,        // R-12
    HtlcPaymentHashMismatch, // R-04
    HtlcClaimPubkeyMismatch, // R-03
    PayoutNotConfirmed,      // R-07, R-13 (depth < 6)
    PreimageNotRevealed,
    BurnSinkMismatch,
    BurnInternalKeyMismatch,
    BurnProofInvalid,
    FinalizeAmountMismatch,
    FinalizeOperatorMismatch,
    PayoutTooLow, // R-02
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

/// Linkage: new binds to prev, epoch advances by one, transition type matches.
fn check_linkage(
    prev: &StateRoot,
    new: &StateRoot,
    ttype: TransitionType,
) -> Result<(), RedeemRejectReason> {
    use RedeemRejectReason::*;
    ensure!(new.transition_type == ttype.as_u8(), WrongTransitionType);
    ensure!(
        new.prev_state_root == state_root_hash(prev),
        BadStateLinkage
    );
    ensure!(
        prev.state_epoch.checked_add(1) == Some(new.state_epoch),
        BadStateLinkage
    );
    Ok(())
}

/// `prev` with the linkage fields advanced; transition-specific mutations are
/// then applied and the whole struct compared to `new` (immutability backstop).
fn next_state(prev: &StateRoot, new: &StateRoot, ttype: TransitionType) -> StateRoot {
    let mut e = prev.clone();
    e.state_epoch = new.state_epoch;
    e.prev_state_root = new.prev_state_root;
    e.transition_type = ttype.as_u8();
    e
}

/// §8.2 step 6.1 intent ↔ lock binding (shared by lock/finalize).
fn check_intent_lock_binding(
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

pub fn verify_redeem_lock(
    prev: &StateRoot,
    new: &StateRoot,
    w: &RedeemLockWitness,
) -> Result<(), RedeemRejectReason> {
    use RedeemRejectReason::*;
    check_linkage(prev, new, TransitionType::RedeemFastLock)?;
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

    let mut expected = next_state(prev, new, TransitionType::RedeemFastLock);
    expected.lock_record_root =
        smt::root_after_update(&lr_hash, &SET_MEMBER, &w.lock_exclusion_proof);
    ensure!(*new == expected, PostStateMismatch);
    Ok(())
}

/// REDEEM_FAST_FINALIZE (§5.D17 finalize; §8.2): active → consumed + nullifier,
/// burn supply, debit reserve by the gross BTC paid out.
pub struct RedeemFinalizeWitness {
    pub redeem_intent: RedeemIntent,
    pub lock_record: LockRecord,
    pub lock_finalize: LockFinalizeRecord,
    pub btc_htlc: BtcHtlcPayoutRecord,
    pub operator_id: [u8; 32],
    pub price_e8: u64,
    pub lock_membership_proof: Vec<[u8; 32]>,
    pub consumed_exclusion_proof: Vec<[u8; 32]>,
    pub refund_exclusion_proof: Vec<[u8; 32]>,
    pub nullifier_exclusion_proof: Vec<[u8; 32]>,
    /// Universe burn proof verifier result (D16, modeled as a fact this chunk).
    pub burn_proof_ok: bool,
}

pub fn verify_redeem_finalize(
    prev: &StateRoot,
    new: &StateRoot,
    w: &RedeemFinalizeWitness,
) -> Result<(), RedeemRejectReason> {
    use RedeemRejectReason::*;
    check_linkage(prev, new, TransitionType::RedeemFastFinalize)?;
    check_intent_lock_binding(
        &w.redeem_intent,
        &w.lock_record,
        &prev.satusd_asset_family_id,
    )?;
    ensure!(
        w.redeem_intent.operator_id == Some(w.operator_id),
        FinalizeOperatorMismatch
    );

    // §5.D17: the lock must be active (∈ record, ∉ consumed, ∉ refund).
    let lr_hash = lock_record_hash(&w.lock_record);
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
        LockConsumed // R-09
    );
    ensure!(
        smt::verify_exclusion(&prev.lock_refund_root, &lr_hash, &w.refund_exclusion_proof),
        LockRefunded // R-17
    );

    // Nullifier must be unused.
    let nf = redemption_nullifier(
        &w.lock_record.lock_anchor_outpoint,
        &w.lock_record.lock_script_key,
        &w.lock_record.redeem_intent_hash,
    );
    ensure!(
        smt::verify_exclusion(
            &prev.redemption_nullifier_root,
            &nf,
            &w.nullifier_exclusion_proof
        ),
        NullifierUsed
    );

    // Preimage ↔ HTLC ↔ intent.
    ensure!(
        satusd_types::sha256(&[&w.lock_finalize.payment_preimage]) == w.redeem_intent.payment_hash,
        PreimageMismatch // R-12
    );
    ensure!(
        w.btc_htlc.payment_hash == w.redeem_intent.payment_hash,
        HtlcPaymentHashMismatch
    );
    ensure!(
        w.btc_htlc.user_claim_pubkey == w.redeem_intent.user_btc_claim_pubkey,
        HtlcClaimPubkeyMismatch
    );

    // BTC payout confirmed ≥ 6 blocks (DL-22), preimage revealed on-chain.
    ensure!(
        w.btc_htlc.confirmation_depth >= BTC_CLAIM_CONFIRMATION_DEPTH,
        PayoutNotConfirmed // R-07, R-13
    );
    ensure!(
        w.btc_htlc.revealed_preimage == w.lock_finalize.payment_preimage,
        PreimageNotRevealed
    );

    // Protocol burn sink (D16) + burn proof.
    ensure!(
        w.lock_finalize.protocol_sink_script_key
            == protocol_sink_script_key(&prev.satusd_asset_family_id),
        BurnSinkMismatch
    );
    ensure!(
        w.lock_finalize.protocol_burn_internal_key
            == protocol_burn_internal_key(&prev.satusd_asset_family_id),
        BurnInternalKeyMismatch
    );
    ensure!(w.burn_proof_ok, BurnProofInvalid);

    // Finalize consistency.
    ensure!(
        w.lock_finalize.finalized_amount_atoms == w.lock_record.lock_amount_atoms,
        FinalizeAmountMismatch
    );
    ensure!(
        w.lock_finalize.operator_id == w.operator_id,
        FinalizeOperatorMismatch
    );

    // Payout amount (§8.2 6.10): gross = amount * 10^14 / price; fee ≤ bps.
    let gross: u128 = (w.lock_record.lock_amount_atoms as u128)
        .checked_mul(10u128.pow(14))
        .ok_or(Overflow)?
        / (w.price_e8 as u128);
    let max_fee = gross
        .checked_mul(w.redeem_intent.max_operator_fee_bps as u128)
        .ok_or(Overflow)?
        / 10_000;
    let expected_user_payout = gross - max_fee;
    ensure!(
        w.btc_htlc.payout_sats as u128 >= expected_user_payout,
        PayoutTooLow // R-02
    );
    let gross_sats = u64::try_from(gross).map_err(|_| Overflow)?;

    // Accounting + root updates, compared as a whole post-state.
    let amount = w.redeem_intent.amount_satusd_atoms;
    let new_supply = prev
        .sat_usd_supply_atoms
        .checked_sub(amount)
        .ok_or(Overflow)?;
    let new_reserve = prev
        .reserve_btc_sats
        .checked_sub(gross_sats)
        .ok_or(Overflow)?;

    let mut expected = next_state(prev, new, TransitionType::RedeemFastFinalize);
    expected.sat_usd_supply_atoms = new_supply;
    expected.reserve_btc_sats = new_reserve;
    expected.latest_oracle_price_e8 = w.price_e8;
    expected.collateral_ratio_ppm =
        tier::collateral_ratio_ppm(new_reserve, new_supply, w.price_e8).unwrap_or(0);
    expected.emergency_tier = tier::recompute_tier(new_reserve, new_supply, w.price_e8).as_u8();
    expected.lock_consumed_root =
        smt::root_after_update(&lr_hash, &SET_MEMBER, &w.consumed_exclusion_proof);
    expected.redemption_nullifier_root =
        smt::root_after_update(&nf, &SET_MEMBER, &w.nullifier_exclusion_proof);
    ensure!(*new == expected, PostStateMismatch);
    Ok(())
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

pub fn verify_lock_refund(
    prev: &StateRoot,
    new: &StateRoot,
    w: &LockRefundWitness,
) -> Result<(), RedeemRejectReason> {
    use RedeemRejectReason::*;
    check_linkage(prev, new, TransitionType::LockRefund)?;
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

    let mut expected = next_state(prev, new, TransitionType::LockRefund);
    expected.lock_refund_root =
        smt::root_after_update(&lr_hash, &SET_MEMBER, &w.refund_exclusion_proof);
    ensure!(*new == expected, PostStateMismatch);
    Ok(())
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

    fn finalize_record(lr: &LockRecord) -> LockFinalizeRecord {
        LockFinalizeRecord {
            lock_record_hash: lock_record_hash(lr),
            payment_preimage: PREIMAGE,
            finalize_anchor_txid: [0x50; 32],
            finalize_anchor_outpoint: OutPoint {
                txid: [0x50; 32],
                vout: 0,
            },
            protocol_sink_script_key: protocol_sink_script_key(&FAMILY),
            protocol_burn_internal_key: protocol_burn_internal_key(&FAMILY),
            finalized_amount_atoms: lr.lock_amount_atoms,
            operator_id: OPERATOR,
            finalize_height: 840_002,
            universe_burn_proof_hash: [0x60; 32],
        }
    }

    fn htlc(intent: &RedeemIntent) -> BtcHtlcPayoutRecord {
        BtcHtlcPayoutRecord {
            operator_id: OPERATOR,
            redeem_intent_hash: rih_of(intent),
            btc_htlc_txid: [0x70; 32],
            btc_htlc_vout: 0,
            payment_hash: intent.payment_hash,
            user_claim_pubkey: intent.user_btc_claim_pubkey,
            operator_refund_pubkey: [0x71; 32],
            payout_sats: 1_990_000_000, // ≥ gross(2e9) − 0.5% fee
            btc_csv_delta: intent.btc_htlc_csv_delta,
            htlc_inclusion_height: 840_000,
            htlc_inclusion_block_hash: [0x72; 32],
            claim_spend_txid: [0x73; 32],
            revealed_preimage: PREIMAGE,
            claim_inclusion_height: 840_000,
            claim_inclusion_block_hash: [0x74; 32],
            confirmation_depth: 6,
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

    // ---- REDEEM_FAST_FINALIZE ----
    /// Build a finalize witness + the matching honest post-state.
    fn finalize_setup(prev: &StateRoot) -> (RedeemFinalizeWitness, StateRoot, [u8; 32]) {
        let it = intent();
        let lr = lock_record(&it);
        let lr_hash = lock_record_hash(&lr);
        let nf = redemption_nullifier(
            &lr.lock_anchor_outpoint,
            &lr.lock_script_key,
            &lr.redeem_intent_hash,
        );
        // lock_record_root contains the lock; the other sets are empty.
        let record_tree = set_tree(&[lr_hash]);
        let empty = set_tree(&[]);
        let w = RedeemFinalizeWitness {
            redeem_intent: it.clone(),
            lock_record: lr,
            lock_finalize: finalize_record(&lock_record(&it)),
            btc_htlc: htlc(&it),
            operator_id: OPERATOR,
            price_e8: PRICE_50K,
            lock_membership_proof: record_tree.prove(&lr_hash),
            consumed_exclusion_proof: empty.prove(&lr_hash),
            refund_exclusion_proof: empty.prove(&lr_hash),
            nullifier_exclusion_proof: empty.prove(&nf),
            burn_proof_ok: true,
        };
        // prev must hold the lock in its record root.
        let mut p = prev.clone();
        p.lock_record_root = record_tree.root();
        let gross_sats = 2_000_000_000u64; // 1e8 atoms * 1e14 / 5e12
        let new_supply = p.sat_usd_supply_atoms - it.amount_satusd_atoms;
        let new_reserve = p.reserve_btc_sats - gross_sats;
        let mut new = with_epoch(&p, TransitionType::RedeemFastFinalize);
        new.sat_usd_supply_atoms = new_supply;
        new.reserve_btc_sats = new_reserve;
        new.collateral_ratio_ppm =
            tier::collateral_ratio_ppm(new_reserve, new_supply, PRICE_50K).unwrap_or(0);
        new.emergency_tier = tier::recompute_tier(new_reserve, new_supply, PRICE_50K).as_u8();
        new.lock_consumed_root =
            smt::root_after_update(&lr_hash, &SET_MEMBER, &w.consumed_exclusion_proof);
        new.redemption_nullifier_root =
            smt::root_after_update(&nf, &SET_MEMBER, &w.nullifier_exclusion_proof);
        (w, new, p.lock_record_root)
    }

    #[test]
    fn finalize_happy_path() {
        let prev = base_state(200_000_000, 4_000_000_000);
        let (w, new, record_root) = finalize_setup(&prev);
        let mut p = prev;
        p.lock_record_root = record_root;
        verify_redeem_finalize(&p, &new, &w).expect("finalize ok");
    }

    #[test]
    fn finalize_bad_preimage_rejected() {
        let prev = base_state(200_000_000, 4_000_000_000);
        let (mut w, new, record_root) = finalize_setup(&prev);
        w.lock_finalize.payment_preimage = [0x00; 32]; // sha256 ≠ payment_hash
        let mut p = prev;
        p.lock_record_root = record_root;
        assert_eq!(
            verify_redeem_finalize(&p, &new, &w),
            Err(RedeemRejectReason::PreimageMismatch)
        );
    }

    #[test]
    fn finalize_shallow_confirmation_rejected() {
        let prev = base_state(200_000_000, 4_000_000_000);
        let (mut w, new, record_root) = finalize_setup(&prev);
        w.btc_htlc.confirmation_depth = 5; // < 6 (R-07/R-13)
        let mut p = prev;
        p.lock_record_root = record_root;
        assert_eq!(
            verify_redeem_finalize(&p, &new, &w),
            Err(RedeemRejectReason::PayoutNotConfirmed)
        );
    }

    #[test]
    fn finalize_double_spend_rejected() {
        // R-09: lock already in consumed set ⇒ non-membership fails.
        let prev = base_state(200_000_000, 4_000_000_000);
        let (mut w, mut new, record_root) = finalize_setup(&prev);
        let lr_hash = lock_record_hash(&w.lock_record);
        let consumed = set_tree(&[lr_hash]);
        w.consumed_exclusion_proof = consumed.prove(&lr_hash); // proves *presence*, not absence
        let mut p = prev;
        p.lock_record_root = record_root;
        p.lock_consumed_root = consumed.root();
        new.prev_state_root = state_root_hash(&p); // re-link after mutating prev
        assert_eq!(
            verify_redeem_finalize(&p, &new, &w),
            Err(RedeemRejectReason::LockConsumed)
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
