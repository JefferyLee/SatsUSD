//! Core protocol data structures with canonical encoding (PRD §6, §5.D2/D6/D7/D10).
//!
//! Field order in each `Encode` impl matches the PRD struct declaration exactly;
//! changing order or width is a spec-breaking change (bump `_V2`, §18.2).
//!
//! Deferred (enum discriminants not frozen in PRD): IssuerPosition / PendingClaim.

use crate::encoding::{Encode, Encoder};

/// Bitcoin outpoint (§18.1): 32B txid in display order || 4B vout (BE).
/// `txid` is stored already in display (RPC) byte order and written verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutPoint {
    pub txid: [u8; 32],
    pub vout: u32,
}

impl Encode for OutPoint {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.txid);
        e.u32(self.vout);
    }
}

/// Transition registry (§5.D10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TransitionType {
    MintCommit = 0x01,
    MintFinalize = 0x02,
    RedeemFastLock = 0x10,
    RedeemFastFinalize = 0x11,
    LockRefund = 0x12,
    RedeemDirect = 0x13,
    OperatorRegister = 0x20,
    IssuerRegister = 0x21,
    ReclaimStaleClaim = 0x30,
    Liquidate = 0x40,
    RotateShard = 0x50,
    Govern = 0x60,
    Settle = 0x70,
}

impl TransitionType {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// RedeemIntent (§5.D2, encoding §18.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedeemIntent {
    pub version: u16,
    pub network: u8,
    pub redemption_id: [u8; 32],
    pub satusd_asset_family_id: [u8; 32],
    pub amount_satusd_atoms: u64,
    pub user_btc_refund_pubkey: [u8; 32],
    pub user_btc_claim_pubkey: [u8; 32],
    pub user_asset_refund_key: [u8; 32],
    pub operator_id: Option<[u8; 32]>,
    pub mode: u8,
    pub payment_hash: [u8; 32],
    pub asset_lock_csv_delta: u32,
    pub btc_htlc_csv_delta: u32,
    pub max_operator_fee_bps: u16,
    pub l1_anchor_height: u32,
    pub l1_anchor_hash: [u8; 32],
    pub expiry_height: u32,
    pub nonce: [u8; 32],
}

impl Encode for RedeemIntent {
    fn encode(&self, e: &mut Encoder) {
        e.u16(self.version);
        e.u8(self.network);
        e.fixed(&self.redemption_id);
        e.fixed(&self.satusd_asset_family_id);
        e.u64(self.amount_satusd_atoms);
        e.fixed(&self.user_btc_refund_pubkey);
        e.fixed(&self.user_btc_claim_pubkey);
        e.fixed(&self.user_asset_refund_key);
        e.opt(&self.operator_id, |e, id| e.fixed(id));
        e.u8(self.mode);
        e.fixed(&self.payment_hash);
        e.u32(self.asset_lock_csv_delta);
        e.u32(self.btc_htlc_csv_delta);
        e.u16(self.max_operator_fee_bps);
        e.u32(self.l1_anchor_height);
        e.fixed(&self.l1_anchor_hash);
        e.u32(self.expiry_height);
        e.fixed(&self.nonce);
    }
}

/// ClaimClock (§5.D6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimClock {
    pub l1_anchor_height: u32,
    pub l1_anchor_hash: [u8; 32],
    pub l1_anchor_mtp: u64,
    pub l1_anchor_chain_time: u64,
    pub recent_header_chain: [[u8; 80]; 12],
    pub oracle_epoch: u64,
    pub selected_oracle_price_e8: u64,
    pub max_epoch_lag_sec: u32,
    pub oracle_future_tolerance: u32,
}

impl Encode for ClaimClock {
    fn encode(&self, e: &mut Encoder) {
        e.u32(self.l1_anchor_height);
        e.fixed(&self.l1_anchor_hash);
        e.u64(self.l1_anchor_mtp);
        e.u64(self.l1_anchor_chain_time);
        // Fixed-size [[u8;80];12]: written as 12 contiguous 80-byte headers, no count prefix.
        for header in &self.recent_header_chain {
            e.fixed(header);
        }
        e.u64(self.oracle_epoch);
        e.u64(self.selected_oracle_price_e8);
        e.u32(self.max_epoch_lag_sec);
        e.u32(self.oracle_future_tolerance);
    }
}

