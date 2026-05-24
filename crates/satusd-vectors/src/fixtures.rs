//! Typed fixture builders and their JSON field projection.
//!
//! For each type: `build_*` fills a struct from the deterministic source, and
//! `*_value` projects it to a JSON `fields` object whose keys match the struct
//! field names so the TypeScript reference can rebuild and re-encode it.
//!
//! JSON conventions: byte arrays -> lowercase hex string; u8/u16/u32 -> number;
//! u64 -> decimal string; Option -> null or value; OutPoint/ClaimClock -> object.

use satusd_types::types::*;
use serde_json::{json, Value};

use crate::det::Det;

fn hx(b: &[u8]) -> Value {
    Value::String(hex::encode(b))
}

fn u64s(v: u64) -> Value {
    Value::String(v.to_string())
}

fn hx_list(items: &[[u8; 32]]) -> Value {
    Value::Array(items.iter().map(|h| hx(h)).collect())
}

fn outpoint_value(o: &OutPoint) -> Value {
    json!({ "txid": hx(&o.txid), "vout": o.vout })
}

// ---------------------------------------------------------------- RedeemIntent

pub fn build_redeem_intent(d: &mut Det, with_operator: bool) -> RedeemIntent {
    RedeemIntent {
        version: 1,
        network: d.u8() % 3,
        redemption_id: d.arr(),
        satusd_asset_family_id: d.arr(),
        amount_satusd_atoms: 100 + (d.u64() % 999_999_901),
        user_btc_refund_pubkey: d.arr(),
        user_btc_claim_pubkey: d.arr(),
        user_asset_refund_key: d.arr(),
        operator_id: if with_operator { Some(d.arr()) } else { None },
        mode: if with_operator { 0 } else { 1 },
        payment_hash: d.arr(),
        asset_lock_csv_delta: d.u32(),
        btc_htlc_csv_delta: d.u32(),
        max_operator_fee_bps: d.u16(),
        l1_anchor_height: d.u32(),
        l1_anchor_hash: d.arr(),
        expiry_height: d.u32(),
        nonce: d.arr(),
    }
}

pub fn redeem_intent_value(v: &RedeemIntent) -> Value {
    json!({
        "version": v.version,
        "network": v.network,
        "redemption_id": hx(&v.redemption_id),
        "satusd_asset_family_id": hx(&v.satusd_asset_family_id),
        "amount_satusd_atoms": u64s(v.amount_satusd_atoms),
        "user_btc_refund_pubkey": hx(&v.user_btc_refund_pubkey),
        "user_btc_claim_pubkey": hx(&v.user_btc_claim_pubkey),
        "user_asset_refund_key": hx(&v.user_asset_refund_key),
        "operator_id": v.operator_id.map_or(Value::Null, |id| hx(&id)),
        "mode": v.mode,
        "payment_hash": hx(&v.payment_hash),
        "asset_lock_csv_delta": v.asset_lock_csv_delta,
        "btc_htlc_csv_delta": v.btc_htlc_csv_delta,
        "max_operator_fee_bps": v.max_operator_fee_bps,
        "l1_anchor_height": v.l1_anchor_height,
        "l1_anchor_hash": hx(&v.l1_anchor_hash),
        "expiry_height": v.expiry_height,
        "nonce": hx(&v.nonce),
    })
}

// ------------------------------------------------------------------ ClaimClock

pub fn build_claim_clock(d: &mut Det) -> ClaimClock {
    let mut headers = [[0u8; 80]; 12];
    for h in headers.iter_mut() {
        *h = d.arr();
    }
    ClaimClock {
        l1_anchor_height: d.u32(),
        l1_anchor_hash: d.arr(),
        l1_anchor_mtp: d.u64(),
        l1_anchor_chain_time: d.u64(),
        recent_header_chain: headers,
        oracle_epoch: d.u64(),
        selected_oracle_price_e8: d.u64(),
        max_epoch_lag_sec: d.u32(),
        oracle_future_tolerance: d.u32(),
    }
}

pub fn claim_clock_value(v: &ClaimClock) -> Value {
    json!({
        "l1_anchor_height": v.l1_anchor_height,
        "l1_anchor_hash": hx(&v.l1_anchor_hash),
        "l1_anchor_mtp": u64s(v.l1_anchor_mtp),
        "l1_anchor_chain_time": u64s(v.l1_anchor_chain_time),
        "recent_header_chain": Value::Array(v.recent_header_chain.iter().map(|h| hx(h)).collect()),
        "oracle_epoch": u64s(v.oracle_epoch),
        "selected_oracle_price_e8": u64s(v.selected_oracle_price_e8),
        "max_epoch_lag_sec": v.max_epoch_lag_sec,
        "oracle_future_tolerance": v.oracle_future_tolerance,
    })
}

