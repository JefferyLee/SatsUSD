//! Protocol hash derivations (PRD §5.D1, §5.D2/§18.1, §5.D3, §5.D13, §6.2).
//!
//! All use the 32-byte zero-padded domain tag convention frozen in `domain`.

use crate::domain;
use crate::encoding::{canonical_encode, Encoder};
use crate::hash::{sha256, tagged_hash};
use crate::types::{
    IssuerPosition, LockRecord, OperatorPosition, OutPoint, PendingClaim, RedeemIntent,
    ReserveClaim,
};

/// `asset_family_id` (§5.D1).
pub fn asset_family_id(
    genesis_asset_id: &[u8; 32],
    group_key: &[u8; 33],
    chain_id: u8,
) -> [u8; 32] {
    let tag = domain::ASSET_FAMILY.as_bytes();
    sha256(&[tag, genesis_asset_id, group_key, &[chain_id]])
}

/// `redeem_intent_hash` (§5.D2 / §18.1).
pub fn redeem_intent_hash(intent: &RedeemIntent) -> [u8; 32] {
    tagged_hash(domain::REDEEM_INTENT, &canonical_encode(intent))
}

/// `lock_tweak` (§5.D3). The secp256k1 TapTweak that consumes this is off-circuit.
pub fn lock_tweak(redeem_intent_hash: &[u8; 32], payment_hash: &[u8; 32]) -> [u8; 32] {
    let tag = domain::LOCK_TWEAK.as_bytes();
    sha256(&[tag, redeem_intent_hash, payment_hash])
}

/// `redemption_nullifier` (§6.2).
pub fn redemption_nullifier(
    lock_anchor_outpoint: &OutPoint,
    lock_script_key: &[u8; 32],
    redeem_intent_hash: &[u8; 32],
) -> [u8; 32] {
    let tag = domain::REDEMPTION_NULLIFIER.as_bytes();
    sha256(&[
        tag,
        &lock_anchor_outpoint.txid,
        &lock_anchor_outpoint.vout.to_be_bytes(),
        lock_script_key,
        redeem_intent_hash,
    ])
}

/// `claim_id` (§5.D13): over the claim without `claim_id`/`operator_signature`.
pub fn claim_id(claim: &ReserveClaim) -> [u8; 32] {
    let tag = domain::CLAIM_ID.as_bytes();
    let mut e = Encoder::new();
    claim.encode_for_claim_id(&mut e);
    sha256(&[tag, &e.into_bytes()])
}

/// `issuer_position_hash` (domain `SATUSD_ISSUER_POSITION_V1`, §18.2).
pub fn issuer_position_hash(p: &IssuerPosition) -> [u8; 32] {
    tagged_hash(domain::ISSUER_POSITION, &canonical_encode(p))
}

/// `pending_claim_hash` (domain `SATUSD_PENDING_CLAIM_V1`, §18.2).
pub fn pending_claim_hash(c: &PendingClaim) -> [u8; 32] {
    tagged_hash(domain::PENDING_CLAIM, &canonical_encode(c))
}

/// `lock_record_hash` (domain `SATUSD_LOCK_RECORD_V1`, §18.2): the SMT element
/// for the §5.D17 lock state machine (lock_record / consumed / refund sets).
pub fn lock_record_hash(r: &LockRecord) -> [u8; 32] {
    tagged_hash(domain::LOCK_RECORD, &canonical_encode(r))
}

/// `operator_position_hash` (domain `SATUSD_OPERATOR_POSITION_V1`, §18.2): the
/// SMT leaf value for an operator in `operator_registry_root`.
pub fn operator_position_hash(p: &OperatorPosition) -> [u8; 32] {
    tagged_hash(domain::OPERATOR_POSITION, &canonical_encode(p))
}

/// `oracle_set_hash` (domain `SATUSD_ORACLE_SET_V1`, §5.D7/§18.2): commitment to
/// an oracle signer set for an epoch. Signer pubkeys are sorted for determinism;
/// `StateRoot::oracle_set_hash` must equal this for the active set.
pub fn oracle_set_hash(oracle_set_epoch: u64, signer_pubkeys: &[[u8; 32]]) -> [u8; 32] {
    let mut sorted = signer_pubkeys.to_vec();
    sorted.sort_unstable();
    let mut e = Encoder::new();
    e.u64(oracle_set_epoch);
    e.seq(&sorted, |e, pk| e.fixed(pk));
    tagged_hash(domain::ORACLE_SET, &e.into_bytes())
}

/// `mint_request_sighash` (§5.D11, domain `SATUSD_MINT_REQUEST_V1`): the message
/// the issuer multisig signs to authorize a MINT_COMMIT. Binds the full economic
/// request (issuer, amount, deposit). Distinct preimage from `mint_commitment`.
pub fn mint_request_sighash(
    issuer_id: &[u8; 32],
    requested_mint_atoms: u64,
    deposit_sats: u64,
    deposit_txid: &[u8; 32],
    asset_metadata_commitment: &[u8; 32],
) -> [u8; 32] {
    let mut e = Encoder::new();
    e.fixed(issuer_id);
    e.u64(requested_mint_atoms);
    e.u64(deposit_sats);
    e.fixed(deposit_txid);
    e.fixed(asset_metadata_commitment);
    tagged_hash(domain::MINT_REQUEST, &e.into_bytes())
}

/// `mint_commitment` (§5.D11, domain `SATUSD_MINT_REQUEST_V1`): binds a
/// MINT_COMMIT so MINT_FINALIZE can match it (I-03) and reject a double finalize
/// (I-07). Stored as `IssuerPosition::pending_mint_commitment`.
pub fn mint_commitment(
    requested_mint_atoms: u64,
    asset_metadata_commitment: &[u8; 32],
    deposit_txid: &[u8; 32],
) -> [u8; 32] {
    let mut e = Encoder::new();
    e.u64(requested_mint_atoms);
    e.fixed(asset_metadata_commitment);
    e.fixed(deposit_txid);
    tagged_hash(domain::MINT_REQUEST, &e.into_bytes())
}
