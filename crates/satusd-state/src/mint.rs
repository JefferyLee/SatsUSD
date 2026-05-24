//! MINT_COMMIT / MINT_FINALIZE software verifiers (PRD §5.D11, §8.1).
//!
//! Two-phase mint: COMMIT records reserve + a pending-mint commitment (no supply
//! increase); FINALIZE re-checks collateralization (DL-27) and mints supply.
//! The mint-capacity CR is re-computed on the **post-mint** supply at finalize,
//! so a price drop after commit forces a reject (I-04).
//!
//! Scope (this chunk): the accounting state machine over `StateRoot` scalars +
//! the issuer's `IssuerPosition` leaf (bound through `issuer_positions_root` via
//! an SMT proof). External facts — deposit SPV depth, multisig signatures, the
//! tapd mint proof, and oracle aggregation — are carried in the witness as
//! already-verified results; their cryptographic establishment is wired in later
//! chunks (SPV/D14, oracle/D7, lineage/DL-23).

use satusd_crypto::ecdsa::verify_ecdsa;
use satusd_crypto::smt;
use satusd_crypto::state::state_root_hash;
use satusd_types::derive::{issuer_position_hash, mint_commitment, mint_request_sighash};
use satusd_types::tier::{self, collateral_ratio_ppm};
use satusd_types::types::{IssuerPosition, IssuerStatus, StateRoot, TransitionType};

/// Minimum CR to mint, in ppm (§5.D8, default 200%).
pub const MIN_MINT_CR_PPM: u64 = 2_000_000;
/// Healthy-tier threshold, in ppm (§5.D8, 150%).
pub const TIER_HEALTHY_THRESHOLD_PPM: u64 = 1_500_000;
/// Deposit confirmation depth required before a mint may commit (§5.D11).
pub const DEPOSIT_MIN_CONFIRMATIONS: u32 = 6;

/// Why a mint transition was rejected. Variants map to the §13.3 I-tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MintRejectReason {
    BadStateLinkage,
    WrongTransitionType,
    ImmutableFieldChanged,
    SupplyChangedAtCommit,
    ReserveChangedAtFinalize,
    Overflow,
    IssuerMismatch,
    IssuerNotInState,
    IssuerNotActive, // I-05
    IssuerRootMismatch,
    DepositNotConfirmed,     // I-01
    DepositNotToReserve,     // I-02
    MultisigThresholdNotMet, // I-06
    PendingMintExists,
    ZeroMintAmount,
    InsufficientCollateralAtCommit { cr_ppm: u64 },
    NoPendingMint,                                    // I-07 (second finalize)
    CommitMismatch,                                   // I-03
    MintAnchorNotConfirmed,                           // I-03
    MintProofInvalid,                                 // I-03
    InsufficientCollateralAtFinalize { cr_ppm: u64 }, // I-04 (DL-27)
    TierNotHealthy { cr_ppm: u64 },
    ReserveMismatch,
    SupplyMismatch,
    OraclePriceMismatch,
    TierFieldMismatch,
    CrFieldMismatch,
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !($cond) {
            return Err($err);
        }
    };
}

/// An issuer multisig signature: a compact ECDSA sig from one authorized signer.
pub struct MultisigSig {
    pub signer_pubkey: [u8; 33],
    pub signature: [u8; 64],
}

/// MINT_COMMIT witness: the request plus the facts external verifiers establish.
pub struct MintCommitWitness {
    pub issuer_id: [u8; 32],
    pub requested_mint_atoms: u64,
    pub deposit_txid: [u8; 32],
    pub deposit_sats: u64,
    pub deposit_confirmations: u32,
    pub deposit_to_reserve: bool,
    pub asset_metadata_commitment: [u8; 32],
    /// Issuer 2-of-3 multisig signatures over `mint_request_sighash`.
    pub signatures: Vec<MultisigSig>,
    pub oracle_price_e8: u64,
}

