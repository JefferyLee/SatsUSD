//! JSON request DTOs for the state-node HTTP API. Byte fields are hex strings
//! (`Hex<N>`); each DTO converts into the corresponding domain type. The SMT
//! proof fields of the redeem witnesses are left empty — the node fills them.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use satusd_state::mint::{MintCommitWitness, MintFinalizeWitness, MultisigSig};
use satusd_state::redeem::{LockRefundWitness, RedeemFinalizeWitness, RedeemLockWitness};
use satusd_types::types::{
    BtcHtlcPayoutRecord, IssuerPosition, IssuerStatus, LockFinalizeRecord, LockRecord,
    LockRefundRecord, OperatorPosition, OperatorStatus, OutPoint, RedeemIntent,
};

/// A fixed-size byte array carried as a hex string in JSON.
#[derive(Clone, Copy)]
pub struct Hex<const N: usize>(pub [u8; N]);

impl<const N: usize> Serialize for Hex<N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(self.0))
    }
}

impl<'de, const N: usize> Deserialize<'de> for Hex<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let v = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let a: [u8; N] = v
            .try_into()
            .map_err(|_| serde::de::Error::custom(format!("expected {N} bytes")))?;
        Ok(Hex(a))
    }
}

fn issuer_status(v: u8) -> Result<IssuerStatus, String> {
    Ok(match v {
        0 => IssuerStatus::Active,
        1 => IssuerStatus::Paused,
        2 => IssuerStatus::Frozen,
        3 => IssuerStatus::Exiting,
        _ => return Err(format!("bad issuer status {v}")),
    })
}

#[derive(Deserialize)]
pub struct OutPointDto {
    pub txid: Hex<32>,
    pub vout: u32,
}
impl OutPointDto {
    fn into_domain(self) -> OutPoint {
        OutPoint {
            txid: self.txid.0,
            vout: self.vout,
        }
    }
}

#[derive(Deserialize)]
pub struct IssuerPositionDto {
    pub issuer_id: Hex<32>,
    pub status: u8,
    pub multisig_pubkeys: Vec<Hex<33>>,
    pub multisig_threshold: u8,
    pub reserve_deposits_sats: u64,
    pub minted_satusd_atoms: u64,
    pub pending_mint_atoms: u64,
    pub collateral_ratio_ppm: u64,
    pub last_deposit_txid: Option<Hex<32>>,
    pub registered_at_height: u32,
    pub pending_mint_commitment: Option<Hex<32>>,
}
impl IssuerPositionDto {
    pub fn into_domain(self) -> Result<IssuerPosition, String> {
        Ok(IssuerPosition {
            issuer_id: self.issuer_id.0,
            status: issuer_status(self.status)?,
            multisig_pubkeys: self.multisig_pubkeys.iter().map(|h| h.0).collect(),
            multisig_threshold: self.multisig_threshold,
            reserve_deposits_sats: self.reserve_deposits_sats,
            minted_satusd_atoms: self.minted_satusd_atoms,
            pending_mint_atoms: self.pending_mint_atoms,
            collateral_ratio_ppm: self.collateral_ratio_ppm,
            last_deposit_txid: self.last_deposit_txid.map(|h| h.0),
            freeze_reason: None,
            registered_at_height: self.registered_at_height,
            pending_mint_commitment: self.pending_mint_commitment.map(|h| h.0),
        })
    }
}

fn operator_status(v: u8) -> Result<OperatorStatus, String> {
    Ok(match v {
        0 => OperatorStatus::Active,
        1 => OperatorStatus::Suspended,
        2 => OperatorStatus::Slashed,
        _ => return Err(format!("bad operator status {v}")),
    })
}

#[derive(Deserialize)]
pub struct OperatorPositionDto {
    pub operator_id: Hex<32>,
    pub status: u8,
    pub operator_pubkey: Hex<33>,
    pub bond_sats: u64,
    pub max_claim_sats: u64,
    pub outstanding_claim_sats: u64,
    pub slashed_sats: u64,
    pub registered_at_height: u32,
}
impl OperatorPositionDto {
    pub fn into_domain(self) -> Result<OperatorPosition, String> {
        Ok(OperatorPosition {
            operator_id: self.operator_id.0,
            status: operator_status(self.status)?,
            operator_pubkey: self.operator_pubkey.0,
            bond_sats: self.bond_sats,
            max_claim_sats: self.max_claim_sats,
            outstanding_claim_sats: self.outstanding_claim_sats,
            slashed_sats: self.slashed_sats,
            registered_at_height: self.registered_at_height,
        })
    }
}

