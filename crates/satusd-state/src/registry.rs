//! Registry transitions (PRD §5.D10/§5.D11/§5.D12).
//!
//! - ISSUER_REGISTER (0x21): insert a fresh `IssuerPosition` into
//!   `issuer_positions_root` (zero balances, ACTIVE, sane multisig config).
//! - OPERATOR_REGISTER (0x20): insert a fresh bonded `OperatorPosition` into
//!   `operator_registry_root` (ADR-0021; bond ≥ OPERATOR_BOND_MULTIPLE × max_claim).
//! - RECLAIM_STALE_CLAIM (0x30): after `claim_expiry_height` passes, any keeper
//!   flips a PENDING/CHALLENGED `PendingClaim` to RECLAIMED and frees its
//!   `reserved_pending_claim_sats` (§5.D12).
//!
//! All verifiers compute the full expected post-state and assert equality
//! (immutability backstop). The current chain height for the expiry check is a
//! verified witness fact (sourced from the L1 anchor, like the SPV facts).

use satusd_crypto::smt;
use satusd_crypto::state::state_root_hash;
use satusd_types::derive::{issuer_position_hash, operator_position_hash, pending_claim_hash};
use satusd_types::types::{
    IssuerPosition, IssuerStatus, OperatorPosition, OperatorStatus, PendingClaim,
    PendingClaimStatus, StateRoot, TransitionType,
};

/// Operator bond must cover at least this multiple of its max single claim (§18.3).
pub const OPERATOR_BOND_MULTIPLE: u64 = 2;

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
    // OPERATOR_REGISTER
    OperatorAlreadyRegistered,
    OperatorNotFresh,
    ZeroMaxClaim,
    BondInsufficient,
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

fn next_state(prev: &StateRoot, ttype: TransitionType) -> StateRoot {
    let mut e = prev.clone();
    e.state_epoch = prev.state_epoch + 1;
    e.prev_state_root = state_root_hash(prev);
    e.transition_type = ttype.as_u8();
    e
}

/// `verify_*` = `apply_*` then whole-struct equality with the claimed `new`.
fn verified(
    expected: Result<StateRoot, RegistryRejectReason>,
    new: &StateRoot,
) -> Result<(), RegistryRejectReason> {
    match expected {
        Ok(e) if e == *new => Ok(()),
        Ok(_) => Err(RegistryRejectReason::PostStateMismatch),
        Err(e) => Err(e),
    }
}

/// ISSUER_REGISTER: add `new_issuer` (must be absent) to `issuer_positions_root`.
pub fn apply_issuer_register(
    prev: &StateRoot,
    new_issuer: &IssuerPosition,
    issuer_exclusion_proof: &[[u8; 32]],
) -> Result<StateRoot, RegistryRejectReason> {
    use RegistryRejectReason::*;

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

    let mut expected = next_state(prev, TransitionType::IssuerRegister);
    expected.issuer_positions_root = smt::root_after_update(
        &new_issuer.issuer_id,
        &issuer_position_hash(new_issuer),
        issuer_exclusion_proof,
    );
    Ok(expected)
}

pub fn verify_issuer_register(
    prev: &StateRoot,
    new: &StateRoot,
    new_issuer: &IssuerPosition,
    issuer_exclusion_proof: &[[u8; 32]],
) -> Result<(), RegistryRejectReason> {
    verified(
        apply_issuer_register(prev, new_issuer, issuer_exclusion_proof),
        new,
    )
}

/// OPERATOR_REGISTER: add a fresh bonded `new_op` (must be absent) to
/// `operator_registry_root` (§5.D10, ADR-0021).
pub fn apply_operator_register(
    prev: &StateRoot,
    new_op: &OperatorPosition,
    operator_exclusion_proof: &[[u8; 32]],
) -> Result<StateRoot, RegistryRejectReason> {
    use RegistryRejectReason::*;

    let fresh = new_op.status == OperatorStatus::Active
        && new_op.outstanding_claim_sats == 0
        && new_op.slashed_sats == 0;
    ensure!(fresh, OperatorNotFresh);
    ensure!(new_op.max_claim_sats > 0, ZeroMaxClaim);
    // bond_sats ≥ OPERATOR_BOND_MULTIPLE × max_claim_sats.
    let required = new_op
        .max_claim_sats
        .checked_mul(OPERATOR_BOND_MULTIPLE)
        .ok_or(Overflow)?;
    ensure!(new_op.bond_sats >= required, BondInsufficient);

    ensure!(
        smt::verify_exclusion(
            &prev.operator_registry_root,
            &new_op.operator_id,
            operator_exclusion_proof
        ),
        OperatorAlreadyRegistered
    );

    let mut expected = next_state(prev, TransitionType::OperatorRegister);
    expected.operator_registry_root = smt::root_after_update(
        &new_op.operator_id,
        &operator_position_hash(new_op),
        operator_exclusion_proof,
    );
    Ok(expected)
}