// ---------------------------------------------------------------- OracleMessage

pub fn build_oracle_message(d: &mut Det) -> OracleMessage {
    OracleMessage {
        domain: d.arr(),
        oracle_id: d.arr(),
        oracle_set_epoch: d.u64(),
        price_epoch: d.u64(),
        timestamp_ms: d.u64(),
        pair: d.arr(),
        price_e8: d.u64(),
        source_commitment: d.arr(),
        signer_pubkey: d.arr(),
        signature: d.arr(),
    }
}

pub fn oracle_message_value(v: &OracleMessage) -> Value {
    json!({
        "domain": hx(&v.domain),
        "oracle_id": hx(&v.oracle_id),
        "oracle_set_epoch": u64s(v.oracle_set_epoch),
        "price_epoch": u64s(v.price_epoch),
        "timestamp_ms": u64s(v.timestamp_ms),
        "pair": hx(&v.pair),
        "price_e8": u64s(v.price_e8),
        "source_commitment": hx(&v.source_commitment),
        "signer_pubkey": hx(&v.signer_pubkey),
        "signature": hx(&v.signature),
    })
}

// ------------------------------------------------------------------- LockRecord

pub fn build_lock_record(d: &mut Det) -> LockRecord {
    let n = d.count(3);
    LockRecord {
        lock_record_version: 1,
        redeem_intent_hash: d.arr(),
        lock_anchor_outpoint: OutPoint {
            txid: d.arr(),
            vout: d.u32(),
        },
        lock_anchor_txid: d.arr(),
        lock_script_key: d.arr(),
        lock_amount_atoms: d.u64(),
        asset_family_id: d.arr(),
        asset_lock_csv_delta: d.u32(),
        payment_hash: d.arr(),
        lineage_proof_hash: d.arr(),
        lineage_verified_by: (0..n).map(|_| d.arr()).collect(),
        anchor_inclusion_height: d.u32(),
    }
}

pub fn lock_record_value(v: &LockRecord) -> Value {
    json!({
        "lock_record_version": v.lock_record_version,
        "redeem_intent_hash": hx(&v.redeem_intent_hash),
        "lock_anchor_outpoint": outpoint_value(&v.lock_anchor_outpoint),
        "lock_anchor_txid": hx(&v.lock_anchor_txid),
        "lock_script_key": hx(&v.lock_script_key),
        "lock_amount_atoms": u64s(v.lock_amount_atoms),
        "asset_family_id": hx(&v.asset_family_id),
        "asset_lock_csv_delta": v.asset_lock_csv_delta,
        "payment_hash": hx(&v.payment_hash),
        "lineage_proof_hash": hx(&v.lineage_proof_hash),
        "lineage_verified_by": hx_list(&v.lineage_verified_by),
        "anchor_inclusion_height": v.anchor_inclusion_height,
    })
}

// ----------------------------------------------------------- LockFinalizeRecord

pub fn build_lock_finalize(d: &mut Det) -> LockFinalizeRecord {
    LockFinalizeRecord {
        lock_record_hash: d.arr(),
        payment_preimage: d.arr(),
        finalize_anchor_txid: d.arr(),
        finalize_anchor_outpoint: OutPoint {
            txid: d.arr(),
            vout: d.u32(),
        },
        protocol_sink_script_key: d.arr(),
        protocol_burn_internal_key: d.arr(),
        finalized_amount_atoms: d.u64(),
        operator_id: d.arr(),
        finalize_height: d.u32(),
        universe_burn_proof_hash: d.arr(),
    }
}

pub fn lock_finalize_value(v: &LockFinalizeRecord) -> Value {
    json!({
        "lock_record_hash": hx(&v.lock_record_hash),
        "payment_preimage": hx(&v.payment_preimage),
        "finalize_anchor_txid": hx(&v.finalize_anchor_txid),
        "finalize_anchor_outpoint": outpoint_value(&v.finalize_anchor_outpoint),
        "protocol_sink_script_key": hx(&v.protocol_sink_script_key),
        "protocol_burn_internal_key": hx(&v.protocol_burn_internal_key),
        "finalized_amount_atoms": u64s(v.finalized_amount_atoms),
        "operator_id": hx(&v.operator_id),
        "finalize_height": v.finalize_height,
        "universe_burn_proof_hash": hx(&v.universe_burn_proof_hash),
    })
}

