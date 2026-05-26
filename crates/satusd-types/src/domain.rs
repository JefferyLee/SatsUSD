//! Domain separator registry (PRD §18.2).
//!
//! SPEC DECISION (ADR-pending): every domain-tagged hash prefixes the raw ASCII
//! name bytes with no padding: `SHA256(name_bytes || body)`. This matches
//! §5.D1/D3, §6.2, §5.D13, and is the only convention compatible with the
//! 33-byte registered name `SATUSD_BTC_PAYOUT_CONFIRMATION_V1`. §18.1's "zero-pad
//! to 32 bytes" phrasing is therefore incorrect and must be corrected there.

/// Returns the raw ASCII bytes of the domain separator (no padding).
pub fn domain_tag(name: &str) -> &[u8] {
    name.as_bytes()
}

pub const ASSET_FAMILY: &str = "SATUSD_ASSET_FAMILY_V1";
pub const STATE_ROOT: &str = "SATUSD_STATE_ROOT_V1";
pub const REDEEM_INTENT: &str = "SATUSD_REDEEM_INTENT_V1";
pub const LOCK_TWEAK: &str = "SATUSD_LOCK_TWEAK_V1";
/// NUMS internal key for the Bitcoin-layer lock anchor (§5.D3, §18.7; no salt).
pub const LOCK_ANCHOR_NUMS: &str = "SATUSD_LOCK_ANCHOR_NUMS_V1";
pub const LOCK_RECORD: &str = "SATUSD_LOCK_RECORD_V1";
pub const LOCK_FINALIZE: &str = "SATUSD_LOCK_FINALIZE_V1";
pub const LOCK_REFUND: &str = "SATUSD_LOCK_REFUND_V1";
pub const BTC_HTLC_RECORD: &str = "SATUSD_BTC_HTLC_RECORD_V1";
/// NUMS internal key for the operator BTC HTLC anchor (§5.D5, §18.7; salted by
/// `payment_hash`). No key-path spend; both legs go through the explicit leaves.
pub const BTC_HTLC_NUMS: &str = "SATUSD_BTC_HTLC_NUMS_V1";
pub const BTC_PAYOUT_CONFIRMATION: &str = "SATUSD_BTC_PAYOUT_CONFIRMATION_V1";
pub const REDEMPTION_RECORD: &str = "SATUSD_REDEMPTION_RECORD_V1";
pub const REDEMPTION_NULLIFIER: &str = "SATUSD_REDEMPTION_NULLIFIER_V1";
pub const RESERVE_CLAIM: &str = "SATUSD_RESERVE_CLAIM_V1";
pub const CLAIM_ID: &str = "SATUSD_CLAIM_ID_V1";
pub const PENDING_CLAIM: &str = "SATUSD_PENDING_CLAIM_V1";
pub const ISSUER_POSITION: &str = "SATUSD_ISSUER_POSITION_V1";
pub const MINT_REQUEST: &str = "SATUSD_MINT_REQUEST_V1";
pub const ORACLE: &str = "SATUSD_ORACLE_V1";
pub const ORACLE_SET: &str = "SATUSD_ORACLE_SET_V1";
pub const CLAIM_CLOCK: &str = "SATUSD_CLAIM_CLOCK_V1";
pub const LIVE_DA_BUNDLE: &str = "SATUSD_LIVE_DA_BUNDLE_V1";
pub const ARCHIVAL_DA_BUNDLE: &str = "SATUSD_ARCHIVAL_DA_BUNDLE_V1";
pub const OPERATOR_POSITION: &str = "SATUSD_OPERATOR_POSITION_V1";
pub const BURN_SINK: &str = "SATUSD_BURN_SINK_V1";
pub const BURN_TWEAK: &str = "SATUSD_BURN_TWEAK_V1";
pub const LINEAGE_PROOF: &str = "SATUSD_LINEAGE_PROOF_V1";
pub const RESERVE_COMMITTEE: &str = "SATUSD_RESERVE_COMMITTEE_V1";

/// Every registered domain separator, for registry-completeness tests.
pub const ALL: &[&str] = &[
    ASSET_FAMILY,
    STATE_ROOT,
    REDEEM_INTENT,
    LOCK_TWEAK,
    LOCK_ANCHOR_NUMS,
    LOCK_RECORD,
    LOCK_FINALIZE,
    LOCK_REFUND,
    BTC_HTLC_RECORD,
    BTC_HTLC_NUMS,
    BTC_PAYOUT_CONFIRMATION,
    REDEMPTION_RECORD,
    REDEMPTION_NULLIFIER,
    RESERVE_CLAIM,
    CLAIM_ID,
    PENDING_CLAIM,
    ISSUER_POSITION,
    MINT_REQUEST,
    ORACLE,
    ORACLE_SET,
    CLAIM_CLOCK,
    LIVE_DA_BUNDLE,
    ARCHIVAL_DA_BUNDLE,
    OPERATOR_POSITION,
    BURN_SINK,
    BURN_TWEAK,
    LINEAGE_PROOF,
    RESERVE_COMMITTEE,
];