/// OracleMessage (§5.D7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleMessage {
    pub domain: [u8; 32],
    pub oracle_id: [u8; 32],
    pub oracle_set_epoch: u64,
    pub price_epoch: u64,
    pub timestamp_ms: u64,
    pub pair: [u8; 8],
    pub price_e8: u64,
    pub source_commitment: [u8; 32],
    pub signer_pubkey: [u8; 32],
    pub signature: [u8; 64],
}

impl Encode for OracleMessage {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.domain);
        e.fixed(&self.oracle_id);
        e.u64(self.oracle_set_epoch);
        e.u64(self.price_epoch);
        e.u64(self.timestamp_ms);
        e.fixed(&self.pair);
        e.u64(self.price_e8);
        e.fixed(&self.source_commitment);
        e.fixed(&self.signer_pubkey);
        e.fixed(&self.signature);
    }
}

/// LockRecord (§6.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockRecord {
    pub lock_record_version: u16,
    pub redeem_intent_hash: [u8; 32],
    pub lock_anchor_outpoint: OutPoint,
    pub lock_anchor_txid: [u8; 32],
    pub lock_script_key: [u8; 32],
    pub lock_amount_atoms: u64,
    pub asset_family_id: [u8; 32],
    pub asset_lock_csv_delta: u32,
    pub payment_hash: [u8; 32],
    pub lineage_proof_hash: [u8; 32],
    pub lineage_verified_by: Vec<[u8; 32]>,
    pub anchor_inclusion_height: u32,
}

impl Encode for LockRecord {
    fn encode(&self, e: &mut Encoder) {
        e.u16(self.lock_record_version);
        e.fixed(&self.redeem_intent_hash);
        self.lock_anchor_outpoint.encode(e);
        e.fixed(&self.lock_anchor_txid);
        e.fixed(&self.lock_script_key);
        e.u64(self.lock_amount_atoms);
        e.fixed(&self.asset_family_id);
        e.u32(self.asset_lock_csv_delta);
        e.fixed(&self.payment_hash);
        e.fixed(&self.lineage_proof_hash);
        e.seq(&self.lineage_verified_by, |e, id| e.fixed(id));
        e.u32(self.anchor_inclusion_height);
    }
}

/// LockFinalizeRecord (§6.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockFinalizeRecord {
    pub lock_record_hash: [u8; 32],
    pub payment_preimage: [u8; 32],
    pub finalize_anchor_txid: [u8; 32],
    pub finalize_anchor_outpoint: OutPoint,
    pub protocol_sink_script_key: [u8; 32],
    pub protocol_burn_internal_key: [u8; 32],
    pub finalized_amount_atoms: u64,
    pub operator_id: [u8; 32],
    pub finalize_height: u32,
    pub universe_burn_proof_hash: [u8; 32],
}

impl Encode for LockFinalizeRecord {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.lock_record_hash);
        e.fixed(&self.payment_preimage);
        e.fixed(&self.finalize_anchor_txid);
        self.finalize_anchor_outpoint.encode(e);
        e.fixed(&self.protocol_sink_script_key);
        e.fixed(&self.protocol_burn_internal_key);
        e.u64(self.finalized_amount_atoms);
        e.fixed(&self.operator_id);
        e.u32(self.finalize_height);
        e.fixed(&self.universe_burn_proof_hash);
    }
}

/// LockRefundRecord (§6.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockRefundRecord {
    pub lock_record_hash: [u8; 32],
    pub refund_anchor_txid: [u8; 32],
    pub refund_anchor_outpoint: OutPoint,
    pub user_signature: [u8; 64],
    pub refund_height: u32,
    pub asset_returned_to: [u8; 32],
}

impl Encode for LockRefundRecord {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.lock_record_hash);
        e.fixed(&self.refund_anchor_txid);
        self.refund_anchor_outpoint.encode(e);
        e.fixed(&self.user_signature);
        e.u32(self.refund_height);
        e.fixed(&self.asset_returned_to);
    }
}