// ------------------------------------------------------------- LockRefundRecord

pub fn build_lock_refund(d: &mut Det) -> LockRefundRecord {
    LockRefundRecord {
        lock_record_hash: d.arr(),
        refund_anchor_txid: d.arr(),
        refund_anchor_outpoint: OutPoint {
            txid: d.arr(),
            vout: d.u32(),
        },
        user_signature: d.arr(),
        refund_height: d.u32(),
        asset_returned_to: d.arr(),
    }
}

pub fn lock_refund_value(v: &LockRefundRecord) -> Value {
    json!({
        "lock_record_hash": hx(&v.lock_record_hash),
        "refund_anchor_txid": hx(&v.refund_anchor_txid),
        "refund_anchor_outpoint": outpoint_value(&v.refund_anchor_outpoint),
        "user_signature": hx(&v.user_signature),
        "refund_height": v.refund_height,
        "asset_returned_to": hx(&v.asset_returned_to),
    })
}

// --------------------------------------------------------- BtcHtlcPayoutRecord

pub fn build_btc_htlc_payout(d: &mut Det) -> BtcHtlcPayoutRecord {
    BtcHtlcPayoutRecord {
        operator_id: d.arr(),
        redeem_intent_hash: d.arr(),
        btc_htlc_txid: d.arr(),
        btc_htlc_vout: d.u32(),
        payment_hash: d.arr(),
        user_claim_pubkey: d.arr(),
        operator_refund_pubkey: d.arr(),
        payout_sats: d.u64(),
        btc_csv_delta: d.u32(),
        htlc_inclusion_height: d.u32(),
        htlc_inclusion_block_hash: d.arr(),
        claim_spend_txid: d.arr(),
        revealed_preimage: d.arr(),
        claim_inclusion_height: d.u32(),
        claim_inclusion_block_hash: d.arr(),
        confirmation_depth: 6 + (d.u32() % 10),
    }
}

pub fn btc_htlc_payout_value(v: &BtcHtlcPayoutRecord) -> Value {
    json!({
        "operator_id": hx(&v.operator_id),
        "redeem_intent_hash": hx(&v.redeem_intent_hash),
        "btc_htlc_txid": hx(&v.btc_htlc_txid),
        "btc_htlc_vout": v.btc_htlc_vout,
        "payment_hash": hx(&v.payment_hash),
        "user_claim_pubkey": hx(&v.user_claim_pubkey),
        "operator_refund_pubkey": hx(&v.operator_refund_pubkey),
        "payout_sats": u64s(v.payout_sats),
        "btc_csv_delta": v.btc_csv_delta,
        "htlc_inclusion_height": v.htlc_inclusion_height,
        "htlc_inclusion_block_hash": hx(&v.htlc_inclusion_block_hash),
        "claim_spend_txid": hx(&v.claim_spend_txid),
        "revealed_preimage": hx(&v.revealed_preimage),
        "claim_inclusion_height": v.claim_inclusion_height,
        "claim_inclusion_block_hash": hx(&v.claim_inclusion_block_hash),
        "confirmation_depth": v.confirmation_depth,
    })
}

// -------------------------------------------------------- BtcPayoutConfirmation

pub fn build_btc_payout_confirmation(d: &mut Det) -> BtcPayoutConfirmation {
    let script_len = 1 + d.count(40);
    let htlc_merkle_n = d.count(4);
    let witness_n = d.count(4);
    let claim_merkle_n = d.count(4);
    let headers_n = 6 + d.count(4);
    BtcPayoutConfirmation {
        btc_htlc_txid: d.arr(),
        btc_htlc_vout: d.u32(),
        htlc_output_value_sats: d.u64(),
        htlc_output_script: d.bytes(script_len),
        htlc_inclusion_block_hash: d.arr(),
        htlc_inclusion_block_height: d.u32(),
        htlc_inclusion_merkle_proof: (0..htlc_merkle_n).map(|_| d.arr()).collect(),
        claim_spend_txid: d.arr(),
        claim_spend_input_index: d.u32(),
        claim_spend_witness: (0..witness_n)
            .map(|_| {
                let wlen = 1 + d.count(64);
                d.bytes(wlen)
            })
            .collect(),
        revealed_preimage: d.arr(),
        claim_inclusion_block_hash: d.arr(),
        claim_inclusion_block_height: d.u32(),
        claim_inclusion_merkle_proof: (0..claim_merkle_n).map(|_| d.arr()).collect(),
        confirmation_headers: (0..headers_n).map(|_| d.arr::<80>()).collect(),
    }
}

