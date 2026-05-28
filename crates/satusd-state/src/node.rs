//! In-memory state node: holds the live `StateRoot` plus the six SMTs and the
//! issuer/claim records, and *executes* transitions end-to-end (generating its
//! own membership/exclusion proofs, applying the §8.1 executor, committing the
//! tree mutations, and self-checking that the committed roots match).
//!
//! This is the runnable accounting node the §7.3 HTTP API wraps. Callers pass
//! the business inputs (records + verified facts) in the transition witnesses;
//! the node fills in the SMT proof fields itself.
//!
//! The §5.D12 two-phase claim lifecycle is wired here:
//! - `submit_claim` reserves the reimbursement + creates a PENDING `PendingClaim`
//!   (batched `claim::verify_reserve_claim_finalize`, REDEEM_FAST_FINALIZE 0x11);
//! - `finalize_claim` pays an approved claim — debits `reserve_btc_sats`, frees
//!   the reservation, PENDING → FINALIZED (`registry::verify_finalize_claim`,
//!   FINALIZE_CLAIM 0x31). Gated by a committed reserve-committee M-of-N approval
//!   over `claim_id` (§11.2, ADR-0023); configure it via `set_reserve_committee`.
//! - `reclaim_stale_claim` frees an expired reservation, PENDING → RECLAIMED
//!   (`registry::verify_reclaim_stale_claim`, RECLAIM_STALE_CLAIM 0x30).

use std::collections::HashMap;

use satusd_crypto::poseidon::batch_root_be;
use satusd_crypto::smt::SparseMerkleTree;
use satusd_crypto::state::state_root_hash;
use satusd_reserve::{
    Approval, ClaimHandle, Committee, FinalizeError, MockReserve, MultisigReserve, PauseReason,
    ReserveBackend, ReserveView,
};
use satusd_types::derive::{
    claim_id, issuer_position_hash, lock_record_hash, operator_position_hash, pending_claim_hash,
    redemption_nullifier,
};
use satusd_types::tier;
use satusd_types::types::{
    ClaimClock, IssuerPosition, OperatorPosition, OracleMessage, PendingClaim, PendingClaimStatus,
    ReserveClaim, StateRoot, TransitionType,
};

use crate::oracle::{self, OracleParams};
use crate::redeem::SET_MEMBER;
use crate::{claim, mint, redeem, registry};

#[derive(Debug, PartialEq, Eq)]
pub enum NodeError {
    Mint(mint::MintRejectReason),
    Redeem(redeem::RedeemRejectReason),
    Registry(registry::RegistryRejectReason),
    Oracle(oracle::OracleRejectReason),
    Claim(claim::ClaimRejectReason),
    UnknownIssuer,
    UnknownClaim,
    /// The reserve-custody backend refused the payout (emergency pause, challenger
    /// veto, or insufficient committee approval — §11.2 MultisigReserve).
    ReserveRefused(FinalizeError),
    /// The node's committed tree roots disagree with the executor's post-state —
    /// an internal invariant break (should never happen).
    InvariantViolation,
}

#[derive(Clone)]
pub struct StateNode {
    state: StateRoot,
    issuer_tree: SparseMerkleTree,
    issuers: HashMap<[u8; 32], IssuerPosition>,
    operator_tree: SparseMerkleTree,
    operators: HashMap<[u8; 32], OperatorPosition>,
    pending_claim_tree: SparseMerkleTree,
    pending_claims: HashMap<[u8; 32], PendingClaim>,
    lock_record_tree: SparseMerkleTree,
    lock_consumed_tree: SparseMerkleTree,
    lock_refund_tree: SparseMerkleTree,
    nullifier_tree: SparseMerkleTree,
    /// BTC reserve custody (§5.D9). Tracks total/reserved sats in lockstep with the
    /// on-chain `reserve_btc_sats` / `reserved_pending_claim_sats`. MockReserve is
    /// the M0–M6 default; `use_multisig_reserve` swaps in the §11.2 committee-gated
    /// custody, which additionally enforces emergency pause + challenger veto at
    /// finalize (the consensus threshold check stays in `registry::finalize_claim`).
    reserve: Box<dyn ReserveBackend>,
}

impl StateNode {
    /// Fresh node: empty SMTs, epoch 0, the given asset family + oracle context.
    pub fn genesis(
        satusd_asset_family_id: [u8; 32],
        oracle_set_hash: [u8; 32],
        oracle_set_epoch: u64,
        latest_oracle_price_e8: u64,
    ) -> Self {
        let empty = SparseMerkleTree::new().root();
        let state = StateRoot {
            protocol_version: 1,
            state_epoch: 0,
            prev_state_root: [0; 32],
            transition_type: 0,
            satusd_asset_family_id,
            sat_usd_supply_atoms: 0,
            reserve_btc_sats: 0,
            reserved_pending_claim_sats: 0,
            collateral_ratio_ppm: 0,
            emergency_tier: 0,
            oracle_set_hash,
            oracle_set_epoch,
            latest_oracle_epoch_seen: oracle_set_epoch,
            latest_oracle_price_e8,
            reserve_committee_hash: [0; 32], // set via set_reserve_committee
            issuer_positions_root: empty,
            operator_registry_root: empty,
            lock_record_root: empty,
            lock_consumed_root: empty,
            lock_refund_root: empty,
            redemption_nullifier_root: empty,
            pending_claim_root: empty,
            live_da_root: empty,
            archival_da_root: empty,
            l1_anchor_hash: [0; 32],
            l1_anchor_height: 0,
            l1_anchor_mtp: 0,
            l1_anchor_chain_time: 0,
        };
        StateNode {
            state,
            issuer_tree: SparseMerkleTree::new(),
            issuers: HashMap::new(),
            operator_tree: SparseMerkleTree::new(),
            operators: HashMap::new(),
            pending_claim_tree: SparseMerkleTree::new(),
            pending_claims: HashMap::new(),
            lock_record_tree: SparseMerkleTree::new(),
            lock_consumed_tree: SparseMerkleTree::new(),
            lock_refund_tree: SparseMerkleTree::new(),
            nullifier_tree: SparseMerkleTree::new(),
            reserve: Box::new(MockReserve::new(0)),
        }
    }

    pub fn state(&self) -> &StateRoot {
        &self.state
    }

    /// The reserve-custody view (§5.D9). Stays in lockstep with the on-chain
    /// `reserve_btc_sats` / `reserved_pending_claim_sats` (see `reserve_consistent`).
    pub fn reserve_view(&self) -> ReserveView {
        self.reserve.reserve_view()
    }