#[derive(Deserialize)]
pub struct MultisigSigDto {
    pub signer_pubkey: Hex<33>,
    pub signature: Hex<64>,
}

#[derive(Deserialize)]
pub struct MintCommitDto {
    pub issuer_id: Hex<32>,
    pub requested_mint_atoms: u64,
    pub deposit_txid: Hex<32>,
    pub deposit_sats: u64,
    pub deposit_confirmations: u32,
    pub deposit_to_reserve: bool,
    pub asset_metadata_commitment: Hex<32>,
    pub signatures: Vec<MultisigSigDto>,
    pub oracle_price_e8: u64,
}
impl MintCommitDto {
    pub fn into_witness(self) -> MintCommitWitness {
        MintCommitWitness {
            issuer_id: self.issuer_id.0,
            requested_mint_atoms: self.requested_mint_atoms,
            deposit_txid: self.deposit_txid.0,
            deposit_sats: self.deposit_sats,
            deposit_confirmations: self.deposit_confirmations,
            deposit_to_reserve: self.deposit_to_reserve,
            asset_metadata_commitment: self.asset_metadata_commitment.0,
            signatures: self
                .signatures
                .into_iter()
                .map(|s| MultisigSig {
                    signer_pubkey: s.signer_pubkey.0,
                    signature: s.signature.0,
                })
                .collect(),
            oracle_price_e8: self.oracle_price_e8,
        }
    }
}

#[derive(Deserialize)]
pub struct MintFinalizeDto {
    pub issuer_id: Hex<32>,
    pub requested_mint_atoms: u64,
    pub deposit_txid: Hex<32>,
    pub asset_metadata_commitment: Hex<32>,
    pub mint_anchor_confirmations: u32,
    pub mint_proof_ok: bool,
    pub oracle_price_e8: u64,
}
impl MintFinalizeDto {
    pub fn into_witness(self) -> MintFinalizeWitness {
        MintFinalizeWitness {
            issuer_id: self.issuer_id.0,
            requested_mint_atoms: self.requested_mint_atoms,
            deposit_txid: self.deposit_txid.0,
            asset_metadata_commitment: self.asset_metadata_commitment.0,
            mint_anchor_confirmations: self.mint_anchor_confirmations,
            mint_proof_ok: self.mint_proof_ok,
            oracle_price_e8: self.oracle_price_e8,
        }
    }
}

#[derive(Deserialize)]
pub struct RedeemIntentDto {
    pub version: u16,
    pub network: u8,
    pub redemption_id: Hex<32>,
    pub satusd_asset_family_id: Hex<32>,
    pub amount_satusd_atoms: u64,
    pub user_btc_refund_pubkey: Hex<32>,
    pub user_btc_claim_pubkey: Hex<32>,
    pub user_asset_refund_key: Hex<32>,
    pub operator_id: Option<Hex<32>>,
    pub mode: u8,
    pub payment_hash: Hex<32>,
    pub asset_lock_csv_delta: u32,
    pub btc_htlc_csv_delta: u32,
    pub max_operator_fee_bps: u16,
    pub l1_anchor_height: u32,
    pub l1_anchor_hash: Hex<32>,
    pub expiry_height: u32,
    pub nonce: Hex<32>,
}
impl RedeemIntentDto {
    fn into_domain(self) -> RedeemIntent {
        RedeemIntent {
            version: self.version,
            network: self.network,
            redemption_id: self.redemption_id.0,
            satusd_asset_family_id: self.satusd_asset_family_id.0,
            amount_satusd_atoms: self.amount_satusd_atoms,
            user_btc_refund_pubkey: self.user_btc_refund_pubkey.0,
            user_btc_claim_pubkey: self.user_btc_claim_pubkey.0,
            user_asset_refund_key: self.user_asset_refund_key.0,
            operator_id: self.operator_id.map(|h| h.0),
            mode: self.mode,
            payment_hash: self.payment_hash.0,
            asset_lock_csv_delta: self.asset_lock_csv_delta,
            btc_htlc_csv_delta: self.btc_htlc_csv_delta,
            max_operator_fee_bps: self.max_operator_fee_bps,
            l1_anchor_height: self.l1_anchor_height,
            l1_anchor_hash: self.l1_anchor_hash.0,
            expiry_height: self.expiry_height,
            nonce: self.nonce.0,
        }
    }
}