pub fn verify_operator_register(
    prev: &StateRoot,
    new: &StateRoot,
    new_op: &OperatorPosition,
    operator_exclusion_proof: &[[u8; 32]],
) -> Result<(), RegistryRejectReason> {
    verified(
        apply_operator_register(prev, new_op, operator_exclusion_proof),
        new,
    )
}

/// RECLAIM_STALE_CLAIM: flip an expired PENDING/CHALLENGED claim to RECLAIMED and
/// release its reserved sats (§5.D12). `current_height` is the chain tip from the
/// L1 anchor (verified fact this chunk).
pub fn apply_reclaim_stale_claim(
    prev: &StateRoot,
    prev_claim: &PendingClaim,
    claim_membership_proof: &[[u8; 32]],
    current_height: u32,
) -> Result<StateRoot, RegistryRejectReason> {
    use RegistryRejectReason::*;

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

    let mut expected = next_state(prev, TransitionType::ReclaimStaleClaim);
    expected.reserved_pending_claim_sats = new_reserved;
    expected.pending_claim_root = smt::root_after_update(
        &prev_claim.claim_id,
        &pending_claim_hash(&reclaimed),
        claim_membership_proof,
    );
    Ok(expected)
}

pub fn verify_reclaim_stale_claim(
    prev: &StateRoot,
    new: &StateRoot,
    prev_claim: &PendingClaim,
    claim_membership_proof: &[[u8; 32]],
    current_height: u32,
) -> Result<(), RegistryRejectReason> {
    verified(
        apply_reclaim_stale_claim(prev, prev_claim, claim_membership_proof, current_height),
        new,
    )
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
            operator_registry_root: empty,
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

    // ---- OPERATOR_REGISTER ----
    fn fresh_operator(id: u8) -> OperatorPosition {
        OperatorPosition {
            operator_id: [id; 32],
            status: OperatorStatus::Active,
            operator_pubkey: [0x02; 33],
            bond_sats: 2_000_000, // 2× max_claim
            max_claim_sats: 1_000_000,
            outstanding_claim_sats: 0,
            slashed_sats: 0,
            registered_at_height: 840_000,
        }
    }

    #[test]
    fn operator_register_happy() {
        let prev = base_state();
        let op = fresh_operator(0xbb);
        let proof = SparseMerkleTree::new().prove(&op.operator_id);
        let mut new = with_epoch(&prev, TransitionType::OperatorRegister);
        new.operator_registry_root =
            smt::root_after_update(&op.operator_id, &operator_position_hash(&op), &proof);
        verify_operator_register(&prev, &new, &op, &proof).expect("operator register ok");
    }

    #[test]
    fn operator_register_bond_insufficient_rejected() {
        let prev = base_state();
        let mut op = fresh_operator(0xbb);
        op.bond_sats = op.max_claim_sats; // 1× < 2× required
        let proof = SparseMerkleTree::new().prove(&op.operator_id);
        let new = with_epoch(&prev, TransitionType::OperatorRegister);
        assert_eq!(
            verify_operator_register(&prev, &new, &op, &proof),
            Err(RegistryRejectReason::BondInsufficient)
        );
    }

    #[test]
    fn operator_register_already_present_rejected() {
        let op = fresh_operator(0xbb);
        let mut t = SparseMerkleTree::new();
        t.insert(op.operator_id, &operator_position_hash(&op));
        let mut prev = base_state();
        prev.operator_registry_root = t.root();
        let proof = t.prove(&op.operator_id);
        let new = with_epoch(&prev, TransitionType::OperatorRegister);
        assert_eq!(
            verify_operator_register(&prev, &new, &op, &proof),
            Err(RegistryRejectReason::OperatorAlreadyRegistered)
        );
    }

    #[test]
    fn operator_register_not_fresh_rejected() {
        let prev = base_state();
        let mut op = fresh_operator(0xbb);
        op.slashed_sats = 1; // not fresh
        let proof = SparseMerkleTree::new().prove(&op.operator_id);
        let new = with_epoch(&prev, TransitionType::OperatorRegister);
        assert_eq!(
            verify_operator_register(&prev, &new, &op, &proof),
            Err(RegistryRejectReason::OperatorNotFresh)
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
