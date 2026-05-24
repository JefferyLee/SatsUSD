//! Protocol hash derivations (PRD §5.D1, §5.D2/§18.1, §5.D3, §5.D13, §6.2).
//!
//! All use the 32-byte zero-padded domain tag convention frozen in `domain`.

use crate::domain;
use crate::encoding::{canonical_encode, Encoder};
use crate::hash::{sha256, tagged_hash};
use crate::types::{OutPoint, RedeemIntent, ReserveClaim};

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