/// Count distinct, authorized, valid signatures over `sighash` (§5.D11). A sig
/// counts only if its key is one of `issuer.multisig_pubkeys`, not already used,
/// and the ECDSA check passes.
fn count_valid_multisig(
    sighash: &[u8; 32],
    issuer: &IssuerPosition,
    sigs: &[MultisigSig],
) -> usize {
    let mut seen: Vec<[u8; 33]> = Vec::new();
    let mut count = 0;
    for s in sigs {
        if !issuer.multisig_pubkeys.contains(&s.signer_pubkey) || seen.contains(&s.signer_pubkey) {
            continue;
        }
        if verify_ecdsa(sighash, &s.signer_pubkey, &s.signature) {
            seen.push(s.signer_pubkey);
            count += 1;
        }
    }
    count
}

/// MINT_FINALIZE witness.
pub struct MintFinalizeWitness {
    pub issuer_id: [u8; 32],
    pub requested_mint_atoms: u64,
    pub deposit_txid: [u8; 32],
    pub asset_metadata_commitment: [u8; 32],
    pub mint_anchor_confirmations: u32,
    pub mint_proof_ok: bool,
    pub oracle_price_e8: u64,
}

/// Linkage + immutable-field checks common to both mint phases.
fn check_common(
    prev: &StateRoot,
    new: &StateRoot,
    ttype: TransitionType,
) -> Result<(), MintRejectReason> {
    use MintRejectReason::*;
    ensure!(new.transition_type == ttype.as_u8(), WrongTransitionType);
    ensure!(
        new.prev_state_root == state_root_hash(prev),
        BadStateLinkage
    );
    ensure!(
        prev.state_epoch.checked_add(1) == Some(new.state_epoch),
        BadStateLinkage
    );
    let immutable = prev.satusd_asset_family_id == new.satusd_asset_family_id
        && prev.oracle_set_hash == new.oracle_set_hash
        && prev.oracle_set_epoch == new.oracle_set_epoch
        && prev.operator_registry_root == new.operator_registry_root
        && prev.lock_record_root == new.lock_record_root
        && prev.lock_consumed_root == new.lock_consumed_root
        && prev.lock_refund_root == new.lock_refund_root
        && prev.redemption_nullifier_root == new.redemption_nullifier_root
        && prev.pending_claim_root == new.pending_claim_root
        && prev.live_da_root == new.live_da_root
        && prev.archival_da_root == new.archival_da_root
        && prev.reserved_pending_claim_sats == new.reserved_pending_claim_sats;
    ensure!(immutable, ImmutableFieldChanged);
    Ok(())
}

/// Verify the new StateRoot's own `emergency_tier` + `collateral_ratio_ppm`
/// reflect (reserve, supply, price). Zero supply ⇒ Healthy / cr field 0.
fn check_state_tier(new: &StateRoot, price_e8: u64) -> Result<(), MintRejectReason> {
    let cr = collateral_ratio_ppm(new.reserve_btc_sats, new.sat_usd_supply_atoms, price_e8);
    let expected_tier =
        tier::recompute_tier(new.reserve_btc_sats, new.sat_usd_supply_atoms, price_e8);
    ensure!(
        new.emergency_tier == expected_tier.as_u8(),
        MintRejectReason::TierFieldMismatch
    );
    ensure!(
        new.collateral_ratio_ppm == cr.unwrap_or(0),
        MintRejectReason::CrFieldMismatch
    );
    Ok(())
}

/// Bind `issuer` to `root` via the SMT membership `proof` (key = issuer_id).
fn issuer_in_state(
    root: &[u8; 32],
    issuer: &IssuerPosition,
    proof: &[[u8; 32]],
) -> Result<(), MintRejectReason> {
    ensure!(
        smt::verify_inclusion(
            root,
            &issuer.issuer_id,
            &issuer_position_hash(issuer),
            proof
        ),
        MintRejectReason::IssuerNotInState
    );
    Ok(())
}

