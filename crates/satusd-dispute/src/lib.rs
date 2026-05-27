//! BitVM3 dispute resolution (PRD §5.D18, §11.4, M8) — the off-circuit model of
//! what the BitVM3 garbled-circuit sub-circuits enforce on-chain, plus a lightweight
//! optimistic backend (`PlaygroundReserve`, §11.3) to exercise the assert →
//! disprove → withdraw lifecycle.
//!
//! Two dispute kinds, each comparing the operator's committed value to the value a
//! challenger recomputes from the data (§5.D18):
//!
//! - **lineage-dispute (D15)**: the operator's `asset_proof_hash` vs the hash of the
//!   real TA lineage in the DA. If they differ (and the real lineage verifies), the
//!   operator committed a fake lineage → Disprove succeeds.
//! - **lock-binding-dispute (D18)**: the operator's `lock_script_key` vs
//!   `TapTweak(user_asset_refund_key, lock_tweak(rih, payment_hash))`. If they
//!   differ, the lock was not bound to the intent → Disprove succeeds.
//!
//! The actual BitVM3 garbled sub-circuit (SHA-256 chain + secp256k1 tweak), the
//! on-chain Assert/Disprove tx graph, and the setup ceremony are the
//! signet/upstream integration (G4 gate, G6 readiness) — deferred. This crate is
//! the verifier the dispute reduces to.

use satusd_crypto::nums::tap_tweak;
use satusd_reserve::{
    ClaimHandle, Disprove, FinalizeError, MockReserve, OptimisticEnforcementBackend, PauseReason,
    ReserveBackend, ReserveSpend, ReserveView, Txid, WindowStatus,
};
use satusd_ta_proof::{parse_proof_file, verify_lineage};
use satusd_types::derive::{lineage_proof_hash, lock_tweak, redeem_intent_hash};
use satusd_types::types::{RedeemIntent, ReserveClaim};

/// A challenger's dispute against an operator's claimed value (§5.D18).
pub enum DisputeWitness {
    /// D15: the operator's `asset_proof_hash` vs the real DA lineage `proof_file`.
    Lineage {
        claimed_asset_proof_hash: [u8; 32],
        proof_file: Vec<u8>,
    },
    /// D18: the operator's `lock_script_key` vs the key derived from `intent`.
    LockBinding {
        claimed_lock_script_key: [u8; 32],
        intent: Box<RedeemIntent>,
    },
}

/// The verdict of resolving a dispute — i.e. what the BitVM3 sub-circuit would
/// decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisputeOutcome {
    /// The operator's value is provably wrong; the disprove is valid.
    DisproveSucceeds {
        claimed: [u8; 32],
        correct: [u8; 32],
    },
    /// The operator's value matches the recomputation; no fraud.
    DisproveFails,
    /// The dispute itself is ill-formed (e.g. the offered lineage doesn't verify, so
    /// there is no correct value to disprove against).
    Malformed,
}

/// Resolve a dispute by recomputing the correct value and comparing it to the
/// operator's claim — the core of the BitVM3 lineage / lock-binding sub-circuits.
pub fn resolve_dispute(w: &DisputeWitness) -> DisputeOutcome {
    match w {
        DisputeWitness::Lineage {
            claimed_asset_proof_hash,
            proof_file,
        } => {
            // The challenger's lineage must itself be a valid genesis→head proof,
            // else there is nothing authoritative to disprove against.
            let Ok(file) = parse_proof_file(proof_file) else {
                return DisputeOutcome::Malformed;
            };
            let Ok(proofs) = file.parsed() else {
                return DisputeOutcome::Malformed;
            };
            if verify_lineage(&proofs).is_err() {
                return DisputeOutcome::Malformed;
            }
            let correct = lineage_proof_hash(proof_file);
            verdict(*claimed_asset_proof_hash, correct)
        }
        DisputeWitness::LockBinding {
            claimed_lock_script_key,
            intent,
        } => {
            let rih = redeem_intent_hash(intent);
            let correct = tap_tweak(
                &intent.user_asset_refund_key,
                &lock_tweak(&rih, &intent.payment_hash),
            );
            verdict(*claimed_lock_script_key, correct)
        }
    }
}

fn verdict(claimed: [u8; 32], correct: [u8; 32]) -> DisputeOutcome {
    if claimed == correct {
        DisputeOutcome::DisproveFails
    } else {
        DisputeOutcome::DisproveSucceeds { claimed, correct }
    }
}

