//! REDEEM_FAST_FINALIZE batched into a ReserveClaim (§5.D13, §6.8, §8.2 step 6).
//!
//! Wraps N single-redemption finalizes (§5.D17) into one claim:
//! - real oracle aggregation (D7) replaces the per-redemption `price_e8` fact;
//! - real BTC SPV payout confirmation (D14) replaces the `confirmation_depth` fact;
//! - the D17 lock-consumed and redemption-nullifier sets are advanced with
//!   *running* roots, so multiple redemptions chain within one transition.
//!
//! This is the **submit_claim** half of the §5.D12 two-phase flow: it burns the
//! redeemed supply, consumes the locks, *reserves* the reimbursement
//! (`reserved_pending_claim_sats += reimbursement`) and inserts a PENDING
//! `PendingClaim` keyed by `claim_id` — it does NOT debit `reserve_btc_sats`.
//! `reserve_btc_sats` is debited later by finalize_claim/settle, or the
//! reservation is freed by `registry::verify_reclaim_stale_claim` if the claim
//! expires. CR/tier use the full reserve (§5.D8), so CR rises at submit and
//! falls back at settle.
//!
//! The universe burn proof (D16, #31) is independently re-verified here
//! ([`check_burn_proof_binding`]): each redemption's finalize/burn `proof.File`
//! is checked to commit the asset to the protocol NUMS sink for the claimed
//! amount/anchor — it is not a trust-anchored witness fact.

use satusd_crypto::nums::{protocol_burn_internal_key, protocol_sink_script_key};
use satusd_crypto::poseidon::batch_root_be;
use satusd_crypto::smt;
use satusd_crypto::state::state_root_hash;
use satusd_ta_proof::{parse_proof_file, verify_lineage};
use satusd_types::derive::{
    claim_id, lineage_proof_hash, lock_record_hash, pending_claim_hash, redemption_nullifier,
};
use satusd_types::tier;
use satusd_types::types::{
    BtcPayoutConfirmation, LockFinalizeRecord, LockRecord, OracleMessage, PendingClaim,
    PendingClaimStatus, RedeemIntent, ReserveClaim, StateRoot, TransitionType,
};

use crate::oracle::{self, OracleParams};
use crate::redeem::{
    check_intent_lock_binding, RedeemRejectReason, BTC_CLAIM_CONFIRMATION_DEPTH, SET_MEMBER,
};
use crate::spv;

/// `chain_time = l1_anchor_mtp + MTP_LAG_OFFSET` (§5.D6, DL-26).
pub const MTP_LAG_OFFSET_SEC: u64 = 3600;

/// One redemption inside a batch (the §5.D17 finalize inputs, SPV-confirmed).
pub struct BatchRedemption {
    pub redeem_intent: RedeemIntent,
    pub lock_record: LockRecord,
    pub lock_finalize: LockFinalizeRecord,
    pub payout_confirmation: BtcPayoutConfirmation,
    /// `lock_record_hash` ∈ prev.lock_record_root.
    pub lock_membership_proof: Vec<[u8; 32]>,
    /// `lock_record_hash` ∉ the *running* consumed root.
    pub consumed_exclusion_proof: Vec<[u8; 32]>,
    /// `lock_record_hash` ∉ prev.lock_refund_root.
    pub refund_exclusion_proof: Vec<[u8; 32]>,
    /// nullifier ∉ the *running* nullifier root.
    pub nullifier_exclusion_proof: Vec<[u8; 32]>,
    /// The full tapd `proof.File` for the finalize/burn output — the universe
    /// burn proof (D16), independently re-verified here.
    pub burn_proof: Vec<u8>,
}

/// ReserveClaim REDEEM_FAST_FINALIZE witness: the claim, its redemptions, and the
/// oracle quorum whose median fixes the price for the whole batch.
pub struct ReserveClaimWitness {
    pub claim: ReserveClaim,
    pub redemptions: Vec<BatchRedemption>,
    pub oracle_messages: Vec<OracleMessage>,
    pub oracle_signer_set: Vec<[u8; 32]>,
    /// `claim_id` ∉ prev.pending_claim_root (DL-19 idempotency: no double-submit).
    pub pending_claim_exclusion_proof: Vec<[u8; 32]>,
    /// Best-chain tip height (local bitcoind), for SPV burial depth.
    pub btc_tip_height: u32,
}

