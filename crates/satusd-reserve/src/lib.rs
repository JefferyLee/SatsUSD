//! Reserve backend abstraction (PRD §5.D9, DL-7) + the Mock / Multisig backends.
//!
//! The BTC reserve custody is modelled separately from the SatUSD state machine:
//! the state node burns supply and reserves a reimbursement at `submit_claim`; the
//! **reserve backend** is what actually holds the sats and pays the operator at
//! `finalize_claim`. DL-7 splits the abstraction so MultisigReserve does **not**
//! pretend to run an optimistic challenge — the optimistic three-phase
//! (assert/disprove/withdraw) lives in a separate `OptimisticEnforcementBackend`
//! (M7+), keeping the challenger code stable across the BitVM3 migration.
//!
//! Scope here (M6 code core): the [`ReserveBackend`] trait, [`MockReserve`] (M0–M4
//! default, immediate), and [`MultisigReserve`] (the §11.2 3-of-5 committee
//! approval state machine + emergency pause + challenger veto + stale reclaim) with
//! the committee signing/aggregation tooling. The actual BTC reserve-spend
//! transaction broadcast on signet is represented by [`ReserveSpend`] and deferred
//! to the live pilot.

use std::collections::HashMap;

use satusd_crypto::ecdsa::verify_ecdsa;
use satusd_types::derive::reserve_committee_hash;
use satusd_types::types::ReserveClaim;

/// A view of the BTC reserve custody (sats), independent of the SatUSD supply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReserveView {
    /// Total sats in the reserve UTXO(s).
    pub total_sats: u64,
    /// Sats committed to PENDING claims (not yet paid out).
    pub reserved_sats: u64,
}

impl ReserveView {
    /// Sats free to back new claims.
    pub fn available_sats(&self) -> u64 {
        self.total_sats.saturating_sub(self.reserved_sats)
    }
}

/// Opaque handle to a submitted claim (its `claim_id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClaimHandle(pub [u8; 32]);

/// Why finalization was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FinalizeError {
    /// The backend is emergency-paused (§11.2 runbook).
    Paused,
    /// A challenger filed a veto package against this claim.
    Vetoed,
    UnknownClaim,
    AlreadyFinalized,
    /// Committee approvals are below threshold.
    ApprovalInsufficient {
        have: usize,
        need: usize,
    },
    /// The reserve does not hold enough sats to pay the claim.
    ReserveInsufficient,
}

/// The reserve-spend the backend would broadcast to reimburse the operator. On
/// MockReserve it is informational; on MultisigReserve it is the 3-of-5 spend tx to
/// build/broadcast on signet (deferred to the live pilot).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveSpend {
    pub claim_id: [u8; 32],
    pub operator_id: [u8; 32],
    pub amount_sats: u64,
}

/// Why the committee paused finalization (§11.2 emergency recovery runbook).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    StateNodeBug,
    FundsAtRisk,
    Manual,
}

/// The non-optimistic reserve backend (§5.D9). MockReserve and MultisigReserve
/// implement this; optimistic backends (M7+) extend it with the three-phase
/// `OptimisticEnforcementBackend` (not defined here — DL-7).
///
/// Deviation from the PRD sketch: methods take `&mut self` (a real backend mutates
/// its custody/pending state), and `finalize_claim` returns the spend or a typed
/// refusal rather than panicking.
pub trait ReserveBackend: Send + Sync {
    fn reserve_view(&self) -> ReserveView;
    /// Credit a confirmed BTC reserve deposit into custody.
    fn credit(&mut self, sats: u64);
    fn submit_claim(&mut self, claim: &ReserveClaim) -> ClaimHandle;
    fn finalize_claim(&mut self, h: ClaimHandle) -> Result<ReserveSpend, FinalizeError>;
    fn emergency_pause(&mut self, reason: PauseReason);
    /// Free a stale claim's reservation; returns the freed sats.
    fn reclaim_stale(&mut self, h: ClaimHandle) -> Result<u64, FinalizeError>;

    // --- committee hooks (no-ops on non-committee backends like MockReserve) ---
    /// Lift an emergency pause.
    fn resume(&mut self) {}
    /// Record a committee member's approval of a claim.
    fn add_approval(&mut self, _h: ClaimHandle, _a: Approval) -> Result<(), FinalizeError> {
        Ok(())
    }
    /// A challenger's veto package against a claim.
    fn veto(&mut self, _h: ClaimHandle) -> Result<(), FinalizeError> {
        Ok(())
    }
    /// Rotate the committee (ROTATE_SHARD, §11.2): replaces the member set and
    /// drops any approvals collected under the old set.
    fn rotate_committee(&mut self, _new: Committee) {}