#[derive(Deserialize)]
pub struct LockRecordDto {
    pub lock_record_version: u16,
    pub redeem_intent_hash: Hex<32>,
    pub lock_anchor_outpoint: OutPointDto,
    pub lock_anchor_txid: Hex<32>,
    pub lock_script_key: Hex<32>,
    pub lock_amount_atoms: u64,
    pub asset_family_id: Hex<32>,
    pub asset_lock_csv_delta: u32,
    pub payment_hash: Hex<32>,
    pub lineage_proof_hash: Hex<32>,
    pub lineage_verified_by: Vec<Hex<32>>,
    pub anchor_inclusion_height: u32,
}
impl LockRecordDto {
    fn into_domain(self) -> LockRecord {
        LockRecord {
            lock_record_version: self.lock_record_version,
            redeem_intent_hash: self.redeem_intent_hash.0,
            lock_anchor_outpoint: self.lock_anchor_outpoint.into_domain(),
            lock_anchor_txid: self.lock_anchor_txid.0,
            lock_script_key: self.lock_script_key.0,
            lock_amount_atoms: self.lock_amount_atoms,
            asset_family_id: self.asset_family_id.0,
            asset_lock_csv_delta: self.asset_lock_csv_delta,
            payment_hash: self.payment_hash.0,
            lineage_proof_hash: self.lineage_proof_hash.0,
            lineage_verified_by: self.lineage_verified_by.iter().map(|h| h.0).collect(),
            anchor_inclusion_height: self.anchor_inclusion_height,
        }
    }
}

#[derive(Deserialize)]
pub struct RedeemLockDto {
    pub redeem_intent: RedeemIntentDto,
    pub lock_record: LockRecordDto,
    pub lineage_ok: bool,
    pub lineage_proof_hash: Hex<32>,
}
impl RedeemLockDto {
    pub fn into_witness(self) -> RedeemLockWitness {
        RedeemLockWitness {
            redeem_intent: self.redeem_intent.into_domain(),
            lock_record: self.lock_record.into_domain(),
            lock_exclusion_proof: vec![],
            lineage_ok: self.lineage_ok,
            lineage_proof_hash: self.lineage_proof_hash.0,
        }
    }
}

#[derive(Deserialize)]
pub struct LockFinalizeDto {
    pub lock_record_hash: Hex<32>,
    pub payment_preimage: Hex<32>,
    pub finalize_anchor_txid: Hex<32>,
    pub finalize_anchor_outpoint: OutPointDto,
    pub protocol_sink_script_key: Hex<32>,
    pub protocol_burn_internal_key: Hex<32>,
    pub finalized_amount_atoms: u64,
    pub operator_id: Hex<32>,
    pub finalize_height: u32,
    pub universe_burn_proof_hash: Hex<32>,
}
impl LockFinalizeDto {
    fn into_domain(self) -> LockFinalizeRecord {
        LockFinalizeRecord {
            lock_record_hash: self.lock_record_hash.0,
            payment_preimage: self.payment_preimage.0,
            finalize_anchor_txid: self.finalize_anchor_txid.0,
            finalize_anchor_outpoint: self.finalize_anchor_outpoint.into_domain(),
            protocol_sink_script_key: self.protocol_sink_script_key.0,
            protocol_burn_internal_key: self.protocol_burn_internal_key.0,
            finalized_amount_atoms: self.finalized_amount_atoms,
            operator_id: self.operator_id.0,
            finalize_height: self.finalize_height,
            universe_burn_proof_hash: self.universe_burn_proof_hash.0,
        }
    }
}