pub fn btc_payout_confirmation_value(v: &BtcPayoutConfirmation) -> Value {
    json!({
        "btc_htlc_txid": hx(&v.btc_htlc_txid),
        "btc_htlc_vout": v.btc_htlc_vout,
        "htlc_output_value_sats": u64s(v.htlc_output_value_sats),
        "htlc_output_script": hx(&v.htlc_output_script),
        "htlc_inclusion_block_hash": hx(&v.htlc_inclusion_block_hash),
        "htlc_inclusion_block_height": v.htlc_inclusion_block_height,
        "htlc_inclusion_merkle_proof": hx_list(&v.htlc_inclusion_merkle_proof),
        "claim_spend_txid": hx(&v.claim_spend_txid),
        "claim_spend_input_index": v.claim_spend_input_index,
        "claim_spend_witness": Value::Array(v.claim_spend_witness.iter().map(|w| hx(w)).collect()),
        "revealed_preimage": hx(&v.revealed_preimage),
        "claim_inclusion_block_hash": hx(&v.claim_inclusion_block_hash),
        "claim_inclusion_block_height": v.claim_inclusion_block_height,
        "claim_inclusion_merkle_proof": hx_list(&v.claim_inclusion_merkle_proof),
        "confirmation_headers": Value::Array(v.confirmation_headers.iter().map(|h| hx(h)).collect()),
    })
}

// ---------------------------------------------------------------- RedemptionRecord

pub fn build_redemption_record(d: &mut Det) -> RedemptionRecord {
    RedemptionRecord {
        redeem_intent_hash: d.arr(),
        lock_record_hash: d.arr(),
        btc_htlc_record_hash: d.arr(),
        btc_payout_confirmation_hash: d.arr(),
        lock_finalize_hash: d.arr(),
        selected_oracle_epoch: d.u64(),
        selected_price_e8: d.u64(),
        gross_btc_sats: d.u64(),
        operator_fee_sats: d.u64(),
        user_payout_sats: d.u64(),
    }
}

pub fn redemption_record_value(v: &RedemptionRecord) -> Value {
    json!({
        "redeem_intent_hash": hx(&v.redeem_intent_hash),
        "lock_record_hash": hx(&v.lock_record_hash),
        "btc_htlc_record_hash": hx(&v.btc_htlc_record_hash),
        "btc_payout_confirmation_hash": hx(&v.btc_payout_confirmation_hash),
        "lock_finalize_hash": hx(&v.lock_finalize_hash),
        "selected_oracle_epoch": u64s(v.selected_oracle_epoch),
        "selected_price_e8": u64s(v.selected_price_e8),
        "gross_btc_sats": u64s(v.gross_btc_sats),
        "operator_fee_sats": u64s(v.operator_fee_sats),
        "user_payout_sats": u64s(v.user_payout_sats),
    })
}

// ----------------------------------------------------------------- ReserveClaim

pub fn build_reserve_claim(d: &mut Det) -> ReserveClaim {
    ReserveClaim {
        claim_id: d.arr(),
        transition_type: 0x11,
        operator_id: d.arr(),
        prev_state_root: d.arr(),
        new_state_root: d.arr(),
        redemption_batch_root: d.arr(),
        oracle_batch_root: d.arr(),
        lock_batch_root: d.arr(),
        payout_batch_root: d.arr(),
        confirmation_batch_root: d.arr(),
        finalize_batch_root: d.arr(),
        burn_proof_batch_root: d.arr(),
        lineage_proof_batch_root: d.arr(),
        live_da_root: d.arr(),
        archival_da_root: d.arr(),
        l1_anchor: build_claim_clock(d),
        reserve_shard_id: d.u64(),
        reimbursement_sats: d.u64(),
        proof_commitment: d.arr(),
        claim_expiry_height: d.u32(),
        operator_signature: d.arr(),
    }
}