    /// Clone into a box (lets `Box<dyn ReserveBackend>` be `Clone`).
    fn clone_box(&self) -> Box<dyn ReserveBackend>;
}

impl Clone for Box<dyn ReserveBackend> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Clone)]
struct Pending {
    operator_id: [u8; 32],
    amount_sats: u64,
}

// ---------------------------------------------------------------------------
// MockReserve — in-memory, immediate (M0–M4 default).
// ---------------------------------------------------------------------------

/// In-memory reserve: `finalize_claim` pays immediately (no committee, no BTC tx).
#[derive(Clone)]
pub struct MockReserve {
    total_sats: u64,
    reserved_sats: u64,
    pending: HashMap<[u8; 32], Pending>,
}

impl MockReserve {
    pub fn new(total_sats: u64) -> Self {
        Self {
            total_sats,
            reserved_sats: 0,
            pending: HashMap::new(),
        }
    }
}

impl ReserveBackend for MockReserve {
    fn reserve_view(&self) -> ReserveView {
        ReserveView {
            total_sats: self.total_sats,
            reserved_sats: self.reserved_sats,
        }
    }

    fn credit(&mut self, sats: u64) {
        self.total_sats += sats;
    }

    fn clone_box(&self) -> Box<dyn ReserveBackend> {
        Box::new(self.clone())
    }

    fn submit_claim(&mut self, claim: &ReserveClaim) -> ClaimHandle {
        self.reserved_sats += claim.reimbursement_sats;
        self.pending.insert(
            claim.claim_id,
            Pending {
                operator_id: claim.operator_id,
                amount_sats: claim.reimbursement_sats,
            },
        );
        ClaimHandle(claim.claim_id)
    }

    fn finalize_claim(&mut self, h: ClaimHandle) -> Result<ReserveSpend, FinalizeError> {
        let p = self
            .pending
            .remove(&h.0)
            .ok_or(FinalizeError::UnknownClaim)?;
        if self.total_sats < p.amount_sats {
            self.pending.insert(h.0, p); // restore
            return Err(FinalizeError::ReserveInsufficient);
        }
        self.total_sats -= p.amount_sats;
        self.reserved_sats -= p.amount_sats;
        Ok(ReserveSpend {
            claim_id: h.0,
            operator_id: p.operator_id,
            amount_sats: p.amount_sats,
        })
    }

    fn emergency_pause(&mut self, _reason: PauseReason) {}

    fn reclaim_stale(&mut self, h: ClaimHandle) -> Result<u64, FinalizeError> {
        let p = self
            .pending
            .remove(&h.0)
            .ok_or(FinalizeError::UnknownClaim)?;
        self.reserved_sats -= p.amount_sats;
        Ok(p.amount_sats)
    }
}

// ---------------------------------------------------------------------------
// MultisigReserve — 3-of-5 committee approval (§11.2, Signet M6).
// ---------------------------------------------------------------------------

/// The reserve committee: an M-of-N over `claim_id` (ECDSA, §11.2 / ADR-0023).
/// Distinct from the issuer set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committee {
    pub threshold: u8,
    pub pubkeys: Vec<[u8; 33]>,
}

impl Committee {
    /// `reserve_committee_hash` — the commitment stored in `StateRoot` (ADR-0023).
    pub fn hash(&self) -> [u8; 32] {
        reserve_committee_hash(self.threshold, &self.pubkeys)
    }
}

/// A committee member's approval of a `claim_id` (compact ECDSA over the 32-byte
/// `claim_id`, matching `registry::verify_finalize_claim`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Approval {
    pub signer_pubkey: [u8; 33],
    pub signature: [u8; 64],
}

/// Committee tooling: produce an [`Approval`] of `claim_id` with a member secret
/// key (33-byte compressed pubkey, 64-byte compact signature).
pub fn sign_claim_approval(claim_id: &[u8; 32], secret_key: &[u8; 32]) -> Approval {
    use secp256k1::{Message, Secp256k1, SecretKey};
    let secp = Secp256k1::new();
    let sk = SecretKey::from_byte_array(*secret_key).expect("valid secret key");
    let signer_pubkey = sk.public_key(&secp).serialize();
    let signature = secp
        .sign_ecdsa(Message::from_digest(*claim_id), &sk)
        .serialize_compact();
    Approval {
        signer_pubkey,
        signature,
    }
}