#[derive(Deserialize)]
pub struct BtcHtlcDto {
    pub operator_id: Hex<32>,
    pub redeem_intent_hash: Hex<32>,
    pub btc_htlc_txid: Hex<32>,
    pub btc_htlc_vout: u32,
    pub payment_hash: Hex<32>,
    pub user_claim_pubkey: Hex<32>,
    pub operator_refund_pubkey: Hex<32>,
    pub payout_sats: u64,
    pub btc_csv_delta: u32,
    pub htlc_inclusion_height: u32,
    pub htlc_inclusion_block_hash: Hex<32>,
    pub claim_spend_txid: Hex<32>,
    pub revealed_preimage: Hex<32>,
    pub claim_inclusion_height: u32,
    pub claim_inclusion_block_hash: Hex<32>,
    pub confirmation_depth: u32,
}
impl BtcHtlcDto {
    fn into_domain(self) -> BtcHtlcPayoutRecord {
        BtcHtlcPayoutRecord {
            operator_id: self.operator_id.0,
            redeem_intent_hash: self.redeem_intent_hash.0,
            btc_htlc_txid: self.btc_htlc_txid.0,
            btc_htlc_vout: self.btc_htlc_vout,
            payment_hash: self.payment_hash.0,
            user_claim_pubkey: self.user_claim_pubkey.0,
            operator_refund_pubkey: self.operator_refund_pubkey.0,
            payout_sats: self.payout_sats,
            btc_csv_delta: self.btc_csv_delta,
            htlc_inclusion_height: self.htlc_inclusion_height,
            htlc_inclusion_block_hash: self.htlc_inclusion_block_hash.0,
            claim_spend_txid: self.claim_spend_txid.0,
            revealed_preimage: self.revealed_preimage.0,
            claim_inclusion_height: self.claim_inclusion_height,
            claim_inclusion_block_hash: self.claim_inclusion_block_hash.0,
            confirmation_depth: self.confirmation_depth,
        }
    }
}

#[derive(Deserialize)]
pub struct RedeemFinalizeDto {
    pub redeem_intent: RedeemIntentDto,
    pub lock_record: LockRecordDto,
    pub lock_finalize: LockFinalizeDto,
    pub btc_htlc: BtcHtlcDto,
    pub operator_id: Hex<32>,
    pub price_e8: u64,
    pub burn_proof_ok: bool,
}
impl RedeemFinalizeDto {
    pub fn into_witness(self) -> RedeemFinalizeWitness {
        RedeemFinalizeWitness {
            redeem_intent: self.redeem_intent.into_domain(),
            lock_record: self.lock_record.into_domain(),
            lock_finalize: self.lock_finalize.into_domain(),
            btc_htlc: self.btc_htlc.into_domain(),
            operator_id: self.operator_id.0,
            price_e8: self.price_e8,
            lock_membership_proof: vec![],
            consumed_exclusion_proof: vec![],
            refund_exclusion_proof: vec![],
            nullifier_exclusion_proof: vec![],
            burn_proof_ok: self.burn_proof_ok,
        }
    }
}

#[derive(Deserialize)]
pub struct LockRefundRecordDto {
    pub lock_record_hash: Hex<32>,
    pub refund_anchor_txid: Hex<32>,
    pub refund_anchor_outpoint: OutPointDto,
    pub user_signature: Hex<64>,
    pub refund_height: u32,
    pub asset_returned_to: Hex<32>,
}
impl LockRefundRecordDto {
    fn into_domain(self) -> LockRefundRecord {
        LockRefundRecord {
            lock_record_hash: self.lock_record_hash.0,
            refund_anchor_txid: self.refund_anchor_txid.0,
            refund_anchor_outpoint: self.refund_anchor_outpoint.into_domain(),
            user_signature: self.user_signature.0,
            refund_height: self.refund_height,
            asset_returned_to: self.asset_returned_to.0,
        }
    }
}

#[derive(Deserialize)]
pub struct LockRefundDto {
    pub redeem_intent: RedeemIntentDto,
    pub lock_record: LockRecordDto,
    pub lock_refund: LockRefundRecordDto,
}
impl LockRefundDto {
    pub fn into_witness(self) -> LockRefundWitness {
        LockRefundWitness {
            redeem_intent: self.redeem_intent.into_domain(),
            lock_record: self.lock_record.into_domain(),
            lock_refund: self.lock_refund.into_domain(),
            lock_membership_proof: vec![],
            consumed_exclusion_proof: vec![],
            refund_exclusion_proof: vec![],
        }
    }
}

/// A submitted/simulated transition, tagged by `"transition"`.
#[derive(Deserialize)]
#[serde(tag = "transition", rename_all = "snake_case")]
pub enum TransitionRequest {
    IssuerRegister(Box<IssuerPositionDto>),
    OperatorRegister(Box<OperatorPositionDto>),
    MintCommit(Box<MintCommitDto>),
    MintFinalize(MintFinalizeDto),
    RedeemLock(Box<RedeemLockDto>),
    RedeemFinalize(Box<RedeemFinalizeDto>),
    LockRefund(Box<LockRefundDto>),
}