    /// The custody backend must agree with the on-chain reserve fields after every
    /// reserve-touching transition; a mismatch is an internal invariant break.
    fn reserve_consistent(&self) -> Result<(), NodeError> {
        let v = self.reserve.reserve_view();
        if v.total_sats == self.state.reserve_btc_sats
            && v.reserved_sats == self.state.reserved_pending_claim_sats
        {
            Ok(())
        } else {
            Err(NodeError::InvariantViolation)
        }
    }

    pub fn state_root_hash(&self) -> [u8; 32] {
        state_root_hash(&self.state)
    }

    /// Sync the observed Bitcoin L1 anchor (the node follows the chain tip).
    /// `chain_time = mtp + MTP_LAG_OFFSET` (§5.D6) drives ClaimClock freshness.
    /// Not a SatUSD transition — it records external chain state.
    pub fn set_l1_anchor(&mut self, height: u32, hash: [u8; 32], mtp: u64) {
        self.state.l1_anchor_height = height;
        self.state.l1_anchor_hash = hash;
        self.state.l1_anchor_mtp = mtp;
        self.state.l1_anchor_chain_time = mtp + claim::MTP_LAG_OFFSET_SEC;
    }

    /// Configure the reserve-committee commitment (governance config, §11.2). The
    /// committed M-of-N then gates FINALIZE_CLAIM.
    pub fn set_reserve_committee(&mut self, reserve_committee_hash: [u8; 32]) {
        self.state.reserve_committee_hash = reserve_committee_hash;
    }

    /// Swap in the §11.2 MultisigReserve custody (Signet pilot), seeded from the
    /// current reserve, and commit its committee. Call before any pending claims.
    pub fn use_multisig_reserve(&mut self, committee: Committee) {
        self.state.reserve_committee_hash = committee.hash();
        self.reserve = Box::new(MultisigReserve::new(self.state.reserve_btc_sats, committee));
    }

    /// Emergency-pause the reserve custody — finalize is blocked until `resume`
    /// (§11.2 runbook scenarios 1/2).
    pub fn emergency_pause(&mut self, reason: PauseReason) {
        self.reserve.emergency_pause(reason);
    }

    /// Lift the emergency pause.
    pub fn resume_reserve(&mut self) {
        self.reserve.resume();
    }

    /// A challenger veto package against a pending claim — blocks its finalize.
    pub fn veto_claim(&mut self, claim_id: [u8; 32]) -> Result<(), NodeError> {
        self.reserve
            .veto(ClaimHandle(claim_id))
            .map_err(NodeError::ReserveRefused)
    }

    /// ROTATE_SHARD (§11.2): rotate the reserve committee — recommit its hash and
    /// drop stale approvals (the new members must re-approve pending claims).
    pub fn rotate_reserve_committee(&mut self, new_committee: Committee) {
        self.state.reserve_committee_hash = new_committee.hash();
        self.reserve.rotate_committee(new_committee);
    }

    pub fn issuer(&self, issuer_id: &[u8; 32]) -> Option<&IssuerPosition> {
        self.issuers.get(issuer_id)
    }

    /// Commit a candidate post-state, asserting the node's trees produce its roots.
    fn commit(&mut self, new_state: StateRoot) -> Result<[u8; 32], NodeError> {
        let ok = self.issuer_tree.root() == new_state.issuer_positions_root
            && self.operator_tree.root() == new_state.operator_registry_root
            && self.pending_claim_tree.root() == new_state.pending_claim_root
            && self.lock_record_tree.root() == new_state.lock_record_root
            && self.lock_consumed_tree.root() == new_state.lock_consumed_root
            && self.lock_refund_tree.root() == new_state.lock_refund_root
            && self.nullifier_tree.root() == new_state.redemption_nullifier_root;
        if !ok {
            return Err(NodeError::InvariantViolation);
        }
        self.state = new_state;
        Ok(self.state_root_hash())
    }

    /// ISSUER_REGISTER.
    pub fn issuer_register(&mut self, new_issuer: IssuerPosition) -> Result<[u8; 32], NodeError> {
        let proof = self.issuer_tree.prove(&new_issuer.issuer_id);
        let new_state = registry::apply_issuer_register(&self.state, &new_issuer, &proof)
            .map_err(NodeError::Registry)?;
        self.issuer_tree
            .insert(new_issuer.issuer_id, &issuer_position_hash(&new_issuer));
        self.issuers.insert(new_issuer.issuer_id, new_issuer);
        self.commit(new_state)
    }

    pub fn operator(&self, operator_id: &[u8; 32]) -> Option<&OperatorPosition> {
        self.operators.get(operator_id)
    }

    /// OPERATOR_REGISTER.
    pub fn operator_register(&mut self, new_op: OperatorPosition) -> Result<[u8; 32], NodeError> {
        let proof = self.operator_tree.prove(&new_op.operator_id);
        let new_state = registry::apply_operator_register(&self.state, &new_op, &proof)
            .map_err(NodeError::Registry)?;
        self.operator_tree
            .insert(new_op.operator_id, &operator_position_hash(&new_op));
        self.operators.insert(new_op.operator_id, new_op);
        self.commit(new_state)
    }

    /// MINT_COMMIT.
    pub fn mint_commit(
        &mut self,
        issuer_id: [u8; 32],
        w: &mint::MintCommitWitness,
    ) -> Result<[u8; 32], NodeError> {
        let prev_issuer = self
            .issuers
            .get(&issuer_id)
            .ok_or(NodeError::UnknownIssuer)?
            .clone();
        let proof = self.issuer_tree.prove(&issuer_id);
        let new_state = mint::apply_mint_commit(&self.state, &prev_issuer, &proof, w)
            .map_err(NodeError::Mint)?;
        let new_issuer = mint::issuer_after_commit(&prev_issuer, w).map_err(NodeError::Mint)?;
        self.issuer_tree
            .insert(issuer_id, &issuer_position_hash(&new_issuer));
        self.issuers.insert(issuer_id, new_issuer);
        let root = self.commit(new_state)?;
        // The deposit funds the reserve custody (§5.D9; commit added it to
        // reserve_btc_sats).
        self.reserve.credit(w.deposit_sats);
        self.reserve_consistent()?;
        Ok(root)
    }