/// Build the on-chain [`Disprove`] for a claim from a successful dispute outcome.
pub fn disprove_for(claim_id: [u8; 32], outcome: DisputeOutcome) -> Option<Disprove> {
    match outcome {
        DisputeOutcome::DisproveSucceeds { claimed, correct } => Some(Disprove {
            claim_id,
            claimed,
            correct,
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// PlaygroundReserve — a lightweight optimistic backend (§11.3) over MockReserve
// custody, to exercise assert → (disprove | window) → withdraw.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Assert {
    open_height: u32,
    disproven: bool,
}

/// Optimistic-enforcement model: a claim is asserted (window opens), and after the
/// window elapses with no successful disprove it may be withdrawn; a disprove
/// slashes it (the reservation is freed, never paid). BitVM3/BitVM2 replace this
/// with the on-chain tx graph (same trait — DL-7).
#[derive(Clone)]
pub struct PlaygroundReserve {
    custody: MockReserve,
    challenge_window: u32,
    now: u32,
    asserts: std::collections::HashMap<[u8; 32], Assert>,
}

impl PlaygroundReserve {
    pub fn new(total_sats: u64, challenge_window: u32) -> Self {
        Self {
            custody: MockReserve::new(total_sats),
            challenge_window,
            now: 0,
            asserts: std::collections::HashMap::new(),
        }
    }

    /// Advance the logical block height (drives the challenge window).
    pub fn set_height(&mut self, height: u32) {
        self.now = height;
    }
}

impl ReserveBackend for PlaygroundReserve {
    fn reserve_view(&self) -> ReserveView {
        self.custody.reserve_view()
    }
    fn credit(&mut self, sats: u64) {
        self.custody.credit(sats);
    }
    fn submit_claim(&mut self, claim: &ReserveClaim) -> ClaimHandle {
        self.custody.submit_claim(claim)
    }
    fn finalize_claim(&mut self, h: ClaimHandle) -> Result<ReserveSpend, FinalizeError> {
        self.custody.finalize_claim(h)
    }
    fn emergency_pause(&mut self, reason: PauseReason) {
        self.custody.emergency_pause(reason);
    }
    fn reclaim_stale(&mut self, h: ClaimHandle) -> Result<u64, FinalizeError> {
        self.custody.reclaim_stale(h)
    }
    fn clone_box(&self) -> Box<dyn ReserveBackend> {
        Box::new(self.clone())
    }
}

impl OptimisticEnforcementBackend for PlaygroundReserve {
    fn submit_assert(&mut self, claim: &ReserveClaim) -> Txid {
        self.asserts.insert(
            claim.claim_id,
            Assert {
                open_height: self.now,
                disproven: false,
            },
        );
        claim.claim_id // model txid
    }

    fn submit_disprove(&mut self, disprove: &Disprove) -> Txid {
        if let Some(a) = self.asserts.get_mut(&disprove.claim_id) {
            a.disproven = true;
        }
        // Slash: free the reservation so the fraudulent claim is never paid.
        let _ = self.custody.reclaim_stale(ClaimHandle(disprove.claim_id));
        disprove.claim_id // model txid
    }

    fn finalize_withdraw(&mut self, h: ClaimHandle) -> Result<ReserveSpend, FinalizeError> {
        match self.observe_challenge_window(h) {
            WindowStatus::Disproven => Err(FinalizeError::Disproven),
            WindowStatus::Open => Err(FinalizeError::WindowOpen),
            WindowStatus::Elapsed => self.custody.finalize_claim(h),
        }
    }

    fn observe_challenge_window(&self, h: ClaimHandle) -> WindowStatus {
        match self.asserts.get(&h.0) {
            None => WindowStatus::Open, // not yet asserted
            Some(a) if a.disproven => WindowStatus::Disproven,
            Some(a) if self.now >= a.open_height + self.challenge_window => WindowStatus::Elapsed,
            Some(_) => WindowStatus::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.trim()).unwrap()
    }

    const BURN: &str = include_str!("../../../integration/lineage_vectors/burn_to_sink.hex");

    fn intent() -> RedeemIntent {
        RedeemIntent {
            version: 1,
            network: 0,
            redemption_id: [0x01; 32],
            satusd_asset_family_id: [0x0c; 32],
            amount_satusd_atoms: 4_000,
            user_btc_refund_pubkey: [0x31; 32],
            user_btc_claim_pubkey: [0x32; 32],
            user_asset_refund_key: [0x33; 32],
            operator_id: Some([0x20; 32]),
            mode: 1,
            payment_hash: [0x44; 32],
            asset_lock_csv_delta: 144,
            btc_htlc_csv_delta: 144,
            max_operator_fee_bps: 50,
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x0c; 32],
            expiry_height: 850_000,
            nonce: [0x66; 32],
        }
    }

    fn correct_lock_key(it: &RedeemIntent) -> [u8; 32] {
        tap_tweak(
            &it.user_asset_refund_key,
            &lock_tweak(&redeem_intent_hash(it), &it.payment_hash),
        )
    }

    #[test]
    fn lineage_dispute_fails_on_honest_hash() {
        // The operator committed the correct hash of the real lineage ⇒ no fraud.
        let proof = bytes(BURN);
        let w = DisputeWitness::Lineage {
            claimed_asset_proof_hash: lineage_proof_hash(&proof),
            proof_file: proof,
        };
        assert_eq!(resolve_dispute(&w), DisputeOutcome::DisproveFails);
    }

    #[test]
    fn lineage_dispute_succeeds_on_fake_hash() {
        // The operator committed a hash that doesn't match the real lineage.
        let proof = bytes(BURN);
        let w = DisputeWitness::Lineage {
            claimed_asset_proof_hash: [0xff; 32],
            proof_file: proof.clone(),
        };
        assert_eq!(
            resolve_dispute(&w),
            DisputeOutcome::DisproveSucceeds {
                claimed: [0xff; 32],
                correct: lineage_proof_hash(&proof),
            }
        );
    }

    #[test]
    fn lineage_dispute_malformed_when_proof_invalid() {
        // A tampered proof doesn't verify ⇒ nothing authoritative to disprove.
        let mut proof = bytes(BURN);
        let mid = proof.len() / 2;
        proof[mid] ^= 0xff;
        let w = DisputeWitness::Lineage {
            claimed_asset_proof_hash: [0x00; 32],
            proof_file: proof,
        };
        assert_eq!(resolve_dispute(&w), DisputeOutcome::Malformed);
    }

    #[test]
    fn lock_binding_dispute_detects_wrong_key() {
        let it = intent();
        // Honest: the operator's key is the derived one ⇒ no fraud.
        assert_eq!(
            resolve_dispute(&DisputeWitness::LockBinding {
                claimed_lock_script_key: correct_lock_key(&it),
                intent: Box::new(it.clone()),
            }),
            DisputeOutcome::DisproveFails
        );
        // Fraud: a key not bound to the intent ⇒ disprove succeeds.
        assert!(matches!(
            resolve_dispute(&DisputeWitness::LockBinding {
                claimed_lock_script_key: [0x77; 32],
                intent: Box::new(it),
            }),
            DisputeOutcome::DisproveSucceeds { .. }
        ));
    }

    fn claim(amount: u64) -> ReserveClaim {
        let mut c = base_claim();
        c.claim_id = [0xaa; 32];
        c.operator_id = [0x20; 32];
        c.reimbursement_sats = amount;
        c
    }

    #[test]
    fn optimistic_valid_claim_withdraws_after_window() {
        let mut r = PlaygroundReserve::new(1_000_000, 36);
        let c = claim(80_000);
        r.submit_claim(&c);
        r.submit_assert(&c);
        let h = ClaimHandle(c.claim_id);
        assert_eq!(r.observe_challenge_window(h), WindowStatus::Open);
        assert_eq!(r.finalize_withdraw(h), Err(FinalizeError::WindowOpen));
        // Window elapses with no disprove → withdraw pays out.
        r.set_height(36);
        assert_eq!(r.observe_challenge_window(h), WindowStatus::Elapsed);
        assert_eq!(r.finalize_withdraw(h).unwrap().amount_sats, 80_000);
        assert_eq!(r.reserve_view().total_sats, 1_000_000 - 80_000);
    }

    #[test]
    fn optimistic_disproven_claim_is_slashed() {
        let mut r = PlaygroundReserve::new(1_000_000, 36);
        let c = claim(80_000);
        r.submit_claim(&c);
        r.submit_assert(&c);
        let h = ClaimHandle(c.claim_id);

        // A challenger disproves (e.g. fake lineage resolved to DisproveSucceeds).
        let dp = disprove_for(
            c.claim_id,
            DisputeOutcome::DisproveSucceeds {
                claimed: [0xff; 32],
                correct: [0x11; 32],
            },
        )
        .unwrap();
        r.submit_disprove(&dp);

        r.set_height(100); // even past the window
        assert_eq!(r.observe_challenge_window(h), WindowStatus::Disproven);
        assert_eq!(r.finalize_withdraw(h), Err(FinalizeError::Disproven));
        // Slashed: the reservation was freed, nothing paid.
        assert_eq!(
            r.reserve_view(),
            ReserveView {
                total_sats: 1_000_000,
                reserved_sats: 0
            }
        );
    }

    // A zeroed ReserveClaim with the fields the backend reads.
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
}