/// MINT_COMMIT (§5.D11 stage 1).
pub fn verify_mint_commit(
    prev_state: &StateRoot,
    new_state: &StateRoot,
    prev_issuer: &IssuerPosition,
    issuer_proof: &[[u8; 32]],
    w: &MintCommitWitness,
) -> Result<(), MintRejectReason> {
    use MintRejectReason::*;

    check_common(prev_state, new_state, TransitionType::MintCommit)?;
    ensure!(
        new_state.sat_usd_supply_atoms == prev_state.sat_usd_supply_atoms,
        SupplyChangedAtCommit
    );

    ensure!(prev_issuer.issuer_id == w.issuer_id, IssuerMismatch);
    issuer_in_state(&prev_state.issuer_positions_root, prev_issuer, issuer_proof)?;

    ensure!(prev_issuer.status == IssuerStatus::Active, IssuerNotActive); // I-05
    ensure!(
        w.deposit_confirmations >= DEPOSIT_MIN_CONFIRMATIONS,
        DepositNotConfirmed
    ); // I-01
    ensure!(w.deposit_to_reserve, DepositNotToReserve); // I-02
    let sighash = mint_request_sighash(
        &w.issuer_id,
        w.requested_mint_atoms,
        w.deposit_sats,
        &w.deposit_txid,
        &w.asset_metadata_commitment,
    );
    ensure!(
        count_valid_multisig(&sighash, prev_issuer, &w.signatures)
            >= prev_issuer.multisig_threshold as usize,
        MultisigThresholdNotMet // I-06
    );
    ensure!(
        prev_issuer.pending_mint_commitment.is_none(),
        PendingMintExists
    );
    ensure!(w.requested_mint_atoms > 0, ZeroMintAmount);

    let reserve_total = prev_state
        .reserve_btc_sats
        .checked_add(w.deposit_sats)
        .ok_or(Overflow)?;
    let post_supply = prev_state
        .sat_usd_supply_atoms
        .checked_add(w.requested_mint_atoms)
        .ok_or(Overflow)?;
    // Commit-time pre-check on post-mint supply (finalize re-checks via DL-27).
    let cr = collateral_ratio_ppm(reserve_total, post_supply, w.oracle_price_e8)
        .expect("post_supply > 0");
    ensure!(
        cr >= MIN_MINT_CR_PPM,
        InsufficientCollateralAtCommit { cr_ppm: cr }
    );

    // Expected post-state issuer position.
    let commitment = mint_commitment(
        w.requested_mint_atoms,
        &w.asset_metadata_commitment,
        &w.deposit_txid,
    );
    let mut exp = prev_issuer.clone();
    exp.reserve_deposits_sats = exp
        .reserve_deposits_sats
        .checked_add(w.deposit_sats)
        .ok_or(Overflow)?;
    exp.pending_mint_atoms = exp
        .pending_mint_atoms
        .checked_add(w.requested_mint_atoms)
        .ok_or(Overflow)?;
    exp.last_deposit_txid = Some(w.deposit_txid);
    exp.pending_mint_commitment = Some(commitment);

    ensure!(new_state.reserve_btc_sats == reserve_total, ReserveMismatch);
    ensure!(
        new_state.latest_oracle_price_e8 == w.oracle_price_e8,
        OraclePriceMismatch
    );
    check_state_tier(new_state, w.oracle_price_e8)?;

    let new_root = smt::root_after_update(&w.issuer_id, &issuer_position_hash(&exp), issuer_proof);
    ensure!(
        new_state.issuer_positions_root == new_root,
        IssuerRootMismatch
    );
    Ok(())
}