/// BtcHtlcPayoutRecord (§6.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtcHtlcPayoutRecord {
    pub operator_id: [u8; 32],
    pub redeem_intent_hash: [u8; 32],
    pub btc_htlc_txid: [u8; 32],
    pub btc_htlc_vout: u32,
    pub payment_hash: [u8; 32],
    pub user_claim_pubkey: [u8; 32],
    pub operator_refund_pubkey: [u8; 32],
    pub payout_sats: u64,
    pub btc_csv_delta: u32,
    pub htlc_inclusion_height: u32,
    pub htlc_inclusion_block_hash: [u8; 32],
    pub claim_spend_txid: [u8; 32],
    pub revealed_preimage: [u8; 32],
    pub claim_inclusion_height: u32,
    pub claim_inclusion_block_hash: [u8; 32],
    pub confirmation_depth: u32,
}

impl Encode for BtcHtlcPayoutRecord {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.operator_id);
        e.fixed(&self.redeem_intent_hash);
        e.fixed(&self.btc_htlc_txid);
        e.u32(self.btc_htlc_vout);
        e.fixed(&self.payment_hash);
        e.fixed(&self.user_claim_pubkey);
        e.fixed(&self.operator_refund_pubkey);
        e.u64(self.payout_sats);
        e.u32(self.btc_csv_delta);
        e.u32(self.htlc_inclusion_height);
        e.fixed(&self.htlc_inclusion_block_hash);
        e.fixed(&self.claim_spend_txid);
        e.fixed(&self.revealed_preimage);
        e.u32(self.claim_inclusion_height);
        e.fixed(&self.claim_inclusion_block_hash);
        e.u32(self.confirmation_depth);
    }
}

/// BtcPayoutConfirmation (§5.D14, §6.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtcPayoutConfirmation {
    pub btc_htlc_txid: [u8; 32],
    pub btc_htlc_vout: u32,
    pub htlc_output_value_sats: u64,
    pub htlc_output_script: Vec<u8>,
    pub htlc_inclusion_block_hash: [u8; 32],
    pub htlc_inclusion_block_height: u32,
    pub htlc_inclusion_merkle_proof: Vec<[u8; 32]>,
    pub claim_spend_txid: [u8; 32],
    pub claim_spend_input_index: u32,
    pub claim_spend_witness: Vec<Vec<u8>>,
    pub revealed_preimage: [u8; 32],
    pub claim_inclusion_block_hash: [u8; 32],
    pub claim_inclusion_block_height: u32,
    pub claim_inclusion_merkle_proof: Vec<[u8; 32]>,
    pub confirmation_headers: Vec<[u8; 80]>,
}

impl Encode for BtcPayoutConfirmation {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.btc_htlc_txid);
        e.u32(self.btc_htlc_vout);
        e.u64(self.htlc_output_value_sats);
        e.var_bytes(&self.htlc_output_script);
        e.fixed(&self.htlc_inclusion_block_hash);
        e.u32(self.htlc_inclusion_block_height);
        e.seq(&self.htlc_inclusion_merkle_proof, |e, h| e.fixed(h));
        e.fixed(&self.claim_spend_txid);
        e.u32(self.claim_spend_input_index);
        e.seq(&self.claim_spend_witness, |e, w| e.var_bytes(w));
        e.fixed(&self.revealed_preimage);
        e.fixed(&self.claim_inclusion_block_hash);
        e.u32(self.claim_inclusion_block_height);
        e.seq(&self.claim_inclusion_merkle_proof, |e, h| e.fixed(h));
        e.seq(&self.confirmation_headers, |e, h| e.fixed(h));
    }
}

/// RedemptionRecord (§6.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedemptionRecord {
    pub redeem_intent_hash: [u8; 32],
    pub lock_record_hash: [u8; 32],
    pub btc_htlc_record_hash: [u8; 32],
    pub btc_payout_confirmation_hash: [u8; 32],
    pub lock_finalize_hash: [u8; 32],
    pub selected_oracle_epoch: u64,
    pub selected_price_e8: u64,
    pub gross_btc_sats: u64,
    pub operator_fee_sats: u64,
    pub user_payout_sats: u64,
}