    /// MINT_FINALIZE.
    pub fn mint_finalize(
        &mut self,
        issuer_id: [u8; 32],
        w: &mint::MintFinalizeWitness,
    ) -> Result<[u8; 32], NodeError> {
        let prev_issuer = self
            .issuers
            .get(&issuer_id)
            .ok_or(NodeError::UnknownIssuer)?
            .clone();
        let proof = self.issuer_tree.prove(&issuer_id);
        let new_state = mint::apply_mint_finalize(&self.state, &prev_issuer, &proof, w)
            .map_err(NodeError::Mint)?;
        let new_issuer = mint::issuer_after_finalize(&prev_issuer, w.requested_mint_atoms)
            .map_err(NodeError::Mint)?;
        self.issuer_tree
            .insert(issuer_id, &issuer_position_hash(&new_issuer));
        self.issuers.insert(issuer_id, new_issuer);
        self.commit(new_state)
    }

    /// REDEEM_FAST_LOCK. `w`'s proof field is filled by the node.
    pub fn redeem_lock(&mut self, mut w: redeem::RedeemLockWitness) -> Result<[u8; 32], NodeError> {
        let lr_hash = lock_record_hash(&w.lock_record);
        w.lock_exclusion_proof = self.lock_record_tree.prove(&lr_hash);
        let new_state = redeem::apply_redeem_lock(&self.state, &w).map_err(NodeError::Redeem)?;
        self.lock_record_tree.insert(lr_hash, &SET_MEMBER);
        self.commit(new_state)
    }

    /// Test-only: inject an already-validated lock (as if a prior
    /// `REDEEM_FAST_LOCK` had committed it) so claim-lifecycle tests can set up
    /// their precondition without a real tapd lineage anchor. Production locks
    /// always go through [`Self::redeem_lock`]'s §5.D15 verification.
    #[cfg(test)]
    fn insert_validated_lock_for_test(&mut self, lock: &satusd_types::types::LockRecord) {
        let lr_hash = lock_record_hash(lock);
        self.lock_record_tree.insert(lr_hash, &SET_MEMBER);
        self.state.lock_record_root = self.lock_record_tree.root();
    }

    /// LOCK_REFUND. `w`'s proof fields are filled by the node.
    pub fn lock_refund(&mut self, mut w: redeem::LockRefundWitness) -> Result<[u8; 32], NodeError> {
        let lr_hash = lock_record_hash(&w.lock_record);
        w.lock_membership_proof = self.lock_record_tree.prove(&lr_hash);
        w.consumed_exclusion_proof = self.lock_consumed_tree.prove(&lr_hash);
        w.refund_exclusion_proof = self.lock_refund_tree.prove(&lr_hash);
        let new_state = redeem::apply_lock_refund(&self.state, &w).map_err(NodeError::Redeem)?;
        self.lock_refund_tree.insert(lr_hash, &SET_MEMBER);
        self.commit(new_state)
    }

    pub fn pending_claim(&self, claim_id: &[u8; 32]) -> Option<&PendingClaim> {
        self.pending_claims.get(claim_id)
    }

    /// All PENDING / FINALIZED / RECLAIMED claims the node has seen (for
    /// challenger / keeper iteration).
    pub fn pending_claims(&self) -> &HashMap<[u8; 32], PendingClaim> {
        &self.pending_claims
    }

    /// REDEEM_FAST_FINALIZE submit_claim (§5.D12): reserve the reimbursement and
    /// create a PENDING claim (no `reserve_btc_sats` debit). `redemptions` carry
    /// the business data; the node fills their SMT proof fields and builds the
    /// `ReserveClaim`, then self-checks via the claim verifier before committing.
    /// Returns the `claim_id` handle (for a later settle/reclaim).
    #[allow(clippy::too_many_arguments)]
    pub fn submit_claim(
        &mut self,
        mut redemptions: Vec<claim::BatchRedemption>,
        oracle_messages: Vec<OracleMessage>,
        oracle_signer_set: Vec<[u8; 32]>,
        l1_anchor: ClaimClock,
        reserve_shard_id: u64,
        claim_expiry_height: u32,
        btc_tip_height: u32,
    ) -> Result<[u8; 32], NodeError> {
        // Batch price from the oracle quorum.
        oracle::verify_signatures(&oracle_messages).map_err(NodeError::Oracle)?;
        let chain_time = self.state.l1_anchor_mtp + claim::MTP_LAG_OFFSET_SEC;
        let params = OracleParams {
            expected_oracle_set_epoch: self.state.oracle_set_epoch,
            expected_oracle_set_hash: self.state.oracle_set_hash,
            chain_time_sec: chain_time,
            max_epoch_lag_sec: l1_anchor.max_epoch_lag_sec,
            oracle_future_tolerance_sec: l1_anchor.oracle_future_tolerance,
            signer_set: &oracle_signer_set,
        };
        let price = oracle::aggregate(&oracle_messages, &params).map_err(NodeError::Oracle)?;

        // Fill SMT proofs against running clones; accumulate totals + batch leaves.
        let operator_id = redemptions[0].redeem_intent.operator_id.unwrap_or([0; 32]);
        let mut consumed = self.lock_consumed_tree.clone();
        let mut nullifier = self.nullifier_tree.clone();
        let mut total_amount = 0u64;
        let mut total_gross = 0u64;
        let mut leaves = Vec::with_capacity(redemptions.len());
        for r in &mut redemptions {
            let lr_hash = lock_record_hash(&r.lock_record);
            let nf = redemption_nullifier(
                &r.lock_record.lock_anchor_outpoint,
                &r.lock_record.lock_script_key,
                &r.lock_record.redeem_intent_hash,
            );
            r.lock_membership_proof = self.lock_record_tree.prove(&lr_hash);
            r.consumed_exclusion_proof = consumed.prove(&lr_hash);
            r.refund_exclusion_proof = self.lock_refund_tree.prove(&lr_hash);
            r.nullifier_exclusion_proof = nullifier.prove(&nf);
            consumed.insert(lr_hash, &SET_MEMBER);
            nullifier.insert(nf, &SET_MEMBER);
            let gross = (r.lock_record.lock_amount_atoms as u128 * 10u128.pow(14)) / price as u128;
            total_amount += r.redeem_intent.amount_satusd_atoms;
            total_gross += gross as u64;
            leaves.push(lr_hash);
        }

        // Live DA root (§10.3, ADR-008) over the batch — committed in the claim and
        // recorded in the new state so the challenger can recompute from DA.
        let live_da = claim::live_da_root_for_claim(&redemptions, &oracle_messages);

        // Build the claim; claim_id is derived over its inputs (ADR-0022).
        let mut claim = ReserveClaim {
            claim_id: [0; 32],
            transition_type: TransitionType::RedeemFastFinalize.as_u8(),
            operator_id,
            prev_state_root: self.state_root_hash(),
            new_state_root: [0; 32],
            redemption_batch_root: batch_root_be(&leaves),
            oracle_batch_root: [0; 32],
            lock_batch_root: [0; 32],
            payout_batch_root: [0; 32],
            confirmation_batch_root: [0; 32],
            finalize_batch_root: [0; 32],
            burn_proof_batch_root: [0; 32],
            lineage_proof_batch_root: [0; 32],
            live_da_root: live_da,
            archival_da_root: self.state.archival_da_root,
            l1_anchor: l1_anchor.clone(),
            reserve_shard_id,
            reimbursement_sats: total_gross,
            proof_commitment: [0; 32],
            claim_expiry_height,
            operator_signature: [0; 64],
        };
        claim.claim_id = claim_id(&claim);

        // PENDING claim → pending_claim_root.
        let id = claim.claim_id;
        let pending_proof = self.pending_claim_tree.prove(&id);
        let pending = PendingClaim {
            claim_id: id,
            operator_id,
            reserved_sats: total_gross,
            claim_created_height: l1_anchor.l1_anchor_height,
            claim_expiry_height,
            status: PendingClaimStatus::Pending,
        };

        let new_supply = self.state.sat_usd_supply_atoms - total_amount;
        let reserve = self.state.reserve_btc_sats;
        let mut new = self.state.clone();
        new.state_epoch += 1;
        new.prev_state_root = self.state_root_hash();
        new.transition_type = TransitionType::RedeemFastFinalize.as_u8();
        new.sat_usd_supply_atoms = new_supply;
        new.reserved_pending_claim_sats += total_gross;
        new.latest_oracle_price_e8 = price;
        new.collateral_ratio_ppm =
            tier::collateral_ratio_ppm(reserve, new_supply, price).unwrap_or(0);
        new.emergency_tier = tier::recompute_tier(reserve, new_supply, price).as_u8();
        new.lock_consumed_root = consumed.root();
        new.redemption_nullifier_root = nullifier.root();
        new.live_da_root = live_da;
        new.pending_claim_root = satusd_crypto::smt::root_after_update(
            &id,
            &pending_claim_hash(&pending),
            &pending_proof,
        );
        claim.new_state_root = state_root_hash(&new);

        // Self-check via the claim verifier, then commit the tree mutations.
        let witness = claim::ReserveClaimWitness {
            claim,
            redemptions,
            oracle_messages,
            oracle_signer_set,
            pending_claim_exclusion_proof: pending_proof,
            btc_tip_height,
        };
        claim::verify_reserve_claim_finalize(&self.state, &new, &witness)
            .map_err(NodeError::Claim)?;

        self.lock_consumed_tree = consumed;
        self.nullifier_tree = nullifier;
        self.pending_claim_tree
            .insert(id, &pending_claim_hash(&pending));
        self.pending_claims.insert(id, pending);
        self.commit(new)?;
        // Reserve the reimbursement in custody (§5.D9; no payout yet).
        self.reserve.submit_claim(&witness.claim);
        self.reserve_consistent()?;
        Ok(id) // the claim handle for later settle/reclaim
    }