/// MINT_FINALIZE (§5.D11 stage 2; DL-27 finalize-time CR re-check).
pub fn verify_mint_finalize(
    prev_state: &StateRoot,
    new_state: &StateRoot,
    prev_issuer: &IssuerPosition,
    issuer_proof: &[[u8; 32]],
    w: &MintFinalizeWitness,
) -> Result<(), MintRejectReason> {
    use MintRejectReason::*;

    check_common(prev_state, new_state, TransitionType::MintFinalize)?;
    ensure!(
        new_state.reserve_btc_sats == prev_state.reserve_btc_sats,
        ReserveChangedAtFinalize
    );

    ensure!(prev_issuer.issuer_id == w.issuer_id, IssuerMismatch);
    issuer_in_state(&prev_state.issuer_positions_root, prev_issuer, issuer_proof)?;
    ensure!(prev_issuer.status == IssuerStatus::Active, IssuerNotActive);

    // Must match the outstanding commit (I-03) and not already be finalized (I-07).
    let commitment = mint_commitment(
        w.requested_mint_atoms,
        &w.asset_metadata_commitment,
        &w.deposit_txid,
    );
    match prev_issuer.pending_mint_commitment {
        None => return Err(NoPendingMint),
        Some(c) => ensure!(c == commitment, CommitMismatch),
    }
    ensure!(
        w.mint_anchor_confirmations >= DEPOSIT_MIN_CONFIRMATIONS,
        MintAnchorNotConfirmed
    );
    ensure!(w.mint_proof_ok, MintProofInvalid);

    // DL-27: re-check CR on post-mint supply with the finalize-time price.
    let post_supply = prev_state
        .sat_usd_supply_atoms
        .checked_add(w.requested_mint_atoms)
        .ok_or(Overflow)?;
    let cr = collateral_ratio_ppm(prev_state.reserve_btc_sats, post_supply, w.oracle_price_e8)
        .expect("post_supply > 0");
    ensure!(
        cr >= MIN_MINT_CR_PPM,
        InsufficientCollateralAtFinalize { cr_ppm: cr }
    );
    ensure!(
        cr >= TIER_HEALTHY_THRESHOLD_PPM,
        TierNotHealthy { cr_ppm: cr }
    );

    // Expected post-state issuer position.
    let mut exp = prev_issuer.clone();
    exp.minted_satusd_atoms = exp
        .minted_satusd_atoms
        .checked_add(w.requested_mint_atoms)
        .ok_or(Overflow)?;
    exp.pending_mint_atoms = exp
        .pending_mint_atoms
        .checked_sub(w.requested_mint_atoms)
        .ok_or(Overflow)?;
    exp.pending_mint_commitment = None;

    ensure!(
        new_state.sat_usd_supply_atoms == post_supply,
        SupplyMismatch
    );
    ensure!(
        new_state.latest_oracle_price_e8 == w.oracle_price_e8,
        OraclePriceMismatch
    );
    check_state_tier(new_state, w.oracle_price_e8)?;

    let new_root = smt::root_after_update(&w.issuer_id, &issuer_position_hash(&exp), issuer_proof);
    ensure!(
        new_state.issuer_positions_root == new_root,
        IssuerRootMismatch
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_crypto::smt::SparseMerkleTree;
    use secp256k1::{Message, Secp256k1, SecretKey};

    const PRICE_50K: u64 = 5_000_000_000_000; // $50,000 × 10^8
    const ISSUER_ID: [u8; 32] = [0xab; 32];
    const DEPOSIT_TXID: [u8; 32] = [0xcd; 32];
    const META: [u8; 32] = [0xef; 32];

    fn signer_keys() -> [SecretKey; 3] {
        [
            SecretKey::from_byte_array([0x11; 32]).unwrap(),
            SecretKey::from_byte_array([0x22; 32]).unwrap(),
            SecretKey::from_byte_array([0x33; 32]).unwrap(),
        ]
    }

    fn signer_pubkeys() -> Vec<[u8; 33]> {
        let secp = Secp256k1::new();
        signer_keys()
            .iter()
            .map(|sk| sk.public_key(&secp).serialize())
            .collect()
    }

    fn sign(sk: &SecretKey, sighash: &[u8; 32]) -> [u8; 64] {
        Secp256k1::new()
            .sign_ecdsa(Message::from_digest(*sighash), sk)
            .serialize_compact()
    }

    fn issuer(pending: Option<[u8; 32]>, status: IssuerStatus) -> IssuerPosition {
        IssuerPosition {
            issuer_id: ISSUER_ID,
            status,
            multisig_pubkeys: signer_pubkeys(),
            multisig_threshold: 2,
            reserve_deposits_sats: 0,
            minted_satusd_atoms: 0,
            pending_mint_atoms: 0,
            collateral_ratio_ppm: 0,
            last_deposit_txid: None,
            freeze_reason: None,
            registered_at_height: 100,
            pending_mint_commitment: pending,
        }
    }

    fn base_state(supply: u64, reserve: u64, issuer_root: [u8; 32]) -> StateRoot {
        StateRoot {
            protocol_version: 1,
            state_epoch: 10,
            prev_state_root: [0; 32],
            transition_type: 0,
            satusd_asset_family_id: [0x01; 32],
            sat_usd_supply_atoms: supply,
            reserve_btc_sats: reserve,
            reserved_pending_claim_sats: 0,
            collateral_ratio_ppm: 0,
            emergency_tier: 0,
            oracle_set_hash: [0x02; 32],
            oracle_set_epoch: 3,
            latest_oracle_epoch_seen: 3,
            latest_oracle_price_e8: PRICE_50K,
            issuer_positions_root: issuer_root,
            operator_registry_root: [0x04; 32],
            lock_record_root: [0x05; 32],
            lock_consumed_root: [0x06; 32],
            lock_refund_root: [0x07; 32],
            redemption_nullifier_root: [0x08; 32],
            pending_claim_root: [0x09; 32],
            live_da_root: [0x0a; 32],
            archival_da_root: [0x0b; 32],
            l1_anchor_hash: [0x0c; 32],
            l1_anchor_height: 840_000,
            l1_anchor_mtp: 1_700_000_000,
            l1_anchor_chain_time: 1_700_000_600,
        }
    }

    /// Build an SMT holding `issuer` (+ one decoy leaf); return (tree, proof, root).
    fn tree_with(issuer: &IssuerPosition) -> (SparseMerkleTree, Vec<[u8; 32]>, [u8; 32]) {
        let mut t = SparseMerkleTree::new();
        t.insert([0x77; 32], &[0x11; 32]); // decoy
        t.insert(issuer.issuer_id, &issuer_position_hash(issuer));
        let proof = t.prove(&issuer.issuer_id);
        let root = t.root();
        (t, proof, root)
    }

    /// Recompute a StateRoot's own tier/cr fields and issuer root for a candidate
    /// post-state. `supply`/`reserve` are the post-state scalars.
    fn make_new(
        prev: &StateRoot,
        ttype: TransitionType,
        supply: u64,
        reserve: u64,
        price: u64,
        new_issuer_root: [u8; 32],
    ) -> StateRoot {
        let mut n = prev.clone();
        n.state_epoch = prev.state_epoch + 1;
        n.prev_state_root = state_root_hash(prev);
        n.transition_type = ttype.as_u8();
        n.sat_usd_supply_atoms = supply;
        n.reserve_btc_sats = reserve;
        n.latest_oracle_price_e8 = price;
        n.collateral_ratio_ppm = collateral_ratio_ppm(reserve, supply, price).unwrap_or(0);
        n.emergency_tier = tier::recompute_tier(reserve, supply, price).as_u8();
        n.issuer_positions_root = new_issuer_root;
        n
    }

    /// A commit witness signed by the first `n_sigs` issuer multisig keys.
    fn signed_commit_witness(n_sigs: usize) -> MintCommitWitness {
        let requested_mint_atoms = 100_000_000u64; // $1M
        let deposit_sats = 4_000_000_000u64; // 40 BTC ⇒ 400% post-mint
        let sighash = mint_request_sighash(
            &ISSUER_ID,
            requested_mint_atoms,
            deposit_sats,
            &DEPOSIT_TXID,
            &META,
        );
        let keys = signer_keys();
        let pks = signer_pubkeys();
        let signatures = (0..n_sigs)
            .map(|i| MultisigSig {
                signer_pubkey: pks[i],
                signature: sign(&keys[i], &sighash),
            })
            .collect();
        MintCommitWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms,
            deposit_txid: DEPOSIT_TXID,
            deposit_sats,
            deposit_confirmations: 6,
            deposit_to_reserve: true,
            asset_metadata_commitment: META,
            signatures,
            oracle_price_e8: PRICE_50K,
        }
    }

    fn commit_witness() -> MintCommitWitness {
        signed_commit_witness(2)
    }

    /// Drive a full happy-path commit and return (post_state, post_issuer).
    fn do_commit(w: &MintCommitWitness) -> (StateRoot, IssuerPosition) {
        let prev_issuer = issuer(None, IssuerStatus::Active);
        let (_t, proof, root) = tree_with(&prev_issuer);
        let prev = base_state(0, 0, root);

        let mut exp = prev_issuer.clone();
        exp.reserve_deposits_sats += w.deposit_sats;
        exp.pending_mint_atoms += w.requested_mint_atoms;
        exp.last_deposit_txid = Some(w.deposit_txid);
        exp.pending_mint_commitment = Some(mint_commitment(
            w.requested_mint_atoms,
            &w.asset_metadata_commitment,
            &w.deposit_txid,
        ));
        let new_root = smt::root_after_update(&ISSUER_ID, &issuer_position_hash(&exp), &proof);
        let new = make_new(
            &prev,
            TransitionType::MintCommit,
            0,
            w.deposit_sats,
            w.oracle_price_e8,
            new_root,
        );
        verify_mint_commit(&prev, &new, &prev_issuer, &proof, w).expect("commit ok");
        (new, exp)
    }

    #[test]
    fn happy_path_commit_then_finalize() {
        let cw = commit_witness();
        let (commit_state, committed_issuer) = do_commit(&cw);

        // Finalize against the committed state.
        let (_t, proof, root) = tree_with(&committed_issuer);
        assert_eq!(root, commit_state.issuer_positions_root);
        let fw = MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: cw.requested_mint_atoms,
            deposit_txid: DEPOSIT_TXID,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        };
        let mut exp = committed_issuer.clone();
        exp.minted_satusd_atoms += fw.requested_mint_atoms;
        exp.pending_mint_atoms -= fw.requested_mint_atoms;
        exp.pending_mint_commitment = None;
        let new_root = smt::root_after_update(&ISSUER_ID, &issuer_position_hash(&exp), &proof);
        let new = make_new(
            &commit_state,
            TransitionType::MintFinalize,
            fw.requested_mint_atoms, // supply 0 + minted
            commit_state.reserve_btc_sats,
            PRICE_50K,
            new_root,
        );
        verify_mint_finalize(&commit_state, &new, &committed_issuer, &proof, &fw)
            .expect("finalize");
    }

    #[test]
    fn i01_deposit_unconfirmed_rejected() {
        let prev_issuer = issuer(None, IssuerStatus::Active);
        let (_t, proof, root) = tree_with(&prev_issuer);
        let prev = base_state(0, 0, root);
        let mut w = commit_witness();
        w.deposit_confirmations = 5;
        let new = make_new(
            &prev,
            TransitionType::MintCommit,
            0,
            w.deposit_sats,
            PRICE_50K,
            root,
        );
        assert_eq!(
            verify_mint_commit(&prev, &new, &prev_issuer, &proof, &w),
            Err(MintRejectReason::DepositNotConfirmed)
        );
    }

    #[test]
    fn i02_deposit_to_non_reserve_rejected() {
        let prev_issuer = issuer(None, IssuerStatus::Active);
        let (_t, proof, root) = tree_with(&prev_issuer);
        let prev = base_state(0, 0, root);
        let mut w = commit_witness();
        w.deposit_to_reserve = false;
        let new = make_new(
            &prev,
            TransitionType::MintCommit,
            0,
            w.deposit_sats,
            PRICE_50K,
            root,
        );
        assert_eq!(
            verify_mint_commit(&prev, &new, &prev_issuer, &proof, &w),
            Err(MintRejectReason::DepositNotToReserve)
        );
    }

    #[test]
    fn i03_finalize_anchor_mismatch_rejected() {
        let cw = commit_witness();
        let (commit_state, committed_issuer) = do_commit(&cw);
        let (_t, proof, _root) = tree_with(&committed_issuer);
        let fw = MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: cw.requested_mint_atoms,
            deposit_txid: DEPOSIT_TXID,
            asset_metadata_commitment: [0x00; 32], // ≠ committed META
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        };
        let new = make_new(
            &commit_state,
            TransitionType::MintFinalize,
            fw.requested_mint_atoms,
            commit_state.reserve_btc_sats,
            PRICE_50K,
            commit_state.issuer_positions_root,
        );
        assert_eq!(
            verify_mint_finalize(&commit_state, &new, &committed_issuer, &proof, &fw),
            Err(MintRejectReason::CommitMismatch)
        );
    }

    #[test]
    fn i04_dl27_finalize_cr_drop_rejected() {
        // Commit at 400% (40 BTC @ $50k for $1M). Finalize after price falls to
        // $12k ⇒ post-mint CR = 40 * 12000 / 1M = 48% < 200% ⇒ reject.
        let cw = commit_witness();
        let (commit_state, committed_issuer) = do_commit(&cw);
        let (_t, proof, _root) = tree_with(&committed_issuer);
        let crash_price = 1_200_000_000_000u64; // $12,000 × 10^8
        let fw = MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: cw.requested_mint_atoms,
            deposit_txid: DEPOSIT_TXID,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: crash_price,
        };
        let new = make_new(
            &commit_state,
            TransitionType::MintFinalize,
            fw.requested_mint_atoms,
            commit_state.reserve_btc_sats,
            crash_price,
            commit_state.issuer_positions_root,
        );
        match verify_mint_finalize(&commit_state, &new, &committed_issuer, &proof, &fw) {
            Err(MintRejectReason::InsufficientCollateralAtFinalize { .. }) => {}
            other => panic!("expected DL-27 reject, got {other:?}"),
        }
    }

    #[test]
    fn i05_frozen_issuer_rejected() {
        let prev_issuer = issuer(None, IssuerStatus::Frozen);
        let (_t, proof, root) = tree_with(&prev_issuer);
        let prev = base_state(0, 0, root);
        let w = commit_witness();
        let new = make_new(
            &prev,
            TransitionType::MintCommit,
            0,
            w.deposit_sats,
            PRICE_50K,
            root,
        );
        assert_eq!(
            verify_mint_commit(&prev, &new, &prev_issuer, &proof, &w),
            Err(MintRejectReason::IssuerNotActive)
        );
    }

    #[test]
    fn i06_insufficient_multisig_rejected() {
        let prev_issuer = issuer(None, IssuerStatus::Active);
        let (_t, proof, root) = tree_with(&prev_issuer);
        let prev = base_state(0, 0, root);
        let w = signed_commit_witness(1); // only 1-of-2 signatures
        let new = make_new(
            &prev,
            TransitionType::MintCommit,
            0,
            w.deposit_sats,
            PRICE_50K,
            root,
        );
        assert_eq!(
            verify_mint_commit(&prev, &new, &prev_issuer, &proof, &w),
            Err(MintRejectReason::MultisigThresholdNotMet)
        );
    }

    #[test]
    fn i07_double_finalize_rejected() {
        let cw = commit_witness();
        let (commit_state, committed_issuer) = do_commit(&cw);

        // First finalize → post-state issuer has pending_mint_commitment = None.
        let mut finalized = committed_issuer.clone();
        finalized.minted_satusd_atoms += cw.requested_mint_atoms;
        finalized.pending_mint_atoms -= cw.requested_mint_atoms;
        finalized.pending_mint_commitment = None;
        let (_t, proof, root) = tree_with(&finalized);
        let final_state = make_new(
            &commit_state,
            TransitionType::MintFinalize,
            cw.requested_mint_atoms,
            commit_state.reserve_btc_sats,
            PRICE_50K,
            root,
        );

        // Second finalize attempts against the already-finalized issuer.
        let fw = MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: cw.requested_mint_atoms,
            deposit_txid: DEPOSIT_TXID,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        };
        let new2 = make_new(
            &final_state,
            TransitionType::MintFinalize,
            final_state.sat_usd_supply_atoms,
            final_state.reserve_btc_sats,
            PRICE_50K,
            final_state.issuer_positions_root,
        );
        assert_eq!(
            verify_mint_finalize(&final_state, &new2, &finalized, &proof, &fw),
            Err(MintRejectReason::NoPendingMint)
        );
    }
}