impl Encode for RedemptionRecord {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.redeem_intent_hash);
        e.fixed(&self.lock_record_hash);
        e.fixed(&self.btc_htlc_record_hash);
        e.fixed(&self.btc_payout_confirmation_hash);
        e.fixed(&self.lock_finalize_hash);
        e.u64(self.selected_oracle_epoch);
        e.u64(self.selected_price_e8);
        e.u64(self.gross_btc_sats);
        e.u64(self.operator_fee_sats);
        e.u64(self.user_payout_sats);
    }
}

/// ReserveClaim (§6.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveClaim {
    pub claim_id: [u8; 32],
    pub transition_type: u8,
    pub operator_id: [u8; 32],
    pub prev_state_root: [u8; 32],
    pub new_state_root: [u8; 32],
    pub redemption_batch_root: [u8; 32],
    pub oracle_batch_root: [u8; 32],
    pub lock_batch_root: [u8; 32],
    pub payout_batch_root: [u8; 32],
    pub confirmation_batch_root: [u8; 32],
    pub finalize_batch_root: [u8; 32],
    pub burn_proof_batch_root: [u8; 32],
    pub lineage_proof_batch_root: [u8; 32],
    pub live_da_root: [u8; 32],
    pub archival_da_root: [u8; 32],
    pub l1_anchor: ClaimClock,
    pub reserve_shard_id: u64,
    pub reimbursement_sats: u64,
    pub proof_commitment: [u8; 32],
    pub claim_expiry_height: u32,
    pub operator_signature: [u8; 64],
}

impl ReserveClaim {
    /// Encode every field except `claim_id` and `operator_signature`.
    ///
    /// SPEC DECISION (flag for ADR): §5.D13 defines `claim_id` over the claim
    /// "without signatures". `claim_id` is itself a field, so its own preimage
    /// cannot include it; we exclude both `claim_id` and `operator_signature`.
    pub fn encode_for_claim_id(&self, e: &mut Encoder) {
        e.u8(self.transition_type);
        e.fixed(&self.operator_id);
        e.fixed(&self.prev_state_root);
        e.fixed(&self.new_state_root);
        e.fixed(&self.redemption_batch_root);
        e.fixed(&self.oracle_batch_root);
        e.fixed(&self.lock_batch_root);
        e.fixed(&self.payout_batch_root);
        e.fixed(&self.confirmation_batch_root);
        e.fixed(&self.finalize_batch_root);
        e.fixed(&self.burn_proof_batch_root);
        e.fixed(&self.lineage_proof_batch_root);
        e.fixed(&self.live_da_root);
        e.fixed(&self.archival_da_root);
        self.l1_anchor.encode(e);
        e.u64(self.reserve_shard_id);
        e.u64(self.reimbursement_sats);
        e.fixed(&self.proof_commitment);
        e.u32(self.claim_expiry_height);
    }
}

impl Encode for ReserveClaim {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.claim_id);
        self.encode_for_claim_id(e);
        e.fixed(&self.operator_signature);
    }
}

/// Issuer status (§5.D11). Discriminants frozen 0-based in declaration order (ADR-0016).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IssuerStatus {
    Active = 0,
    Paused = 1,
    Frozen = 2,
    Exiting = 3,
}

impl IssuerStatus {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Reason an issuer was paused/frozen — the five §5.D11 triggers (ADR-0016).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FreezeReason {
    OracleUnavailable = 0,
    IndividualCrBelowTier1 = 1,
    ReserveCommitteeVote = 2,
    MintProofMismatch = 3,
    ManualEmergencyPause = 4,
}

impl FreezeReason {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Pending reserve claim status (§5.D12). Discriminants frozen 0-based (ADR-0016).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PendingClaimStatus {
    Pending = 0,
    Finalized = 1,
    Challenged = 2,
    Expired = 3,
    Reclaimed = 4,
}

impl PendingClaimStatus {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// IssuerPosition (§5.D11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuerPosition {
    pub issuer_id: [u8; 32],
    pub status: IssuerStatus,
    pub multisig_pubkeys: Vec<[u8; 33]>,
    pub multisig_threshold: u8,
    pub reserve_deposits_sats: u64,
    pub minted_satusd_atoms: u64,
    pub pending_mint_atoms: u64,
    pub collateral_ratio_ppm: u64,
    pub last_deposit_txid: Option<[u8; 32]>,
    pub freeze_reason: Option<FreezeReason>,
    pub registered_at_height: u32,
}

impl Encode for IssuerPosition {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.issuer_id);
        e.enum_u8(self.status.as_u8());
        e.seq(&self.multisig_pubkeys, |e, pk| e.fixed(pk));
        e.u8(self.multisig_threshold);
        e.u64(self.reserve_deposits_sats);
        e.u64(self.minted_satusd_atoms);
        e.u64(self.pending_mint_atoms);
        e.u64(self.collateral_ratio_ppm);
        e.opt(&self.last_deposit_txid, |e, txid| e.fixed(txid));
        e.opt(&self.freeze_reason, |e, r| e.enum_u8(r.as_u8()));
        e.u32(self.registered_at_height);
    }
}