    /// RECLAIM_STALE_CLAIM (§5.D12): after expiry, free a PENDING claim's
    /// reservation and flip it to RECLAIMED.
    pub fn reclaim_stale_claim(
        &mut self,
        claim_id: [u8; 32],
        current_height: u32,
    ) -> Result<[u8; 32], NodeError> {
        let prev_claim = self
            .pending_claims
            .get(&claim_id)
            .ok_or(NodeError::UnknownClaim)?
            .clone();
        let proof = self.pending_claim_tree.prove(&claim_id);
        let new_state =
            registry::apply_reclaim_stale_claim(&self.state, &prev_claim, &proof, current_height)
                .map_err(NodeError::Registry)?;
        let mut reclaimed = prev_claim;
        reclaimed.status = PendingClaimStatus::Reclaimed;
        self.pending_claim_tree
            .insert(claim_id, &pending_claim_hash(&reclaimed));
        self.pending_claims.insert(claim_id, reclaimed);
        let root = self.commit(new_state)?;
        // Free the reservation in custody (§5.D9; never paid).
        self.reserve
            .reclaim_stale(ClaimHandle(claim_id))
            .map_err(|_| NodeError::InvariantViolation)?;
        self.reserve_consistent()?;
        Ok(root)
    }

    /// FINALIZE_CLAIM (§5.D12): pay out an approved PENDING claim — debit
    /// `reserve_btc_sats`, free the reservation, flip PENDING → FINALIZED. Requires
    /// the reserve committee's M-of-N `approvals` over `claim_id` (§11.2).
    pub fn finalize_claim(
        &mut self,
        claim_id: [u8; 32],
        committee: &registry::ReserveCommittee,
        approvals: &[mint::MultisigSig],
        current_height: u32,
    ) -> Result<[u8; 32], NodeError> {
        let prev_claim = self
            .pending_claims
            .get(&claim_id)
            .ok_or(NodeError::UnknownClaim)?
            .clone();
        let proof = self.pending_claim_tree.prove(&claim_id);
        // Consensus authority (pure): threshold + pending + expiry + membership.
        let new_state = registry::apply_finalize_claim(
            &self.state,
            &prev_claim,
            &proof,
            committee,
            approvals,
            current_height,
        )
        .map_err(NodeError::Registry)?;

        // Custody backend: record the committee approvals and pay out. For a
        // MultisigReserve this additionally enforces emergency pause / challenger
        // veto (a no-op on MockReserve). Both happen before any node mutation, so a
        // refusal leaves the state untouched.
        let h = ClaimHandle(claim_id);
        for a in approvals {
            self.reserve
                .add_approval(
                    h,
                    Approval {
                        signer_pubkey: a.signer_pubkey,
                        signature: a.signature,
                    },
                )
                .map_err(NodeError::ReserveRefused)?;
        }
        self.reserve
            .finalize_claim(h)
            .map_err(NodeError::ReserveRefused)?;

        let mut finalized = prev_claim;
        finalized.status = PendingClaimStatus::Finalized;
        self.pending_claim_tree
            .insert(claim_id, &pending_claim_hash(&finalized));
        self.pending_claims.insert(claim_id, finalized);
        let root = self.commit(new_state)?;
        self.reserve_consistent()?;
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_crypto::nums::{derive_nums_key, tap_tweak};
    use satusd_types::derive::{
        lock_tweak, oracle_set_hash, redeem_intent_hash, reserve_committee_hash,
    };
    use satusd_types::types::{
        IssuerStatus, LockFinalizeRecord, LockRecord, OutPoint, RedeemIntent,
    };
    use secp256k1::{Message, Secp256k1, SecretKey};

    fn h32(s: &str) -> [u8; 32] {
        hex::decode(s).unwrap().try_into().unwrap()
    }
    fn h64(s: &str) -> [u8; 64] {
        hex::decode(s).unwrap().try_into().unwrap()
    }

    /// circomlibjs oracle signer pubkeys (privkeys 0x11/0x22/0x33).
    fn oracle_pubkeys() -> Vec<[u8; 32]> {
        vec![
            h32("323a1772ccd2bf78ca0f82e4de1d4d48ded87f6f26d92d6a99e5998ac88901a6"),
            h32("fff67c4b050994bb6d9e1a482edb649dab97b1bff12b73ae97af37487d828180"),
            h32("b54e5bf89ce35b522a33efcb1d87a3ae81754b36a5aa1bcb87c5bedd8986bd14"),
        ]
    }
    /// Their signatures over `oracle_message_hash(7, 100, 1.7e12, 5e12)`.
    fn oracle_messages() -> Vec<OracleMessage> {
        let sigs = [
            h64("e1c966e0d52d5f5b20161c5b653101c10c7935521980770d838d826fbc93c42e35351a47e0b0d02009c15179e144ba6780244d10d391fbda0d5411f7b8562a02"),
            h64("149b0d3019084f8537bce93c981e68c812c663df98c81ba36d1083af6353a694f299684962649aa9c7cc27ae06f0f96f295665f32c7ab817b0ce4e2341ecd104"),
            h64("804b792bd825a16561f6f3b78f9a6db2949d5e5289d2230dd28a7b23c23e722b066761e4474e02c82d251a2c7ea0b8f34d1c79071bcb9b2b47094b9ceb572204"),
        ];
        oracle_pubkeys()
            .into_iter()
            .zip(sigs)
            .enumerate()
            .map(|(i, (pk, sig))| OracleMessage {
                domain: [0; 32],
                oracle_id: [i as u8; 32],
                oracle_set_epoch: 7,
                price_epoch: 100,
                timestamp_ms: 1_700_000_000_000,
                pair: *b"BTC/USD\0",
                price_e8: PRICE_50K,
                source_commitment: [0; 32],
                signer_pubkey: pk,
                signature: sig,
            })
            .collect()
    }
    const ORACLE_MTP: u64 = 1_699_996_400; // chain_time = MTP + 3600 = 1.7e9

    fn submit_claim_clock() -> ClaimClock {
        ClaimClock {
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x0c; 32],
            l1_anchor_mtp: ORACLE_MTP,
            l1_anchor_chain_time: ORACLE_MTP + claim::MTP_LAG_OFFSET_SEC,
            recent_header_chain: [[0u8; 80]; 12],
            oracle_epoch: 7,
            selected_oracle_price_e8: PRICE_50K,
            max_epoch_lag_sec: 600,
            oracle_future_tolerance: 300,
        }
    }