/// Count the distinct, authorized, valid approvals of `claim_id` (committee
/// tooling — the same rule `registry::verify_finalize_claim` enforces).
pub fn count_approvals(
    committee: &Committee,
    claim_id: &[u8; 32],
    approvals: &[Approval],
) -> usize {
    let mut seen: Vec<[u8; 33]> = Vec::new();
    for a in approvals {
        if !committee.pubkeys.contains(&a.signer_pubkey) || seen.contains(&a.signer_pubkey) {
            continue;
        }
        if verify_ecdsa(claim_id, &a.signer_pubkey, &a.signature) {
            seen.push(a.signer_pubkey);
        }
    }
    seen.len()
}

#[derive(Clone)]
struct MsPending {
    operator_id: [u8; 32],
    amount_sats: u64,
    approvals: Vec<Approval>,
    vetoed: bool,
    finalized: bool,
}

/// 3-of-5 committee-gated reserve (§11.2). `submit_claim` records a pending claim;
/// committee members [`add_approval`](Self::add_approval) over `claim_id`; a
/// challenger may [`veto_package`](Self::veto_package); `finalize_claim` succeeds
/// only when ≥ threshold distinct approvals are present, the backend is not paused,
/// and the claim is not vetoed.
#[derive(Clone)]
pub struct MultisigReserve {
    total_sats: u64,
    reserved_sats: u64,
    committee: Committee,
    paused: Option<PauseReason>,
    pending: HashMap<[u8; 32], MsPending>,
}

impl MultisigReserve {
    pub fn new(total_sats: u64, committee: Committee) -> Self {
        Self {
            total_sats,
            reserved_sats: 0,
            committee,
            paused: None,
            pending: HashMap::new(),
        }
    }

    pub fn committee(&self) -> &Committee {
        &self.committee
    }

    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    /// A challenger's veto package (§5.D9). Also reachable via the trait `veto`.
    pub fn veto_package(&mut self, h: ClaimHandle) -> Result<(), FinalizeError> {
        self.pending
            .get_mut(&h.0)
            .ok_or(FinalizeError::UnknownClaim)?
            .vetoed = true;
        Ok(())
    }

    /// Approvals currently held for a claim (distinct/authorized/valid count).
    pub fn approval_count(&self, h: ClaimHandle) -> usize {
        self.pending
            .get(&h.0)
            .map(|p| count_approvals(&self.committee, &h.0, &p.approvals))
            .unwrap_or(0)
    }
}

impl ReserveBackend for MultisigReserve {
    fn reserve_view(&self) -> ReserveView {
        ReserveView {
            total_sats: self.total_sats,
            reserved_sats: self.reserved_sats,
        }
    }

    fn credit(&mut self, sats: u64) {
        self.total_sats += sats;
    }

    fn clone_box(&self) -> Box<dyn ReserveBackend> {
        Box::new(self.clone())
    }

    fn resume(&mut self) {
        self.paused = None;
    }

    /// Record a committee member's approval. Ignores approvals from non-members,
    /// duplicates, and invalid signatures (idempotent).
    fn add_approval(&mut self, h: ClaimHandle, approval: Approval) -> Result<(), FinalizeError> {
        let authorized = self.committee.pubkeys.contains(&approval.signer_pubkey)
            && verify_ecdsa(&h.0, &approval.signer_pubkey, &approval.signature);
        let p = self
            .pending
            .get_mut(&h.0)
            .ok_or(FinalizeError::UnknownClaim)?;
        if authorized
            && !p
                .approvals
                .iter()
                .any(|a| a.signer_pubkey == approval.signer_pubkey)
        {
            p.approvals.push(approval);
        }
        Ok(())
    }

    fn veto(&mut self, h: ClaimHandle) -> Result<(), FinalizeError> {
        self.veto_package(h)
    }

    /// ROTATE_SHARD (§11.2): swap the committee and drop approvals collected under
    /// the old set — the new members must re-approve any pending claim.
    fn rotate_committee(&mut self, new: Committee) {
        self.committee = new;
        for p in self.pending.values_mut() {
            p.approvals.clear();
        }
    }