/// Why a ReserveClaim was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimRejectReason {
    WrongTransitionType,
    BadStateLinkage,
    ClaimLinkageMismatch,
    ChainTimeInconsistent,
    Oracle(oracle::OracleRejectReason),
    OraclePriceMismatch,
    Spv(spv::SpvError),
    Redeem(RedeemRejectReason),
    EmptyBatch,
    DuplicateInBatch,
    OperatorMismatch,
    PreimageNotRevealed,
    PayoutTooLow,
    ReimbursementMismatch,
    RedemptionBatchRootMismatch,
    ClaimIdMismatch,
    DuplicateClaim, // DL-19: claim_id already pending
    PostStateMismatch,
    Overflow,
    BurnProofMalformed,
    BurnProofInvalid,
    BurnProofScriptKeyMismatch, // R-16: burn output not committed to the NUMS sink
    BurnProofAmountMismatch,
    BurnProofFamilyMismatch,
    BurnProofAnchorMismatch,
    BurnProofHashMismatch,
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

/// §5.D16 (DL-29) universe burn proof binding. Independently re-verify the tapd
/// `proof.File` for the finalize output and bind its head — the burn output — to
/// the `LockFinalizeRecord`. Together with the NUMS-sink derivation checks (which
/// pin `protocol_sink_script_key` to `SHA256("SATUSD_BURN_SINK_V1" || family)`),
/// this proves the SatUSD was provably burned: committed on-chain to an
/// unspendable NUMS sink, for the claimed amount, in the claimed anchor output.
///
/// `chain_id` is the network byte for the asset-family id (§5.D1).
fn check_burn_proof_binding(
    burn_proof_bytes: &[u8],
    fin: &LockFinalizeRecord,
    asset_family_id: &[u8; 32],
    chain_id: u8,
) -> Result<(), ClaimRejectReason> {
    use ClaimRejectReason::*;
    let file = parse_proof_file(burn_proof_bytes).map_err(|_| BurnProofMalformed)?;
    let proofs = file.parsed().map_err(|_| BurnProofMalformed)?;
    // The whole genesis→burn lineage is anchored + every asset committed.
    let head = verify_lineage(&proofs).map_err(|_| BurnProofInvalid)?;

    let head_leaf = proofs
        .last()
        .ok_or(BurnProofInvalid)?
        .asset_leaf()
        .map_err(|_| BurnProofMalformed)?;

    // The burn output commits the asset to the protocol NUMS sink script key
    // (x-only); `protocol_sink_script_key` is separately pinned to the derived
    // NUMS key by the caller, so this proves the burn went to the real sink.
    ensure!(head_leaf.script_key.len() == 33, BurnProofMalformed);
    ensure!(
        head_leaf.script_key[1..33] == fin.protocol_sink_script_key,
        BurnProofScriptKeyMismatch
    );
    ensure!(
        head_leaf.amount == fin.finalized_amount_atoms,
        BurnProofAmountMismatch
    );

    // The burned asset is this SatUSD family.
    let group_key: [u8; 33] = head_leaf
        .group_key
        .and_then(|g| <[u8; 33]>::try_from(g).ok())
        .ok_or(BurnProofFamilyMismatch)?;
    let family =
        satusd_types::derive::asset_family_id(&head_leaf.genesis.asset_id(), &group_key, chain_id);
    ensure!(family == *asset_family_id, BurnProofFamilyMismatch);

    // The verified head IS the claimed finalize anchor output.
    ensure!(
        head.txid == fin.finalize_anchor_outpoint.txid
            && head.output_index == fin.finalize_anchor_outpoint.vout,
        BurnProofAnchorMismatch
    );

    // The LockFinalizeRecord commits the verified burn proof's hash.
    ensure!(
        lineage_proof_hash(burn_proof_bytes) == fin.universe_burn_proof_hash,
        BurnProofHashMismatch
    );
    Ok(())
}

