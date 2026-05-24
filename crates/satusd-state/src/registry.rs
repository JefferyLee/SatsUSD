//! ISSUER_REGISTER + RECLAIM_STALE_CLAIM verifiers (PRD §5.D10/§5.D11/§5.D12).
//!
//! - ISSUER_REGISTER (0x21): insert a fresh `IssuerPosition` into
//!   `issuer_positions_root` (zero balances, ACTIVE, sane multisig config).
//! - RECLAIM_STALE_CLAIM (0x30): after `claim_expiry_height` passes, any keeper
//!   flips a PENDING/CHALLENGED `PendingClaim` to RECLAIMED and frees its
//!   `reserved_pending_claim_sats` (§5.D12).
//!
//! OPERATOR_REGISTER (0x20) is intentionally **not** implemented here: the
//! `OperatorPosition` record has no §6 struct definition (only a registered
//! domain) and is absent from the §14 M2 milestone scope. Implementing it would
//! mean inventing unspecified protocol state; deferred until specced.
//!
//! Both verifiers compute the full expected post-state and assert equality
//! (immutability backstop). The current chain height for the expiry check is a
//! verified witness fact (sourced from the L1 anchor, like the SPV facts).

use satusd_crypto::smt;
use satusd_crypto::state::state_root_hash;
use satusd_types::derive::{issuer_position_hash, pending_claim_hash};
use satusd_types::types::{
    IssuerPosition, IssuerStatus, PendingClaim, PendingClaimStatus, StateRoot, TransitionType,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryRejectReason {
    BadStateLinkage,
    WrongTransitionType,
    PostStateMismatch,
    Overflow,
    // ISSUER_REGISTER
    IssuerAlreadyRegistered,
    IssuerNotFresh,
    BadMultisigConfig,
    // RECLAIM_STALE_CLAIM
    ClaimNotPresent,
    ClaimNotReclaimable,
    ClaimNotExpired,
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

fn check_linkage(
    prev: &StateRoot,
    new: &StateRoot,
    ttype: TransitionType,
) -> Result<(), RegistryRejectReason> {
    use RegistryRejectReason::*;
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

fn next_state(prev: &StateRoot, new: &StateRoot, ttype: TransitionType) -> StateRoot {
    let mut e = prev.clone();
    e.state_epoch = new.state_epoch;
    e.prev_state_root = new.prev_state_root;
    e.transition_type = ttype.as_u8();
    e
}

/// ISSUER_REGISTER: add `new_issuer` (must be absent) to `issuer_positions_root`.
pub fn verify_issuer_register(
    prev: &StateRoot,
    new: &StateRoot,
    new_issuer: &IssuerPosition,
    issuer_exclusion_proof: &[[u8; 32]],
) -> Result<(), RegistryRejectReason> {
    use RegistryRejectReason::*;
    check_linkage(prev, new, TransitionType::IssuerRegister)?;

    // A freshly registered issuer is ACTIVE with zeroed balances and no history.
    let fresh = new_issuer.status == IssuerStatus::Active
        && new_issuer.minted_satusd_atoms == 0
        && new_issuer.pending_mint_atoms == 0
        && new_issuer.reserve_deposits_sats == 0
        && new_issuer.collateral_ratio_ppm == 0
        && new_issuer.pending_mint_commitment.is_none()
        && new_issuer.last_deposit_txid.is_none()
        && new_issuer.freeze_reason.is_none();
    ensure!(fresh, IssuerNotFresh);

    // Sane multisig config: 1 ≤ threshold ≤ #keys and keys are distinct.
    let n = new_issuer.multisig_pubkeys.len();
    let t = new_issuer.multisig_threshold as usize;
    ensure!(n > 0 && (1..=n).contains(&t), BadMultisigConfig);
    let mut seen: Vec<[u8; 33]> = Vec::with_capacity(n);
    for pk in &new_issuer.multisig_pubkeys {
        ensure!(!seen.contains(pk), BadMultisigConfig);
        seen.push(*pk);
    }

    // Must not already be registered.
    ensure!(
        smt::verify_exclusion(
            &prev.issuer_positions_root,
            &new_issuer.issuer_id,
            issuer_exclusion_proof
        ),
        IssuerAlreadyRegistered
    );

    let mut expected = next_state(prev, new, TransitionType::IssuerRegister);
    expected.issuer_positions_root = smt::root_after_update(
        &new_issuer.issuer_id,
        &issuer_position_hash(new_issuer),
        issuer_exclusion_proof,
    );
    ensure!(*new == expected, PostStateMismatch);
    Ok(())
}

/// RECLAIM_STALE_CLAIM: flip an expired PENDING/CHALLENGED claim to RECLAIMED and
/// release its reserved sats (§5.D12). `current_height` is the chain tip from the
/// L1 anchor (verified fact this chunk).
pub fn verify_reclaim_stale_claim(
    prev: &StateRoot,
    new: &StateRoot,
    prev_claim: &PendingClaim,
    claim_membership_proof: &[[u8; 32]],
    current_height: u32,
) -> Result<(), RegistryRejectReason> {
    use RegistryRejectReason::*;
    check_linkage(prev, new, TransitionType::ReclaimStaleClaim)?;

    ensure!(
        matches!(
            prev_claim.status,
            PendingClaimStatus::Pending | PendingClaimStatus::Challenged
        ),
        ClaimNotReclaimable
    );
    ensure!(
        current_height > prev_claim.claim_expiry_height,
        ClaimNotExpired
    );
    ensure!(
        smt::verify_inclusion(
            &prev.pending_claim_root,
            &prev_claim.claim_id,
            &pending_claim_hash(prev_claim),
            claim_membership_proof
        ),
        ClaimNotPresent
    );

    let mut reclaimed = prev_claim.clone();
    reclaimed.status = PendingClaimStatus::Reclaimed;
    let new_reserved = prev
        .reserved_pending_claim_sats
        .checked_sub(prev_claim.reserved_sats)
        .ok_or(Overflow)?;

    let mut expected = next_state(prev, new, TransitionType::ReclaimStaleClaim);
    expected.reserved_pending_claim_sats = new_reserved;
    expected.pending_claim_root = smt::root_after_update(
        &prev_claim.claim_id,
        &pending_claim_hash(&reclaimed),
        claim_membership_proof,
    );
    ensure!(*new == expected, PostStateMismatch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_crypto::smt::SparseMerkleTree;

    fn base_state() -> StateRoot {
        let empty = SparseMerkleTree::new().root();
        StateRoot {
            protocol_version: 1,
            state_epoch: 10,
            prev_state_root: [0; 32],
            transition_type: 0,
            satusd_asset_family_id: [0x01; 32],
            sat_usd_supply_atoms: 0,
            reserve_btc_sats: 5_000_000_000,
            reserved_pending_claim_sats: 1_000_000,
            collateral_ratio_ppm: 0,
            emergency_tier: 0,
            oracle_set_hash: [0x02; 32],
            oracle_set_epoch: 3,
            latest_oracle_epoch_seen: 3,
            latest_oracle_price_e8: 5_000_000_000_000,
            issuer_positions_root: empty,
            operator_registry_root: [0x04; 32],
            lock_record_root: [0x05; 32],
            lock_consumed_root: [0x06; 32],
            lock_refund_root: [0x07; 32],
            redemption_nullifier_root: [0x08; 32],
            pending_claim_root: empty,
            live_da_root: [0x0a; 32],
            archival_da_root: [0x0b; 32],
            l1_anchor_hash: [0x0c; 32],
            l1_anchor_height: 840_000,
            l1_anchor_mtp: 1_700_000_000,
            l1_anchor_chain_time: 1_700_000_600,
        }
    }

    fn fresh_issuer(id: u8) -> IssuerPosition {
        IssuerPosition {
            issuer_id: [id; 32],
            status: IssuerStatus::Active,
            multisig_pubkeys: vec![[0x02; 33], [0x03; 33], [0x04; 33]],
            multisig_threshold: 2,
            reserve_deposits_sats: 0,
            minted_satusd_atoms: 0,
            pending_mint_atoms: 0,
            collateral_ratio_ppm: 0,
            last_deposit_txid: None,
            freeze_reason: None,
            registered_at_height: 840_000,
            pending_mint_commitment: None,
        }
    }

    fn pending_claim(expiry: u32, status: PendingClaimStatus) -> PendingClaim {
        PendingClaim {
            claim_id: [0x55; 32],
            operator_id: [0x56; 32],
            reserved_sats: 400_000,
            claim_created_height: 839_000,
            claim_expiry_height: expiry,
            status,
        }
    }

    fn with_epoch(prev: &StateRoot, ttype: TransitionType) -> StateRoot {
        let mut n = prev.clone();
        n.state_epoch = prev.state_epoch + 1;
        n.prev_state_root = state_root_hash(prev);
        n.transition_type = ttype.as_u8();
        n
    }

    // ---- ISSUER_REGISTER ----
    #[test]
    fn issuer_register_happy() {
        let prev = base_state();
        let iss = fresh_issuer(0xaa);
        let proof = SparseMerkleTree::new().prove(&iss.issuer_id);
        let mut new = with_epoch(&prev, TransitionType::IssuerRegister);
        new.issuer_positions_root =
            smt::root_after_update(&iss.issuer_id, &issuer_position_hash(&iss), &proof);
        verify_issuer_register(&prev, &new, &iss, &proof).expect("register ok");
    }

    #[test]
    fn issuer_register_already_present_rejected() {
        // issuer already in the tree ⇒ exclusion proof fails.
        let iss = fresh_issuer(0xaa);
        let mut t = SparseMerkleTree::new();
        t.insert(iss.issuer_id, &issuer_position_hash(&iss));
        let mut prev = base_state();
        prev.issuer_positions_root = t.root();
        let proof = t.prove(&iss.issuer_id);
        let new = with_epoch(&prev, TransitionType::IssuerRegister);
        assert_eq!(
            verify_issuer_register(&prev, &new, &iss, &proof),
            Err(RegistryRejectReason::IssuerAlreadyRegistered)
        );
    }

    #[test]
    fn issuer_register_bad_threshold_rejected() {
        let prev = base_state();
        let mut iss = fresh_issuer(0xaa);
        iss.multisig_threshold = 4; // > 3 keys
        let proof = SparseMerkleTree::new().prove(&iss.issuer_id);
        let new = with_epoch(&prev, TransitionType::IssuerRegister);
        assert_eq!(
            verify_issuer_register(&prev, &new, &iss, &proof),
            Err(RegistryRejectReason::BadMultisigConfig)
        );
    }

    #[test]
    fn issuer_register_nonzero_balance_rejected() {
        let prev = base_state();
        let mut iss = fresh_issuer(0xaa);
        iss.minted_satusd_atoms = 1; // not fresh
        let proof = SparseMerkleTree::new().prove(&iss.issuer_id);
        let new = with_epoch(&prev, TransitionType::IssuerRegister);
        assert_eq!(
            verify_issuer_register(&prev, &new, &iss, &proof),
            Err(RegistryRejectReason::IssuerNotFresh)
        );
    }

    // ---- RECLAIM_STALE_CLAIM ----
    fn claim_tree(c: &PendingClaim) -> (SparseMerkleTree, Vec<[u8; 32]>) {
        let mut t = SparseMerkleTree::new();
        t.insert(c.claim_id, &pending_claim_hash(c));
        let proof = t.prove(&c.claim_id);
        (t, proof)
    }

    #[test]
    fn reclaim_happy() {
        let c = pending_claim(839_500, PendingClaimStatus::Pending);
        let (t, proof) = claim_tree(&c);
        let mut prev = base_state();
        prev.pending_claim_root = t.root();

        let mut reclaimed = c.clone();
        reclaimed.status = PendingClaimStatus::Reclaimed;
        let mut new = with_epoch(&prev, TransitionType::ReclaimStaleClaim);
        new.reserved_pending_claim_sats = prev.reserved_pending_claim_sats - c.reserved_sats;
        new.pending_claim_root =
            smt::root_after_update(&c.claim_id, &pending_claim_hash(&reclaimed), &proof);

        verify_reclaim_stale_claim(&prev, &new, &c, &proof, 840_000).expect("reclaim ok");
    }

    #[test]
    fn reclaim_not_expired_rejected() {
        let c = pending_claim(840_500, PendingClaimStatus::Pending); // expiry > tip
        let (t, proof) = claim_tree(&c);
        let mut prev = base_state();
        prev.pending_claim_root = t.root();
        let new = with_epoch(&prev, TransitionType::ReclaimStaleClaim);
        assert_eq!(
            verify_reclaim_stale_claim(&prev, &new, &c, &proof, 840_000),
            Err(RegistryRejectReason::ClaimNotExpired)
        );
    }

    #[test]
    fn reclaim_already_finalized_rejected() {
        let c = pending_claim(839_500, PendingClaimStatus::Finalized);
        let (t, proof) = claim_tree(&c);
        let mut prev = base_state();
        prev.pending_claim_root = t.root();
        let new = with_epoch(&prev, TransitionType::ReclaimStaleClaim);
        assert_eq!(
            verify_reclaim_stale_claim(&prev, &new, &c, &proof, 840_000),
            Err(RegistryRejectReason::ClaimNotReclaimable)
        );
    }
}