    fn submit_claim(&mut self, claim: &ReserveClaim) -> ClaimHandle {
        self.reserved_sats += claim.reimbursement_sats;
        self.pending.insert(
            claim.claim_id,
            MsPending {
                operator_id: claim.operator_id,
                amount_sats: claim.reimbursement_sats,
                approvals: Vec::new(),
                vetoed: false,
                finalized: false,
            },
        );
        ClaimHandle(claim.claim_id)
    }

    fn finalize_claim(&mut self, h: ClaimHandle) -> Result<ReserveSpend, FinalizeError> {
        if self.paused.is_some() {
            return Err(FinalizeError::Paused);
        }
        let need = self.committee.threshold as usize;
        let have = self.approval_count(h);
        let p = self.pending.get(&h.0).ok_or(FinalizeError::UnknownClaim)?;
        if p.finalized {
            return Err(FinalizeError::AlreadyFinalized);
        }
        if p.vetoed {
            return Err(FinalizeError::Vetoed);
        }
        if have < need {
            return Err(FinalizeError::ApprovalInsufficient { have, need });
        }
        if self.total_sats < p.amount_sats {
            return Err(FinalizeError::ReserveInsufficient);
        }
        let (operator_id, amount_sats) = (p.operator_id, p.amount_sats);
        self.total_sats -= amount_sats;
        self.reserved_sats -= amount_sats;
        self.pending.get_mut(&h.0).unwrap().finalized = true;
        Ok(ReserveSpend {
            claim_id: h.0,
            operator_id,
            amount_sats,
        })
    }

    fn emergency_pause(&mut self, reason: PauseReason) {
        self.paused = Some(reason);
    }