/// Verify a batched REDEEM_FAST_FINALIZE ReserveClaim takes `prev` → `new`.
pub fn verify_reserve_claim_finalize(
    prev: &StateRoot,
    new: &StateRoot,
    w: &ReserveClaimWitness,
) -> Result<(), ClaimRejectReason> {
    use ClaimRejectReason::*;
    let claim = &w.claim;
    let ttype = TransitionType::RedeemFastFinalize.as_u8();

    // ---- 1. claim ↔ state linkage ----
    ensure!(new.transition_type == ttype, WrongTransitionType);
    ensure!(new.state_epoch == prev.state_epoch + 1, BadStateLinkage);
    let prev_hash = state_root_hash(prev);
    ensure!(new.prev_state_root == prev_hash, BadStateLinkage);
    ensure!(claim.transition_type == ttype, ClaimLinkageMismatch);
    ensure!(claim.prev_state_root == prev_hash, ClaimLinkageMismatch);
    ensure!(
        claim.new_state_root == state_root_hash(new),
        ClaimLinkageMismatch
    );

    // ---- 2. oracle aggregation (D7) → batch price ----
    let chain_time = prev
        .l1_anchor_mtp
        .checked_add(MTP_LAG_OFFSET_SEC)
        .ok_or(Overflow)?;
    ensure!(
        claim.l1_anchor.l1_anchor_chain_time == chain_time,
        ChainTimeInconsistent
    );
    oracle::verify_signatures(&w.oracle_messages).map_err(Oracle)?;
    let params = OracleParams {
        expected_oracle_set_epoch: prev.oracle_set_epoch,
        expected_oracle_set_hash: prev.oracle_set_hash,
        chain_time_sec: chain_time,
        max_epoch_lag_sec: claim.l1_anchor.max_epoch_lag_sec,
        oracle_future_tolerance_sec: claim.l1_anchor.oracle_future_tolerance,
        signer_set: &w.oracle_signer_set,
    };
    let price = oracle::aggregate(&w.oracle_messages, &params).map_err(Oracle)?;
    ensure!(
        claim.l1_anchor.selected_oracle_price_e8 == price,
        OraclePriceMismatch
    );

    // ---- 3. per-redemption loop with running D17 roots ----
    ensure!(!w.redemptions.is_empty(), EmptyBatch);
    let mut consumed_root = prev.lock_consumed_root;
    let mut nullifier_root = prev.redemption_nullifier_root;
    let mut total_amount: u128 = 0;
    let mut total_gross: u128 = 0;
    let mut leaves: Vec<[u8; 32]> = Vec::with_capacity(w.redemptions.len());
    let mut seen: Vec<[u8; 32]> = Vec::with_capacity(w.redemptions.len());

    for r in &w.redemptions {
        check_intent_lock_binding(
            &r.redeem_intent,
            &r.lock_record,
            &prev.satusd_asset_family_id,
        )
        .map_err(Redeem)?;
        ensure!(
            r.redeem_intent.operator_id == Some(claim.operator_id),
            OperatorMismatch
        );

        let lr_hash = lock_record_hash(&r.lock_record);
        ensure!(!seen.contains(&lr_hash), DuplicateInBatch);
        seen.push(lr_hash);

        // §5.D17 active: ∈ record (immutable here), ∉ consumed (running), ∉ refund.
        ensure!(
            smt::verify_inclusion(
                &prev.lock_record_root,
                &lr_hash,
                &SET_MEMBER,
                &r.lock_membership_proof
            ),
            Redeem(RedeemRejectReason::LockNotActive)
        );
        ensure!(
            smt::verify_exclusion(&consumed_root, &lr_hash, &r.consumed_exclusion_proof),
            Redeem(RedeemRejectReason::LockConsumed)
        );
        ensure!(
            smt::verify_exclusion(&prev.lock_refund_root, &lr_hash, &r.refund_exclusion_proof),
            Redeem(RedeemRejectReason::LockRefunded)
        );

        let nf = redemption_nullifier(
            &r.lock_record.lock_anchor_outpoint,
            &r.lock_record.lock_script_key,
            &r.lock_record.redeem_intent_hash,
        );
        ensure!(
            smt::verify_exclusion(&nullifier_root, &nf, &r.nullifier_exclusion_proof),
            Redeem(RedeemRejectReason::NullifierUsed)
        );

        // Preimage ↔ payment_hash (also checked by SPV); preimage matches finalize.
        ensure!(
            satusd_types::sha256(&[&r.lock_finalize.payment_preimage])
                == r.redeem_intent.payment_hash,
            Redeem(RedeemRejectReason::PreimageMismatch)
        );
        ensure!(
            r.payout_confirmation.revealed_preimage == r.lock_finalize.payment_preimage,
            PreimageNotRevealed
        );
        // BTC SPV payout confirmation (D14) — real, replaces the depth fact.
        spv::verify_payout_confirmation(
            &r.payout_confirmation,
            &r.redeem_intent.payment_hash,
            BTC_CLAIM_CONFIRMATION_DEPTH as usize,
            w.btc_tip_height,
        )
        .map_err(Spv)?;

        // Protocol burn sink (D16): the claimed sink/internal keys must be the
        // NUMS keys derived from the asset family (no leakable private key)...
        ensure!(
            r.lock_finalize.protocol_sink_script_key
                == protocol_sink_script_key(&prev.satusd_asset_family_id),
            Redeem(RedeemRejectReason::BurnSinkMismatch)
        );
        ensure!(
            r.lock_finalize.protocol_burn_internal_key
                == protocol_burn_internal_key(&prev.satusd_asset_family_id),
            Redeem(RedeemRejectReason::BurnInternalKeyMismatch)
        );
        // ...and the universe burn proof must independently show the asset was
        // committed on-chain to that sink for this amount (D16, replaces the fact).
        check_burn_proof_binding(
            &r.burn_proof,
            &r.lock_finalize,
            &prev.satusd_asset_family_id,
            r.redeem_intent.network,
        )?;

        // Finalize consistency.
        ensure!(
            r.lock_finalize.finalized_amount_atoms == r.lock_record.lock_amount_atoms,
            Redeem(RedeemRejectReason::FinalizeAmountMismatch)
        );
        ensure!(
            r.lock_finalize.operator_id == claim.operator_id,
            Redeem(RedeemRejectReason::FinalizeOperatorMismatch)
        );

        // Payout amount (§8.2 6.10): gross = amount * 10^14 / price; fee ≤ bps.
        let gross: u128 = (r.lock_record.lock_amount_atoms as u128)
            .checked_mul(10u128.pow(14))
            .ok_or(Overflow)?
            / (price as u128);
        let max_fee = gross
            .checked_mul(r.redeem_intent.max_operator_fee_bps as u128)
            .ok_or(Overflow)?
            / 10_000;
        ensure!(
            r.payout_confirmation.htlc_output_value_sats as u128 >= gross - max_fee,
            PayoutTooLow
        );

        total_amount = total_amount
            .checked_add(r.redeem_intent.amount_satusd_atoms as u128)
            .ok_or(Overflow)?;
        total_gross = total_gross.checked_add(gross).ok_or(Overflow)?;
        leaves.push(lr_hash);

        // Advance the running consumed / nullifier roots.
        consumed_root = smt::root_after_update(&lr_hash, &SET_MEMBER, &r.consumed_exclusion_proof);
        nullifier_root = smt::root_after_update(&nf, &SET_MEMBER, &r.nullifier_exclusion_proof);
    }

    // ---- 4. aggregate accounting (§5.D12 submit_claim) ----
    // Two-phase: submit *reserves* the reimbursement and creates a PENDING claim;
    // `reserve_btc_sats` is debited later by finalize_claim/settle. CR/tier use the
    // full reserve (§5.D8 formula), so CR rises here and falls back at settle.
    let total_amount_u64 = u64::try_from(total_amount).map_err(|_| Overflow)?;
    let total_gross_u64 = u64::try_from(total_gross).map_err(|_| Overflow)?;
    let new_supply = prev
        .sat_usd_supply_atoms
        .checked_sub(total_amount_u64)
        .ok_or(Overflow)?;
    let new_reserved = prev
        .reserved_pending_claim_sats
        .checked_add(total_gross_u64)
        .ok_or(Overflow)?;
    ensure!(
        claim.reimbursement_sats == total_gross_u64,
        ReimbursementMismatch
    );
    ensure!(
        claim.redemption_batch_root == batch_root_be(&leaves),
        RedemptionBatchRootMismatch
    );

    // PENDING claim, keyed by claim_id (ADR-0022: derived over inputs only).
    let id = claim_id(claim);
    ensure!(claim.claim_id == id, ClaimIdMismatch);
    ensure!(
        smt::verify_exclusion(
            &prev.pending_claim_root,
            &id,
            &w.pending_claim_exclusion_proof
        ),
        DuplicateClaim
    );
    let pending = PendingClaim {
        claim_id: id,
        operator_id: claim.operator_id,
        reserved_sats: total_gross_u64,
        claim_created_height: claim.l1_anchor.l1_anchor_height,
        claim_expiry_height: claim.claim_expiry_height,
        status: PendingClaimStatus::Pending,
    };

    // ---- 5. whole-state comparison (covers linkage + immutability) ----
    let mut expected = prev.clone();
    expected.state_epoch = prev.state_epoch + 1;
    expected.prev_state_root = prev_hash;
    expected.transition_type = ttype;
    expected.sat_usd_supply_atoms = new_supply;
    expected.reserved_pending_claim_sats = new_reserved;
    expected.latest_oracle_price_e8 = price;
    expected.collateral_ratio_ppm =
        tier::collateral_ratio_ppm(prev.reserve_btc_sats, new_supply, price).unwrap_or(0);
    expected.emergency_tier =
        tier::recompute_tier(prev.reserve_btc_sats, new_supply, price).as_u8();
    expected.lock_consumed_root = consumed_root;
    expected.redemption_nullifier_root = nullifier_root;
    expected.pending_claim_root = smt::root_after_update(
        &id,
        &pending_claim_hash(&pending),
        &w.pending_claim_exclusion_proof,
    );
    ensure!(*new == expected, PostStateMismatch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redeem::MODE_FAST_OPERATOR;
    use satusd_crypto::nums::{derive_nums_key, tap_tweak};
    use satusd_crypto::smt::SparseMerkleTree;
    use satusd_types::derive::{lock_tweak, oracle_set_hash, redeem_intent_hash};
    use satusd_types::types::{ClaimClock, OutPoint};

    // The asset family of the real burn vector (a grouped SatUSD asset on the
    // devnet); `protocol_sink_script_key(FAMILY)` is the NUMS sink the burn proof
    // commits to, so the D16 binding + sink-derivation checks both hold.
    const FAMILY: [u8; 32] = [
        0x0c, 0x58, 0x77, 0x1b, 0xaf, 0x09, 0x1f, 0xbc, 0xea, 0xdf, 0x1c, 0x22, 0x39, 0x4e, 0x9e,
        0x72, 0xad, 0x91, 0xc6, 0xa1, 0x35, 0xd7, 0xbf, 0x78, 0x30, 0x08, 0x62, 0xa0, 0x63, 0xc4,
        0xbf, 0x9b,
    ];
    const OPERATOR: [u8; 32] = [0x20; 32];
    const PRICE: u64 = 5_000_000_000_000; // $50k/BTC
    const AMOUNT: u64 = 4_000; // gross = 4000 * 1e14 / 5e12 = 80_000 sats
    const GROSS: u64 = 80_000;
    const EPOCH: u64 = 7;
    const MTP: u64 = 1_699_996_400; // chain_time = MTP + 3600 = 1_700_000_000
    const TIP: u32 = 110;

    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().unwrap()
    }
    fn h64(s: &str) -> [u8; 64] {
        hex::decode(s).unwrap().try_into().unwrap()
    }

    /// The three circomlibjs signer pubkeys (privkeys 0x11/0x22/0x33).
    fn pubkeys() -> [[u8; 32]; 3] {
        [
            h32("323a1772ccd2bf78ca0f82e4de1d4d48ded87f6f26d92d6a99e5998ac88901a6"),
            h32("fff67c4b050994bb6d9e1a482edb649dab97b1bff12b73ae97af37487d828180"),
            h32("b54e5bf89ce35b522a33efcb1d87a3ae81754b36a5aa1bcb87c5bedd8986bd14"),
        ]
    }
    /// Their signatures over `oracle_message_hash(7, 100, 1.7e12, 5e12)`.
    fn sigs() -> [[u8; 64]; 3] {
        [
            h64("e1c966e0d52d5f5b20161c5b653101c10c7935521980770d838d826fbc93c42e35351a47e0b0d02009c15179e144ba6780244d10d391fbda0d5411f7b8562a02"),
            h64("149b0d3019084f8537bce93c981e68c812c663df98c81ba36d1083af6353a694f299684962649aa9c7cc27ae06f0f96f295665f32c7ab817b0ce4e2341ecd104"),
            h64("804b792bd825a16561f6f3b78f9a6db2949d5e5289d2230dd28a7b23c23e722b066761e4474e02c82d251a2c7ea0b8f34d1c79071bcb9b2b47094b9ceb572204"),
        ]
    }

    fn oracle_messages() -> Vec<OracleMessage> {
        let (pk, sg) = (pubkeys(), sigs());
        (0..3)
            .map(|i| OracleMessage {
                domain: [0; 32],
                oracle_id: [i as u8; 32],
                oracle_set_epoch: EPOCH,
                price_epoch: 100,
                timestamp_ms: 1_700_000_000_000,
                pair: *b"BTC/USD\0",
                price_e8: PRICE,
                source_commitment: [0; 32],
                signer_pubkey: pk[i],
                signature: sg[i],
            })
            .collect()
    }

    fn intent(seed: u8) -> (RedeemIntent, [u8; 32]) {
        let preimage = [seed; 32];
        let it = RedeemIntent {
            version: 1,
            network: 0,
            redemption_id: [seed; 32],
            satusd_asset_family_id: FAMILY,
            amount_satusd_atoms: AMOUNT,
            user_btc_refund_pubkey: [0x31; 32],
            user_btc_claim_pubkey: [0x32; 32],
            user_asset_refund_key: derive_nums_key("redeem-user-refund", &[seed]),
            operator_id: Some(OPERATOR),
            mode: MODE_FAST_OPERATOR,
            payment_hash: satusd_types::sha256(&[&preimage]),
            asset_lock_csv_delta: 288,
            btc_htlc_csv_delta: 144,
            max_operator_fee_bps: 50,
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x33; 32],
            expiry_height: 900_000,
            nonce: [seed; 32],
        };
        (it, preimage)
    }

    fn lock_record(it: &RedeemIntent) -> LockRecord {
        let rih = redeem_intent_hash(it);
        LockRecord {
            lock_record_version: 1,
            redeem_intent_hash: rih,
            lock_anchor_outpoint: OutPoint {
                txid: it.redemption_id,
                vout: 1,
            },
            lock_anchor_txid: it.redemption_id,
            lock_script_key: tap_tweak(
                &it.user_asset_refund_key,
                &lock_tweak(&rih, &it.payment_hash),
            ),
            lock_amount_atoms: it.amount_satusd_atoms,
            asset_family_id: FAMILY,
            asset_lock_csv_delta: it.asset_lock_csv_delta,
            payment_hash: it.payment_hash,
            lineage_proof_hash: [0x99; 32],
            lineage_verified_by: vec![[0xaa; 32]],
            anchor_inclusion_height: 840_001,
        }
    }

    /// The real burn-to-sink `proof.File` (a 4000-unit grouped SatUSD burn to the
    /// protocol NUMS sink, captured from live tapd) + its verified head anchor.
    fn burn_vector() -> (Vec<u8>, [u8; 32], u32) {
        let bytes = hex::decode(
            include_str!("../../../integration/lineage_vectors/burn_to_sink.hex").trim(),
        )
        .unwrap();
        let f = parse_proof_file(&bytes).unwrap();
        let head = verify_lineage(&f.parsed().unwrap()).unwrap();
        (bytes, head.txid, head.output_index)
    }

    fn finalize(lr: &LockRecord, preimage: [u8; 32]) -> LockFinalizeRecord {
        let (bytes, txid, vout) = burn_vector();
        LockFinalizeRecord {
            lock_record_hash: lock_record_hash(lr),
            payment_preimage: preimage,
            finalize_anchor_txid: txid,
            finalize_anchor_outpoint: OutPoint { txid, vout },
            protocol_sink_script_key: protocol_sink_script_key(&FAMILY),
            protocol_burn_internal_key: protocol_burn_internal_key(&FAMILY),
            finalized_amount_atoms: lr.lock_amount_atoms,
            operator_id: OPERATOR,
            finalize_height: 840_002,
            universe_burn_proof_hash: lineage_proof_hash(&bytes),
        }
    }

    fn base_prev() -> StateRoot {
        let empty = SparseMerkleTree::new().root();
        StateRoot {
            protocol_version: 1,
            state_epoch: 10,
            prev_state_root: [0; 32],
            transition_type: 0,
            satusd_asset_family_id: FAMILY,
            sat_usd_supply_atoms: 200_000_000,
            reserve_btc_sats: 4_000_000_000,
            reserved_pending_claim_sats: 0,
            collateral_ratio_ppm: 0,
            emergency_tier: 0,
            oracle_set_hash: oracle_set_hash(EPOCH, &pubkeys()),
            oracle_set_epoch: EPOCH,
            latest_oracle_epoch_seen: EPOCH,
            latest_oracle_price_e8: PRICE,
            reserve_committee_hash: [0x0d; 32],
            issuer_positions_root: [0x03; 32],
            operator_registry_root: [0x04; 32],
            lock_record_root: empty,
            lock_consumed_root: empty,
            lock_refund_root: empty,
            redemption_nullifier_root: empty,
            pending_claim_root: empty,
            live_da_root: [0x0a; 32],
            archival_da_root: [0x0b; 32],
            l1_anchor_hash: [0x0c; 32],
            l1_anchor_height: 840_000,
            l1_anchor_mtp: MTP,
            l1_anchor_chain_time: MTP + MTP_LAG_OFFSET_SEC,
        }
    }

    fn claim_clock() -> ClaimClock {
        ClaimClock {
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x0c; 32],
            l1_anchor_mtp: MTP,
            l1_anchor_chain_time: MTP + MTP_LAG_OFFSET_SEC,
            recent_header_chain: [[0u8; 80]; 12],
            oracle_epoch: EPOCH,
            selected_oracle_price_e8: PRICE,
            max_epoch_lag_sec: 600,
            oracle_future_tolerance: 300,
        }
    }

    /// Build prev/new + a valid witness for a batch of `seeds` (one lock each).
    fn build(seeds: &[u8]) -> (StateRoot, StateRoot, ReserveClaimWitness) {
        let mut prev = base_prev();

        // All locks live in the (immutable) record root.
        let mut record_tree = SparseMerkleTree::new();
        let parts: Vec<_> = seeds
            .iter()
            .map(|&s| {
                let (it, pre) = intent(s);
                let lr = lock_record(&it);
                record_tree.insert(lock_record_hash(&lr), &SET_MEMBER);
                (it, pre, lr)
            })
            .collect();
        prev.lock_record_root = record_tree.root();

        // Running consumed / nullifier trees produce chained exclusion proofs.
        let mut consumed_tree = SparseMerkleTree::new();
        let mut nullifier_tree = SparseMerkleTree::new();
        let refund_tree = SparseMerkleTree::new();
        let mut redemptions = Vec::new();
        let mut leaves = Vec::new();
        for (it, pre, lr) in &parts {
            let lr_hash = lock_record_hash(lr);
            let nf = redemption_nullifier(
                &lr.lock_anchor_outpoint,
                &lr.lock_script_key,
                &lr.redeem_intent_hash,
            );
            let membership = record_tree.prove(&lr_hash);
            let consumed = consumed_tree.prove(&lr_hash);
            let refund = refund_tree.prove(&lr_hash);
            let nullifier = nullifier_tree.prove(&nf);
            consumed_tree.insert(lr_hash, &SET_MEMBER);
            nullifier_tree.insert(nf, &SET_MEMBER);
            leaves.push(lr_hash);
            redemptions.push(BatchRedemption {
                redeem_intent: it.clone(),
                lock_record: lr.clone(),
                lock_finalize: finalize(lr, *pre),
                payout_confirmation: spv::build_confirmation(*pre, 100_000, 101, 6),
                lock_membership_proof: membership,
                consumed_exclusion_proof: consumed,
                refund_exclusion_proof: refund,
                nullifier_exclusion_proof: nullifier,
                burn_proof: burn_vector().0,
            });
        }

        let n = seeds.len() as u64;
        let new_supply = prev.sat_usd_supply_atoms - n * AMOUNT;

        // Claim inputs first (claim_id excludes claim_id/new_state_root — ADR-0022).
        let mut claim = ReserveClaim {
            claim_id: [0; 32],
            transition_type: TransitionType::RedeemFastFinalize.as_u8(),
            operator_id: OPERATOR,
            prev_state_root: state_root_hash(&prev),
            new_state_root: [0; 32],
            redemption_batch_root: batch_root_be(&leaves),
            oracle_batch_root: [0; 32],
            lock_batch_root: [0; 32],
            payout_batch_root: [0; 32],
            confirmation_batch_root: [0; 32],
            finalize_batch_root: [0; 32],
            burn_proof_batch_root: [0; 32],
            lineage_proof_batch_root: [0; 32],
            live_da_root: [0x0a; 32],
            archival_da_root: [0x0b; 32],
            l1_anchor: claim_clock(),
            reserve_shard_id: 0,
            reimbursement_sats: n * GROSS,
            proof_commitment: [0; 32],
            claim_expiry_height: 900_000,
            operator_signature: [0; 64],
        };
        claim.claim_id = claim_id(&claim);

        // PENDING claim → pending_claim_root (submit reserves, doesn't debit).
        let pending_tree = SparseMerkleTree::new();
        let pending_proof = pending_tree.prove(&claim.claim_id);
        let pending = PendingClaim {
            claim_id: claim.claim_id,
            operator_id: OPERATOR,
            reserved_sats: n * GROSS,
            claim_created_height: claim.l1_anchor.l1_anchor_height,
            claim_expiry_height: claim.claim_expiry_height,
            status: PendingClaimStatus::Pending,
        };

        let mut new = prev.clone();
        new.state_epoch = prev.state_epoch + 1;
        new.prev_state_root = state_root_hash(&prev);
        new.transition_type = TransitionType::RedeemFastFinalize.as_u8();
        new.sat_usd_supply_atoms = new_supply;
        new.reserved_pending_claim_sats = prev.reserved_pending_claim_sats + n * GROSS;
        new.latest_oracle_price_e8 = PRICE;
        new.collateral_ratio_ppm =
            tier::collateral_ratio_ppm(prev.reserve_btc_sats, new_supply, PRICE).unwrap_or(0);
        new.emergency_tier = tier::recompute_tier(prev.reserve_btc_sats, new_supply, PRICE).as_u8();
        new.lock_consumed_root = consumed_tree.root();
        new.redemption_nullifier_root = nullifier_tree.root();
        new.pending_claim_root = smt::root_after_update(
            &claim.claim_id,
            &pending_claim_hash(&pending),
            &pending_proof,
        );
        claim.new_state_root = state_root_hash(&new);

        let w = ReserveClaimWitness {
            claim,
            redemptions,
            oracle_messages: oracle_messages(),
            oracle_signer_set: pubkeys().to_vec(),
            pending_claim_exclusion_proof: pending_proof,
            btc_tip_height: TIP,
        };
        (prev, new, w)
    }

    #[test]
    fn single_redemption_happy() {
        let (prev, new, w) = build(&[1]);
        verify_reserve_claim_finalize(&prev, &new, &w).expect("claim ok");
    }

    #[test]
    fn batch_of_three_happy() {
        let (prev, new, w) = build(&[1, 2, 3]);
        verify_reserve_claim_finalize(&prev, &new, &w).expect("batch ok");
    }

    // ---- §5.D16 universe burn proof binding (real burn-to-sink vector) ----
    fn fin_for_binding() -> (Vec<u8>, LockFinalizeRecord) {
        let (it, pre) = intent(1);
        let lr = lock_record(&it);
        (burn_vector().0, finalize(&lr, pre))
    }

    #[test]
    fn burn_binding_holds() {
        let (bytes, fin) = fin_for_binding();
        check_burn_proof_binding(&bytes, &fin, &FAMILY, 0).expect("burn proof binds to sink");
    }

    #[test]
    fn burn_binding_rejects_wrong_sink() {
        // R-16: the burn output is not committed to the protocol NUMS sink.
        let (bytes, mut fin) = fin_for_binding();
        fin.protocol_sink_script_key[0] ^= 1;
        assert_eq!(
            check_burn_proof_binding(&bytes, &fin, &FAMILY, 0),
            Err(ClaimRejectReason::BurnProofScriptKeyMismatch)
        );
    }

    #[test]
    fn burn_binding_rejects_wrong_amount() {
        let (bytes, mut fin) = fin_for_binding();
        fin.finalized_amount_atoms += 1;
        assert_eq!(
            check_burn_proof_binding(&bytes, &fin, &FAMILY, 0),
            Err(ClaimRejectReason::BurnProofAmountMismatch)
        );
    }

    #[test]
    fn burn_binding_rejects_wrong_family() {
        let (bytes, fin) = fin_for_binding();
        assert_eq!(
            check_burn_proof_binding(&bytes, &fin, &[0x07; 32], 0),
            Err(ClaimRejectReason::BurnProofFamilyMismatch)
        );
    }

    #[test]
    fn burn_binding_rejects_wrong_anchor() {
        let (bytes, mut fin) = fin_for_binding();
        fin.finalize_anchor_outpoint.vout ^= 1;
        assert_eq!(
            check_burn_proof_binding(&bytes, &fin, &FAMILY, 0),
            Err(ClaimRejectReason::BurnProofAnchorMismatch)
        );
    }

    #[test]
    fn burn_binding_rejects_wrong_proof_hash() {
        let (bytes, mut fin) = fin_for_binding();
        fin.universe_burn_proof_hash[0] ^= 1;
        assert_eq!(
            check_burn_proof_binding(&bytes, &fin, &FAMILY, 0),
            Err(ClaimRejectReason::BurnProofHashMismatch)
        );
    }

    #[test]
    fn burn_binding_rejects_tampered_proof() {
        let (mut bytes, fin) = fin_for_binding();
        let i = bytes.len() / 2;
        bytes[i] ^= 1;
        // A flipped byte breaks the proof.File hash chain (or, deeper, the
        // commitment) ⇒ malformed or invalid, never a silent pass.
        assert!(matches!(
            check_burn_proof_binding(&bytes, &fin, &FAMILY, 0),
            Err(ClaimRejectReason::BurnProofMalformed) | Err(ClaimRejectReason::BurnProofInvalid)
        ));
    }

    #[test]
    fn forged_oracle_signature_rejected() {
        let (prev, new, mut w) = build(&[1]);
        w.oracle_messages[0].signature[0] ^= 1;
        assert_eq!(
            verify_reserve_claim_finalize(&prev, &new, &w),
            Err(ClaimRejectReason::Oracle(
                oracle::OracleRejectReason::SignatureInvalid
            ))
        );
    }

    #[test]
    fn shallow_burial_rejected() {
        // Tip only one block past the claim ⇒ not buried 6 deep (real SPV).
        let (prev, new, mut w) = build(&[1]);
        w.btc_tip_height = 102;
        assert_eq!(
            verify_reserve_claim_finalize(&prev, &new, &w),
            Err(ClaimRejectReason::Spv(spv::SpvError::NotBuriedDeepEnough))
        );
    }

    #[test]
    fn duplicate_lock_in_batch_rejected() {
        let (prev, new, w) = build(&[1, 1]);
        assert_eq!(
            verify_reserve_claim_finalize(&prev, &new, &w),
            Err(ClaimRejectReason::DuplicateInBatch)
        );
    }

    #[test]
    fn reimbursement_mismatch_rejected() {
        let (prev, new, mut w) = build(&[1, 2]);
        w.claim.reimbursement_sats += 1;
        assert_eq!(
            verify_reserve_claim_finalize(&prev, &new, &w),
            Err(ClaimRejectReason::ReimbursementMismatch)
        );
    }

    #[test]
    fn two_phase_submit_state_effects() {
        // Submit reserves funds + creates a PENDING claim; reserve_btc is untouched.
        let (prev, new, _w) = build(&[1, 2]);
        assert_eq!(new.reserve_btc_sats, prev.reserve_btc_sats); // not debited
        assert_eq!(new.reserved_pending_claim_sats, 2 * GROSS); // reserved
        assert_eq!(
            new.sat_usd_supply_atoms,
            prev.sat_usd_supply_atoms - 2 * AMOUNT
        );
        assert_ne!(new.pending_claim_root, prev.pending_claim_root); // claim inserted
    }

    #[test]
    fn claim_id_mismatch_rejected() {
        let (prev, new, mut w) = build(&[1]);
        w.claim.claim_id[0] ^= 1; // no longer the canonical derivation
        assert_eq!(
            verify_reserve_claim_finalize(&prev, &new, &w),
            Err(ClaimRejectReason::ClaimIdMismatch)
        );
    }

    // Note: the DuplicateClaim branch (claim_id already in pending_claim_root) is
    // unreachable in isolation — claim_id binds prev_state_root, so the same id
    // can't pre-exist without changing `prev`. Idempotency is covered at the node
    // level once submit is wired in.
}