/// PendingClaim (§5.D12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingClaim {
    pub claim_id: [u8; 32],
    pub operator_id: [u8; 32],
    pub reserved_sats: u64,
    pub claim_created_height: u32,
    pub claim_expiry_height: u32,
    pub status: PendingClaimStatus,
}

impl Encode for PendingClaim {
    fn encode(&self, e: &mut Encoder) {
        e.fixed(&self.claim_id);
        e.fixed(&self.operator_id);
        e.u64(self.reserved_sats);
        e.u32(self.claim_created_height);
        e.u32(self.claim_expiry_height);
        e.enum_u8(self.status.as_u8());
    }
}

/// StateRoot (§6.1). `state_root_hash` is Poseidon(encode) — Poseidon deferred.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateRoot {
    pub protocol_version: u16,
    pub state_epoch: u64,
    pub prev_state_root: [u8; 32],
    pub transition_type: u8,
    pub satusd_asset_family_id: [u8; 32],
    pub sat_usd_supply_atoms: u64,
    pub reserve_btc_sats: u64,
    pub reserved_pending_claim_sats: u64,
    pub collateral_ratio_ppm: u64,
    pub emergency_tier: u8,
    pub oracle_set_hash: [u8; 32],
    pub oracle_set_epoch: u64,
    pub latest_oracle_epoch_seen: u64,
    pub latest_oracle_price_e8: u64,
    pub issuer_positions_root: [u8; 32],
    pub operator_registry_root: [u8; 32],
    pub lock_record_root: [u8; 32],
    pub lock_consumed_root: [u8; 32],
    pub lock_refund_root: [u8; 32],
    pub redemption_nullifier_root: [u8; 32],
    pub pending_claim_root: [u8; 32],
    pub live_da_root: [u8; 32],
    pub archival_da_root: [u8; 32],
    pub l1_anchor_hash: [u8; 32],
    pub l1_anchor_height: u32,
    pub l1_anchor_mtp: u64,
    pub l1_anchor_chain_time: u64,
}

impl Encode for StateRoot {
    fn encode(&self, e: &mut Encoder) {
        e.u16(self.protocol_version);
        e.u64(self.state_epoch);
        e.fixed(&self.prev_state_root);
        e.u8(self.transition_type);
        e.fixed(&self.satusd_asset_family_id);
        e.u64(self.sat_usd_supply_atoms);
        e.u64(self.reserve_btc_sats);
        e.u64(self.reserved_pending_claim_sats);
        e.u64(self.collateral_ratio_ppm);
        e.u8(self.emergency_tier);
        e.fixed(&self.oracle_set_hash);
        e.u64(self.oracle_set_epoch);
        e.u64(self.latest_oracle_epoch_seen);
        e.u64(self.latest_oracle_price_e8);
        e.fixed(&self.issuer_positions_root);
        e.fixed(&self.operator_registry_root);
        e.fixed(&self.lock_record_root);
        e.fixed(&self.lock_consumed_root);
        e.fixed(&self.lock_refund_root);
        e.fixed(&self.redemption_nullifier_root);
        e.fixed(&self.pending_claim_root);
        e.fixed(&self.live_da_root);
        e.fixed(&self.archival_da_root);
        e.fixed(&self.l1_anchor_hash);
        e.u32(self.l1_anchor_height);
        e.u64(self.l1_anchor_mtp);
        e.u64(self.l1_anchor_chain_time);
    }
}