    fn reclaim_stale(&mut self, h: ClaimHandle) -> Result<u64, FinalizeError> {
        let p = self.pending.get(&h.0).ok_or(FinalizeError::UnknownClaim)?;
        if p.finalized {
            return Err(FinalizeError::AlreadyFinalized);
        }
        let amount = p.amount_sats;
        self.reserved_sats -= amount;
        self.pending.remove(&h.0);
        Ok(amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: u8, amount: u64) -> ReserveClaim {
        let mut c: ReserveClaim = base_claim();
        c.claim_id = [id; 32];
        c.operator_id = [0x20; 32];
        c.reimbursement_sats = amount;
        c
    }

    // A zeroed ReserveClaim with the few fields the backend reads.
    fn base_claim() -> ReserveClaim {
        ReserveClaim {
            claim_id: [0; 32],
            transition_type: 0,
            operator_id: [0; 32],
            prev_state_root: [0; 32],
            new_state_root: [0; 32],
            redemption_batch_root: [0; 32],
            oracle_batch_root: [0; 32],
            lock_batch_root: [0; 32],
            payout_batch_root: [0; 32],
            confirmation_batch_root: [0; 32],
            finalize_batch_root: [0; 32],
            burn_proof_batch_root: [0; 32],
            lineage_proof_batch_root: [0; 32],
            live_da_root: [0; 32],
            archival_da_root: [0; 32],
            l1_anchor: satusd_types::types::ClaimClock {
                l1_anchor_height: 0,
                l1_anchor_hash: [0; 32],
                l1_anchor_mtp: 0,
                l1_anchor_chain_time: 0,
                recent_header_chain: [[0u8; 80]; 12],
                oracle_epoch: 0,
                selected_oracle_price_e8: 0,
                max_epoch_lag_sec: 0,
                oracle_future_tolerance: 0,
            },
            reserve_shard_id: 0,
            reimbursement_sats: 0,
            proof_commitment: [0; 32],
            claim_expiry_height: 0,
            operator_signature: [0; 64],
        }
    }

    fn member(seed: u8) -> ([u8; 32], [u8; 33]) {
        use secp256k1::{Secp256k1, SecretKey};
        let secp = Secp256k1::new();
        let sk = [seed; 32];
        let pk = SecretKey::from_byte_array(sk)
            .unwrap()
            .public_key(&secp)
            .serialize();
        (sk, pk)
    }

    fn committee_3of5() -> (Committee, Vec<[u8; 32]>) {
        let members: Vec<_> = (1..=5u8).map(member).collect();
        let pubkeys = members.iter().map(|m| m.1).collect();
        let secrets = members.iter().map(|m| m.0).collect();
        (
            Committee {
                threshold: 3,
                pubkeys,
            },
            secrets,
        )
    }

    #[test]
    fn mock_reserve_immediate_payout() {
        let mut r = MockReserve::new(1_000_000);
        let h = r.submit_claim(&claim(0xaa, 80_000));
        assert_eq!(r.reserve_view().available_sats(), 920_000);
        let spend = r.finalize_claim(h).unwrap();
        assert_eq!(spend.amount_sats, 80_000);
        assert_eq!(
            r.reserve_view(),
            ReserveView {
                total_sats: 920_000,
                reserved_sats: 0
            }
        );
    }

    #[test]
    fn multisig_finalizes_at_threshold() {
        let (committee, secrets) = committee_3of5();
        let cid = [0xaa; 32];
        let mut r = MultisigReserve::new(1_000_000, committee);
        let h = r.submit_claim(&claim(0xaa, 80_000));

        // 2 approvals < 3 → refused.
        for sk in &secrets[0..2] {
            r.add_approval(h, sign_claim_approval(&cid, sk)).unwrap();
        }
        assert_eq!(
            r.finalize_claim(h),
            Err(FinalizeError::ApprovalInsufficient { have: 2, need: 3 })
        );

        // 3rd approval → finalizes and debits.
        r.add_approval(h, sign_claim_approval(&cid, &secrets[2]))
            .unwrap();
        let spend = r.finalize_claim(h).unwrap();
        assert_eq!(spend.amount_sats, 80_000);
        assert_eq!(r.reserve_view().total_sats, 920_000);
        // Idempotency: a second finalize is rejected.
        assert_eq!(r.finalize_claim(h), Err(FinalizeError::AlreadyFinalized));
    }

    #[test]
    fn outsider_and_duplicate_approvals_dont_count() {
        let (committee, secrets) = committee_3of5();
        let cid = [0xbb; 32];
        let mut r = MultisigReserve::new(1_000_000, committee);
        let h = r.submit_claim(&claim(0xbb, 50_000));

        // Same member twice + an outsider → only 1 distinct authorized approval.
        r.add_approval(h, sign_claim_approval(&cid, &secrets[0]))
            .unwrap();
        r.add_approval(h, sign_claim_approval(&cid, &secrets[0]))
            .unwrap();
        let (outsider_sk, _) = member(0x99);
        r.add_approval(h, sign_claim_approval(&cid, &outsider_sk))
            .unwrap();
        assert_eq!(r.approval_count(h), 1);
        assert_eq!(
            r.finalize_claim(h),
            Err(FinalizeError::ApprovalInsufficient { have: 1, need: 3 })
        );
    }

    #[test]
    fn veto_and_pause_block_finalize() {
        let (committee, secrets) = committee_3of5();
        let cid = [0xcc; 32];
        let mut r = MultisigReserve::new(1_000_000, committee);
        let h = r.submit_claim(&claim(0xcc, 40_000));
        for sk in &secrets[0..3] {
            r.add_approval(h, sign_claim_approval(&cid, sk)).unwrap();
        }

        // Challenger veto blocks an otherwise-approved claim.
        r.veto_package(h).unwrap();
        assert_eq!(r.finalize_claim(h), Err(FinalizeError::Vetoed));

        // Emergency pause blocks finalize regardless (runbook scenario 1/2).
        let mut r2 = MultisigReserve::new(1_000_000, committee_3of5().0);
        let h2 = r2.submit_claim(&claim(0xcc, 40_000));
        for sk in &secrets[0..3] {
            r2.add_approval(h2, sign_claim_approval(&cid, sk)).unwrap();
        }
        r2.emergency_pause(PauseReason::StateNodeBug);
        assert_eq!(r2.finalize_claim(h2), Err(FinalizeError::Paused));
        r2.resume();
        assert!(r2.finalize_claim(h2).is_ok());
    }

    #[test]
    fn reclaim_stale_frees_reservation() {
        let mut r = MultisigReserve::new(1_000_000, committee_3of5().0);
        let h = r.submit_claim(&claim(0xdd, 70_000));
        assert_eq!(r.reserve_view().reserved_sats, 70_000);
        assert_eq!(r.reclaim_stale(h).unwrap(), 70_000);
        assert_eq!(
            r.reserve_view(),
            ReserveView {
                total_sats: 1_000_000,
                reserved_sats: 0
            }
        );
    }

    #[test]
    fn committee_hash_matches_state_commitment() {
        let (committee, _) = committee_3of5();
        assert_eq!(
            committee.hash(),
            reserve_committee_hash(3, &committee.pubkeys)
        );
    }
}