    /// A small redemption (4000 atoms → gross = 4000·1e14/5e12 = 80_000 sats).
    fn small_intent() -> (RedeemIntent, [u8; 32]) {
        let preimage = [0x77; 32];
        let it = RedeemIntent {
            version: 1,
            network: 0,
            redemption_id: [0x78; 32],
            satusd_asset_family_id: FAMILY,
            amount_satusd_atoms: 4_000,
            user_btc_refund_pubkey: [0x31; 32],
            user_btc_claim_pubkey: [0x32; 32],
            user_asset_refund_key: derive_nums_key("node-claim-refund", &[]),
            operator_id: Some(OPERATOR),
            mode: redeem::MODE_FAST_OPERATOR,
            payment_hash: satusd_types::sha256(&[&preimage]),
            asset_lock_csv_delta: 288,
            btc_htlc_csv_delta: 144,
            max_operator_fee_bps: 50,
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x33; 32],
            expiry_height: 900_000,
            nonce: [0x79; 32],
        };
        (it, preimage)
    }

    const PRICE_50K: u64 = 5_000_000_000_000;
    // The real burn vector's grouped-asset family (see claim.rs); makes the §5.D16
    // burn-proof binding hold for the claim-lifecycle setup.
    const FAMILY: [u8; 32] = [
        0x0c, 0x58, 0x77, 0x1b, 0xaf, 0x09, 0x1f, 0xbc, 0xea, 0xdf, 0x1c, 0x22, 0x39, 0x4e, 0x9e,
        0x72, 0xad, 0x91, 0xc6, 0xa1, 0x35, 0xd7, 0xbf, 0x78, 0x30, 0x08, 0x62, 0xa0, 0x63, 0xc4,
        0xbf, 0x9b,
    ];
    /// The real burn-to-sink `proof.File` + its verified head anchor (txid, vout).
    fn burn_vector() -> (Vec<u8>, [u8; 32], u32) {
        let bytes = hex::decode(
            include_str!("../../../integration/lineage_vectors/burn_to_sink.hex").trim(),
        )
        .unwrap();
        let f = satusd_ta_proof::parse_proof_file(&bytes).unwrap();
        let head = satusd_ta_proof::verify_lineage(&f.parsed().unwrap()).unwrap();
        (bytes, head.txid, head.output_index)
    }
    const ISSUER_ID: [u8; 32] = [0xaa; 32];
    const OPERATOR: [u8; 32] = [0x20; 32];
    const META: [u8; 32] = [0xef; 32];
    // Deposit txid is now derived from the SPV-verified deposit tx body (see
    // `test_deposit_for_node`), not a fixed const.

    fn keys() -> [SecretKey; 3] {
        [
            SecretKey::from_byte_array([0x11; 32]).unwrap(),
            SecretKey::from_byte_array([0x22; 32]).unwrap(),
            SecretKey::from_byte_array([0x33; 32]).unwrap(),
        ]
    }

    fn pubkeys() -> Vec<[u8; 33]> {
        let secp = Secp256k1::new();
        keys()
            .iter()
            .map(|k| k.public_key(&secp).serialize())
            .collect()
    }

    fn issuer() -> IssuerPosition {
        IssuerPosition {
            issuer_id: ISSUER_ID,
            status: IssuerStatus::Active,
            multisig_pubkeys: pubkeys(),
            multisig_threshold: 2,
            reserve_deposits_sats: 0,
            minted_satusd_atoms: 0,
            pending_mint_atoms: 0,
            collateral_ratio_ppm: 0,
            last_deposit_txid: None,
            freeze_reason: None,
            registered_at_height: 100,
            pending_mint_commitment: None,
        }
    }

    /// Cached synthetic deposit SPV proof + txid (built against the committee
    /// the node tests use). Shared by `commit_witness` and the `full_redemption_e2e`
    /// finalize witness so they reference the same `deposit_txid`.
    fn test_deposit_for_node() -> (satusd_types::types::BtcDepositConfirmation, [u8; 32]) {
        use std::sync::OnceLock;
        static CACHE: OnceLock<(satusd_types::types::BtcDepositConfirmation, [u8; 32])> =
            OnceLock::new();
        CACHE
            .get_or_init(|| {
                crate::spv::build_deposit_confirmation(4_000_000_000, &committee_pubkeys(), 3, 6)
            })
            .clone()
    }

    fn commit_witness() -> mint::MintCommitWitness {
        let requested_mint_atoms = 100_000_000u64;
        let deposit_sats = 4_000_000_000u64;
        let (deposit_conf, deposit_txid) = test_deposit_for_node();
        let sighash = satusd_types::derive::mint_request_sighash(
            &ISSUER_ID,
            requested_mint_atoms,
            deposit_sats,
            &deposit_txid,
            &META,
        );
        let secp = Secp256k1::new();
        let signatures = keys()[..2]
            .iter()
            .zip(pubkeys())
            .map(|(sk, pk)| mint::MultisigSig {
                signer_pubkey: pk,
                signature: secp
                    .sign_ecdsa(Message::from_digest(sighash), sk)
                    .serialize_compact(),
            })
            .collect();
        mint::MintCommitWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms,
            deposit_txid,
            deposit_sats,
            asset_metadata_commitment: META,
            signatures,
            oracle_price_e8: PRICE_50K,
            deposit_confirmation: deposit_conf,
            reserve_committee_pubkeys: committee_pubkeys(),
            reserve_committee_threshold: 3,
        }
    }

    fn lock_record(it: &RedeemIntent) -> LockRecord {
        let rih = redeem_intent_hash(it);
        LockRecord {
            lock_record_version: 1,
            redeem_intent_hash: rih,
            lock_anchor_outpoint: OutPoint {
                txid: [0x40; 32],
                vout: 1,
            },
            lock_anchor_txid: [0x40; 32],
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

    // ---- Reserve committee (3-of-5) ----
    fn committee_keys() -> Vec<SecretKey> {
        (1u8..=5)
            .map(|i| SecretKey::from_byte_array([i; 32]).unwrap())
            .collect()
    }
    fn committee_pubkeys() -> Vec<[u8; 33]> {
        let secp = Secp256k1::new();
        committee_keys()
            .iter()
            .map(|k| k.public_key(&secp).serialize())
            .collect()
    }
    fn committee() -> registry::ReserveCommittee {
        registry::ReserveCommittee {
            threshold: 3,
            pubkeys: committee_pubkeys(),
        }
    }
    fn committee_approvals(claim_id: &[u8; 32], n: usize) -> Vec<mint::MultisigSig> {
        let secp = Secp256k1::new();
        committee_keys()
            .iter()
            .take(n)
            .zip(committee_pubkeys())
            .map(|(sk, pk)| mint::MultisigSig {
                signer_pubkey: pk,
                signature: secp
                    .sign_ecdsa(Message::from_digest(*claim_id), sk)
                    .serialize_compact(),
            })
            .collect()
    }

    /// The single batch redemption for `small_intent`'s lock (proofs filled by
    /// the node at submit). Deterministic, so it is rebuilt for replay tests.
    fn one_redemption() -> claim::BatchRedemption {
        let (it, preimage) = small_intent();
        let lr = lock_record(&it);
        let (burn_bytes, burn_txid, burn_vout) = burn_vector();
        claim::BatchRedemption {
            redeem_intent: it,
            lock_finalize: LockFinalizeRecord {
                lock_record_hash: lock_record_hash(&lr),
                payment_preimage: preimage,
                finalize_anchor_txid: burn_txid,
                finalize_anchor_outpoint: OutPoint {
                    txid: burn_txid,
                    vout: burn_vout,
                },
                protocol_sink_script_key: satusd_crypto::nums::protocol_sink_script_key(&FAMILY),
                protocol_burn_internal_key: satusd_crypto::nums::protocol_burn_internal_key(
                    &FAMILY,
                ),
                finalized_amount_atoms: lr.lock_amount_atoms,
                operator_id: OPERATOR,
                finalize_height: 840_002,
                universe_burn_proof_hash: satusd_types::derive::lineage_proof_hash(&burn_bytes),
            },
            payout_confirmation: crate::spv::build_confirmation(preimage, 100_000, 101, 6),
            lock_record: lr,
            lock_membership_proof: vec![],
            consumed_exclusion_proof: vec![],
            refund_exclusion_proof: vec![],
            nullifier_exclusion_proof: vec![],
            burn_proof: burn_bytes,
        }
    }

    fn submit_one(node: &mut StateNode) -> Result<[u8; 32], NodeError> {
        node.submit_claim(
            vec![one_redemption()],
            oracle_messages(),
            oracle_pubkeys(),
            submit_claim_clock(),
            0,
            840_100, // claim_expiry_height
            110,     // btc tip
        )
    }

    /// genesis → register → mint → lock → submit_claim. Returns the node, the
    /// `claim_id`, and the reserve balance before submit (claim expiry 840_100).
    fn setup_submitted() -> (StateNode, [u8; 32], u64) {
        setup_submitted_backend(false)
    }

    /// As `setup_submitted`, but optionally swaps in the §11.2 MultisigReserve
    /// custody (seeded before any deposits) instead of the default Mock backend.
    fn setup_submitted_backend(multisig: bool) -> (StateNode, [u8; 32], u64) {
        let mut node =
            StateNode::genesis(FAMILY, oracle_set_hash(7, &oracle_pubkeys()), 7, PRICE_50K);
        node.set_l1_anchor(840_000, [0x0c; 32], ORACLE_MTP);
        if multisig {
            node.use_multisig_reserve(Committee {
                threshold: 3,
                pubkeys: committee_pubkeys(),
            });
        } else {
            node.set_reserve_committee(reserve_committee_hash(3, &committee_pubkeys()));
        }
        node.issuer_register(issuer()).unwrap();
        node.mint_commit(ISSUER_ID, &commit_witness()).unwrap();
        let fin = mint::MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: 100_000_000,
            deposit_txid: test_deposit_for_node().1,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        };
        node.mint_finalize(ISSUER_ID, &fin).unwrap();
        let reserve_before = node.state().reserve_btc_sats;

        let lr = one_redemption().lock_record.clone();
        // Set up the lock precondition directly: this test exercises the claim
        // lifecycle, not §5.D15 lineage verification (covered in redeem.rs).
        node.insert_validated_lock_for_test(&lr);

        let claim_id = submit_one(&mut node).expect("submit");

        // Submit reserves 80_000 sats and creates a PENDING claim; no reserve debit.
        assert_eq!(node.state().reserved_pending_claim_sats, 80_000);
        assert_eq!(node.state().reserve_btc_sats, reserve_before);
        assert_eq!(node.state().sat_usd_supply_atoms, 100_000_000 - 4_000);
        assert_eq!(
            node.pending_claim(&claim_id).unwrap().status,
            PendingClaimStatus::Pending
        );
        (node, claim_id, reserve_before)
    }

    /// The canonical lock-vector intent (matches `capture_lock_vector` +
    /// redeem.rs); `lock_anchor.hex`'s head commits `derive_lock_script_key` of it.
    fn lock_vector_intent() -> RedeemIntent {
        RedeemIntent {
            version: 1,
            network: 0,
            redemption_id: [0x77; 32],
            satusd_asset_family_id: FAMILY,
            amount_satusd_atoms: 4_000,
            user_btc_refund_pubkey: [0x31; 32],
            user_btc_claim_pubkey: [0x32; 32],
            user_asset_refund_key: derive_nums_key("satusd-lock-vector-user", &[]),
            operator_id: Some(OPERATOR),
            mode: redeem::MODE_FAST_OPERATOR,
            payment_hash: satusd_types::sha256(&[&[0x55u8; 32]]),
            asset_lock_csv_delta: 288,
            btc_htlc_csv_delta: 144,
            max_operator_fee_bps: 50,
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x33; 32],
            expiry_height: 900_000,
            nonce: [0x34; 32],
        }
    }

    /// The real lock-anchor `proof.File` + its verified head anchor (txid, vout).
    fn lock_anchor_vector() -> (Vec<u8>, [u8; 32], u32) {
        let bytes = hex::decode(
            include_str!("../../../integration/lineage_vectors/lock_anchor.hex").trim(),
        )
        .unwrap();
        let f = satusd_ta_proof::parse_proof_file(&bytes).unwrap();
        let head = satusd_ta_proof::verify_lineage(&f.parsed().unwrap()).unwrap();
        (bytes, head.txid, head.output_index)
    }

    /// ★ Complete redemption E2E through the state node, with REAL on-chain
    /// vectors: genesis → mint → **real `REDEEM_FAST_LOCK`** (the lock's lineage is
    /// independently re-verified from `lock_anchor.hex`, §5.D15) → **submit_claim**
    /// (`REDEEM_FAST_FINALIZE`) with the real universe burn proof (`burn_to_sink.hex`,
    /// §5.D16), a valid SPV payout confirmation (§5.D14), and a real oracle quorum.
    /// All five witnesses (lock / lineage / SPV / burn / oracle) share one family,
    /// intent, payment hash, and amount. The node accepts: supply burns, the
    /// reimbursement is reserved.
    #[test]
    fn full_redemption_e2e_real_vectors() {
        let mut node =
            StateNode::genesis(FAMILY, oracle_set_hash(7, &oracle_pubkeys()), 7, PRICE_50K);
        node.set_l1_anchor(840_000, [0x0c; 32], ORACLE_MTP);
        node.set_reserve_committee(reserve_committee_hash(3, &committee_pubkeys()));
        node.issuer_register(issuer()).unwrap();
        node.mint_commit(ISSUER_ID, &commit_witness()).unwrap();
        node.mint_finalize(
            ISSUER_ID,
            &mint::MintFinalizeWitness {
                issuer_id: ISSUER_ID,
                requested_mint_atoms: 100_000_000,
                deposit_txid: test_deposit_for_node().1,
                asset_metadata_commitment: META,
                mint_anchor_confirmations: 6,
                mint_proof_ok: true,
                oracle_price_e8: PRICE_50K,
            },
        )
        .unwrap();
        let reserve_before = node.state().reserve_btc_sats;

        // --- REDEEM_FAST_LOCK: real lineage proof (lock_anchor.hex) re-verified ---
        let it = lock_vector_intent();
        let rih = redeem_intent_hash(&it);
        let (lock_bytes, lock_txid, lock_vout) = lock_anchor_vector();
        let lock = LockRecord {
            lock_record_version: 1,
            redeem_intent_hash: rih,
            lock_anchor_outpoint: OutPoint {
                txid: lock_txid,
                vout: lock_vout,
            },
            lock_anchor_txid: lock_txid,
            lock_script_key: tap_tweak(
                &it.user_asset_refund_key,
                &lock_tweak(&rih, &it.payment_hash),
            ),
            lock_amount_atoms: it.amount_satusd_atoms,
            asset_family_id: FAMILY,
            asset_lock_csv_delta: it.asset_lock_csv_delta,
            payment_hash: it.payment_hash,
            lineage_proof_hash: satusd_types::derive::lineage_proof_hash(&lock_bytes),
            lineage_verified_by: vec![],
            anchor_inclusion_height: 1,
        };
        node.redeem_lock(redeem::RedeemLockWitness {
            redeem_intent: it.clone(),
            lock_record: lock.clone(),
            lock_exclusion_proof: vec![], // filled by the node
            lineage_proof: lock_bytes,
        })
        .expect("real lock anchor verifies + commits");

        // --- REDEEM_FAST_FINALIZE: submit_claim with real burn + SPV + oracle ---
        let (burn_bytes, burn_txid, burn_vout) = burn_vector();
        let preimage = [0x55u8; 32]; // it.payment_hash == sha256(preimage)
        let redemption = claim::BatchRedemption {
            redeem_intent: it.clone(),
            lock_record: lock.clone(),
            lock_finalize: LockFinalizeRecord {
                lock_record_hash: lock_record_hash(&lock),
                payment_preimage: preimage,
                finalize_anchor_txid: burn_txid,
                finalize_anchor_outpoint: OutPoint {
                    txid: burn_txid,
                    vout: burn_vout,
                },
                protocol_sink_script_key: satusd_crypto::nums::protocol_sink_script_key(&FAMILY),
                protocol_burn_internal_key: satusd_crypto::nums::protocol_burn_internal_key(
                    &FAMILY,
                ),
                finalized_amount_atoms: lock.lock_amount_atoms,
                operator_id: OPERATOR,
                finalize_height: 840_002,
                universe_burn_proof_hash: satusd_types::derive::lineage_proof_hash(&burn_bytes),
            },
            payout_confirmation: crate::spv::build_confirmation(preimage, 100_000, 101, 6),
            lock_membership_proof: vec![],
            consumed_exclusion_proof: vec![],
            refund_exclusion_proof: vec![],
            nullifier_exclusion_proof: vec![],
            burn_proof: burn_bytes,
        };
        let claim_id = node
            .submit_claim(
                vec![redemption],
                oracle_messages(),
                oracle_pubkeys(),
                submit_claim_clock(),
                0,
                840_100,
                110,
            )
            .expect("node accepts the reserve claim");

        // The complete redemption committed: supply burned, reimbursement reserved.
        assert_eq!(node.state().sat_usd_supply_atoms, 100_000_000 - 4_000);
        assert_eq!(node.state().reserved_pending_claim_sats, 80_000);
        assert_eq!(node.state().reserve_btc_sats, reserve_before); // not debited at submit
        assert_eq!(
            node.pending_claim(&claim_id).unwrap().status,
            PendingClaimStatus::Pending
        );
        // The reserve-custody backend (§5.D9) tracks the on-chain reserve fields:
        // funded by the mint deposit, the reimbursement reserved (not yet paid).
        assert_eq!(node.reserve_view().total_sats, reserve_before);
        assert_eq!(node.reserve_view().reserved_sats, 80_000);
        assert_eq!(
            node.reserve_view().available_sats(),
            reserve_before - 80_000
        );
    }

    /// submit → expire → reclaim: the reservation is freed, reserve_btc untouched.
    #[test]
    fn submit_then_reclaim_lifecycle() {
        let (mut node, claim_id, reserve_before) = setup_submitted();
        node.reclaim_stale_claim(claim_id, 840_200) // past expiry
            .expect("reclaim");
        assert_eq!(node.state().reserved_pending_claim_sats, 0);
        assert_eq!(node.state().reserve_btc_sats, reserve_before); // never paid
        assert_eq!(
            node.pending_claim(&claim_id).unwrap().status,
            PendingClaimStatus::Reclaimed
        );
    }

    /// submit → finalize: the operator is paid — reserve_btc debited, reservation
    /// freed, claim FINALIZED.
    #[test]
    fn submit_then_finalize_lifecycle() {
        let (mut node, claim_id, reserve_before) = setup_submitted();
        let approvals = committee_approvals(&claim_id, 3); // 3-of-5
        node.finalize_claim(claim_id, &committee(), &approvals, 840_050) // before expiry
            .expect("finalize");
        assert_eq!(node.state().reserved_pending_claim_sats, 0);
        assert_eq!(node.state().reserve_btc_sats, reserve_before - 80_000); // paid out
        assert_eq!(
            node.pending_claim(&claim_id).unwrap().status,
            PendingClaimStatus::Finalized
        );
    }

    /// A second submit of the same (now-consumed) lock is rejected by the node.
    #[test]
    fn resubmit_consumed_lock_rejected() {
        let (mut node, _claim_id, _reserve) = setup_submitted();
        assert_eq!(
            submit_one(&mut node),
            Err(NodeError::Claim(claim::ClaimRejectReason::Redeem(
                redeem::RedeemRejectReason::LockConsumed
            )))
        );
    }

    /// With the MultisigReserve custody (§11.2), an emergency pause blocks finalize
    /// even with full committee approval; resume lets it through.
    #[test]
    fn multisig_pause_blocks_finalize_then_resume() {
        let (mut node, claim_id, reserve_before) = setup_submitted_backend(true);
        let approvals = committee_approvals(&claim_id, 3);
        node.emergency_pause(PauseReason::StateNodeBug);
        assert_eq!(
            node.finalize_claim(claim_id, &committee(), &approvals, 840_050),
            Err(NodeError::ReserveRefused(FinalizeError::Paused))
        );
        // Still PENDING + nothing paid.
        assert_eq!(node.state().reserve_btc_sats, reserve_before);
        assert_eq!(
            node.pending_claim(&claim_id).unwrap().status,
            PendingClaimStatus::Pending
        );
        node.resume_reserve();
        node.finalize_claim(claim_id, &committee(), &approvals, 840_050)
            .expect("finalize after resume");
        assert_eq!(node.state().reserve_btc_sats, reserve_before - 80_000);
    }

    /// A challenger veto blocks finalize on the MultisigReserve custody.
    #[test]
    fn multisig_veto_blocks_finalize() {
        let (mut node, claim_id, _reserve) = setup_submitted_backend(true);
        let approvals = committee_approvals(&claim_id, 3);
        node.veto_claim(claim_id).unwrap();
        assert_eq!(
            node.finalize_claim(claim_id, &committee(), &approvals, 840_050),
            Err(NodeError::ReserveRefused(FinalizeError::Vetoed))
        );
    }

    /// ROTATE_SHARD: after rotating the committee, the old committee's approvals no
    /// longer match the committed `reserve_committee_hash` (consensus rejects).
    #[test]
    fn rotate_committee_invalidates_old_approvals() {
        let (mut node, claim_id, _reserve) = setup_submitted_backend(true);
        let old_hash = node.state().reserve_committee_hash;
        // Rotate one member out (a distinct pubkey set ⇒ a new commitment).
        let mut pubkeys = committee_pubkeys();
        pubkeys[0] = [0x02; 33];
        let new_committee = Committee {
            threshold: 3,
            pubkeys,
        };
        node.rotate_reserve_committee(new_committee);
        assert_ne!(node.state().reserve_committee_hash, old_hash);

        // The OLD committee's approvals no longer satisfy the committed hash.
        let approvals = committee_approvals(&claim_id, 3);
        assert_eq!(
            node.finalize_claim(claim_id, &committee(), &approvals, 840_050),
            Err(NodeError::Registry(
                registry::RegistryRejectReason::CommitteeMismatch
            ))
        );
    }
}