pub fn reserve_claim_value(v: &ReserveClaim) -> Value {
    json!({
        "claim_id": hx(&v.claim_id),
        "transition_type": v.transition_type,
        "operator_id": hx(&v.operator_id),
        "prev_state_root": hx(&v.prev_state_root),
        "new_state_root": hx(&v.new_state_root),
        "redemption_batch_root": hx(&v.redemption_batch_root),
        "oracle_batch_root": hx(&v.oracle_batch_root),
        "lock_batch_root": hx(&v.lock_batch_root),
        "payout_batch_root": hx(&v.payout_batch_root),
        "confirmation_batch_root": hx(&v.confirmation_batch_root),
        "finalize_batch_root": hx(&v.finalize_batch_root),
        "burn_proof_batch_root": hx(&v.burn_proof_batch_root),
        "lineage_proof_batch_root": hx(&v.lineage_proof_batch_root),
        "live_da_root": hx(&v.live_da_root),
        "archival_da_root": hx(&v.archival_da_root),
        "l1_anchor": claim_clock_value(&v.l1_anchor),
        "reserve_shard_id": u64s(v.reserve_shard_id),
        "reimbursement_sats": u64s(v.reimbursement_sats),
        "proof_commitment": hx(&v.proof_commitment),
        "claim_expiry_height": v.claim_expiry_height,
        "operator_signature": hx(&v.operator_signature),
    })
}

// -------------------------------------------------------------------- StateRoot

pub fn build_state_root(d: &mut Det) -> StateRoot {
    StateRoot {
        protocol_version: 1,
        state_epoch: d.u64(),
        prev_state_root: d.arr(),
        transition_type: d.u8(),
        satusd_asset_family_id: d.arr(),
        sat_usd_supply_atoms: d.u64(),
        reserve_btc_sats: d.u64(),
        reserved_pending_claim_sats: d.u64(),
        collateral_ratio_ppm: d.u64(),
        emergency_tier: d.u8() % 4,
        oracle_set_hash: d.arr(),
        oracle_set_epoch: d.u64(),
        latest_oracle_epoch_seen: d.u64(),
        latest_oracle_price_e8: d.u64(),
        issuer_positions_root: d.arr(),
        operator_registry_root: d.arr(),
        lock_record_root: d.arr(),
        lock_consumed_root: d.arr(),
        lock_refund_root: d.arr(),
        redemption_nullifier_root: d.arr(),
        pending_claim_root: d.arr(),
        live_da_root: d.arr(),
        archival_da_root: d.arr(),
        l1_anchor_hash: d.arr(),
        l1_anchor_height: d.u32(),
        l1_anchor_mtp: d.u64(),
        l1_anchor_chain_time: d.u64(),
    }
}

pub fn state_root_value(v: &StateRoot) -> Value {
    json!({
        "protocol_version": v.protocol_version,
        "state_epoch": u64s(v.state_epoch),
        "prev_state_root": hx(&v.prev_state_root),
        "transition_type": v.transition_type,
        "satusd_asset_family_id": hx(&v.satusd_asset_family_id),
        "sat_usd_supply_atoms": u64s(v.sat_usd_supply_atoms),
        "reserve_btc_sats": u64s(v.reserve_btc_sats),
        "reserved_pending_claim_sats": u64s(v.reserved_pending_claim_sats),
        "collateral_ratio_ppm": u64s(v.collateral_ratio_ppm),
        "emergency_tier": v.emergency_tier,
        "oracle_set_hash": hx(&v.oracle_set_hash),
        "oracle_set_epoch": u64s(v.oracle_set_epoch),
        "latest_oracle_epoch_seen": u64s(v.latest_oracle_epoch_seen),
        "latest_oracle_price_e8": u64s(v.latest_oracle_price_e8),
        "issuer_positions_root": hx(&v.issuer_positions_root),
        "operator_registry_root": hx(&v.operator_registry_root),
        "lock_record_root": hx(&v.lock_record_root),
        "lock_consumed_root": hx(&v.lock_consumed_root),
        "lock_refund_root": hx(&v.lock_refund_root),
        "redemption_nullifier_root": hx(&v.redemption_nullifier_root),
        "pending_claim_root": hx(&v.pending_claim_root),
        "live_da_root": hx(&v.live_da_root),
        "archival_da_root": hx(&v.archival_da_root),
        "l1_anchor_hash": hx(&v.l1_anchor_hash),
        "l1_anchor_height": v.l1_anchor_height,
        "l1_anchor_mtp": u64s(v.l1_anchor_mtp),
        "l1_anchor_chain_time": u64s(v.l1_anchor_chain_time),
    })
}
