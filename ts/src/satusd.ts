// SatUSD canonical encoders + hash derivations (PRD §6, §18) — TypeScript reference.
// Field order in each encoder mirrors crates/satusd-types/src/types.rs exactly.

import { createHash } from "node:crypto";
import { Encoder, hexToBytes, bytesToHex } from "./encoder.ts";
import { hashBytesHex } from "./crypto.ts";

type Fields = Record<string, any>;

const B = (f: Fields, k: string): Uint8Array => hexToBytes(f[k]);
const N = (f: Fields, k: string): number => f[k] as number;
const U = (f: Fields, k: string): bigint => BigInt(f[k]);
const listB = (f: Fields, k: string): Uint8Array[] => (f[k] as string[]).map(hexToBytes);

export function sha256(...parts: Uint8Array[]): Uint8Array {
  const h = createHash("sha256");
  for (const p of parts) h.update(p);
  return new Uint8Array(h.digest());
}

export function sha256Hex(...parts: Uint8Array[]): string {
  return bytesToHex(sha256(...parts));
}

export function domainTag(name: string): Uint8Array {
  return new Uint8Array(Buffer.from(name, "utf8"));
}

function encodeOutpoint(e: Encoder, o: any): void {
  e.fixed(hexToBytes(o.txid));
  e.u32(o.vout as number);
}

// --------------------------------------------------------------- per-type encoders

function encodeRedeemIntent(e: Encoder, f: Fields): void {
  e.u16(N(f, "version"));
  e.u8(N(f, "network"));
  e.fixed(B(f, "redemption_id"));
  e.fixed(B(f, "satusd_asset_family_id"));
  e.u64(U(f, "amount_satusd_atoms"));
  e.fixed(B(f, "user_btc_refund_pubkey"));
  e.fixed(B(f, "user_btc_claim_pubkey"));
  e.fixed(B(f, "user_asset_refund_key"));
  e.opt(f["operator_id"] ?? null, (e, v) => e.fixed(hexToBytes(v)));
  e.u8(N(f, "mode"));
  e.fixed(B(f, "payment_hash"));
  e.u32(N(f, "asset_lock_csv_delta"));
  e.u32(N(f, "btc_htlc_csv_delta"));
  e.u16(N(f, "max_operator_fee_bps"));
  e.u32(N(f, "l1_anchor_height"));
  e.fixed(B(f, "l1_anchor_hash"));
  e.u32(N(f, "expiry_height"));
  e.fixed(B(f, "nonce"));
}

function encodeClaimClock(e: Encoder, f: Fields): void {
  e.u32(N(f, "l1_anchor_height"));
  e.fixed(B(f, "l1_anchor_hash"));
  e.u64(U(f, "l1_anchor_mtp"));
  e.u64(U(f, "l1_anchor_chain_time"));
  for (const h of f["recent_header_chain"] as string[]) e.fixed(hexToBytes(h));
  e.u64(U(f, "oracle_epoch"));
  e.u64(U(f, "selected_oracle_price_e8"));
  e.u32(N(f, "max_epoch_lag_sec"));
  e.u32(N(f, "oracle_future_tolerance"));
}

function encodeOracleMessage(e: Encoder, f: Fields): void {
  e.fixed(B(f, "domain"));
  e.fixed(B(f, "oracle_id"));
  e.u64(U(f, "oracle_set_epoch"));
  e.u64(U(f, "price_epoch"));
  e.u64(U(f, "timestamp_ms"));
  e.fixed(B(f, "pair"));
  e.u64(U(f, "price_e8"));
  e.fixed(B(f, "source_commitment"));
  e.fixed(B(f, "signer_pubkey"));
  e.fixed(B(f, "signature"));
}

function encodeLockRecord(e: Encoder, f: Fields): void {
  e.u16(N(f, "lock_record_version"));
  e.fixed(B(f, "redeem_intent_hash"));
  encodeOutpoint(e, f["lock_anchor_outpoint"]);
  e.fixed(B(f, "lock_anchor_txid"));
  e.fixed(B(f, "lock_script_key"));
  e.u64(U(f, "lock_amount_atoms"));
  e.fixed(B(f, "asset_family_id"));
  e.u32(N(f, "asset_lock_csv_delta"));
  e.fixed(B(f, "payment_hash"));
  e.fixed(B(f, "lineage_proof_hash"));
  e.seq(listB(f, "lineage_verified_by"), (e, h) => e.fixed(h));
  e.u32(N(f, "anchor_inclusion_height"));
}

function encodeLockFinalize(e: Encoder, f: Fields): void {
  e.fixed(B(f, "lock_record_hash"));
  e.fixed(B(f, "payment_preimage"));
  e.fixed(B(f, "finalize_anchor_txid"));
  encodeOutpoint(e, f["finalize_anchor_outpoint"]);
  e.fixed(B(f, "protocol_sink_script_key"));
  e.fixed(B(f, "protocol_burn_internal_key"));
  e.u64(U(f, "finalized_amount_atoms"));
  e.fixed(B(f, "operator_id"));
  e.u32(N(f, "finalize_height"));
  e.fixed(B(f, "universe_burn_proof_hash"));
}

function encodeLockRefund(e: Encoder, f: Fields): void {
  e.fixed(B(f, "lock_record_hash"));
  e.fixed(B(f, "refund_anchor_txid"));
  encodeOutpoint(e, f["refund_anchor_outpoint"]);
  e.fixed(B(f, "user_signature"));
  e.u32(N(f, "refund_height"));
  e.fixed(B(f, "asset_returned_to"));
}

function encodeBtcHtlcPayout(e: Encoder, f: Fields): void {
  e.fixed(B(f, "operator_id"));
  e.fixed(B(f, "redeem_intent_hash"));
  e.fixed(B(f, "btc_htlc_txid"));
  e.u32(N(f, "btc_htlc_vout"));
  e.fixed(B(f, "payment_hash"));
  e.fixed(B(f, "user_claim_pubkey"));
  e.fixed(B(f, "operator_refund_pubkey"));
  e.u64(U(f, "payout_sats"));
  e.u32(N(f, "btc_csv_delta"));
  e.u32(N(f, "htlc_inclusion_height"));
  e.fixed(B(f, "htlc_inclusion_block_hash"));
  e.fixed(B(f, "claim_spend_txid"));
  e.fixed(B(f, "revealed_preimage"));
  e.u32(N(f, "claim_inclusion_height"));
  e.fixed(B(f, "claim_inclusion_block_hash"));
  e.u32(N(f, "confirmation_depth"));
}

function encodeBtcPayoutConfirmation(e: Encoder, f: Fields): void {
  e.fixed(B(f, "btc_htlc_txid"));
  e.u32(N(f, "btc_htlc_vout"));
  e.u64(U(f, "htlc_output_value_sats"));
  e.varBytes(B(f, "htlc_output_script"));
  e.fixed(B(f, "htlc_inclusion_block_hash"));
  e.u32(N(f, "htlc_inclusion_block_height"));
  e.seq(listB(f, "htlc_inclusion_merkle_proof"), (e, h) => e.fixed(h));
  e.fixed(B(f, "claim_spend_txid"));
  e.u32(N(f, "claim_spend_input_index"));
  e.seq(listB(f, "claim_spend_witness"), (e, w) => e.varBytes(w));
  e.fixed(B(f, "revealed_preimage"));
  e.fixed(B(f, "claim_inclusion_block_hash"));
  e.u32(N(f, "claim_inclusion_block_height"));
  e.seq(listB(f, "claim_inclusion_merkle_proof"), (e, h) => e.fixed(h));
  e.seq(listB(f, "confirmation_headers"), (e, h) => e.fixed(h));
}

function encodeRedemptionRecord(e: Encoder, f: Fields): void {
  e.fixed(B(f, "redeem_intent_hash"));
  e.fixed(B(f, "lock_record_hash"));
  e.fixed(B(f, "btc_htlc_record_hash"));
  e.fixed(B(f, "btc_payout_confirmation_hash"));
  e.fixed(B(f, "lock_finalize_hash"));
  e.u64(U(f, "selected_oracle_epoch"));
  e.u64(U(f, "selected_price_e8"));
  e.u64(U(f, "gross_btc_sats"));
  e.u64(U(f, "operator_fee_sats"));
  e.u64(U(f, "user_payout_sats"));
}

function encodeReserveClaimForClaimId(e: Encoder, f: Fields): void {
  e.u8(N(f, "transition_type"));
  e.fixed(B(f, "operator_id"));
  e.fixed(B(f, "prev_state_root"));
  e.fixed(B(f, "new_state_root"));
  e.fixed(B(f, "redemption_batch_root"));
  e.fixed(B(f, "oracle_batch_root"));
  e.fixed(B(f, "lock_batch_root"));
  e.fixed(B(f, "payout_batch_root"));
  e.fixed(B(f, "confirmation_batch_root"));
  e.fixed(B(f, "finalize_batch_root"));
  e.fixed(B(f, "burn_proof_batch_root"));
  e.fixed(B(f, "lineage_proof_batch_root"));
  e.fixed(B(f, "live_da_root"));
  e.fixed(B(f, "archival_da_root"));
  encodeClaimClock(e, f["l1_anchor"]);
  e.u64(U(f, "reserve_shard_id"));
  e.u64(U(f, "reimbursement_sats"));
  e.fixed(B(f, "proof_commitment"));
  e.u32(N(f, "claim_expiry_height"));
}

function encodeReserveClaim(e: Encoder, f: Fields): void {
  e.fixed(B(f, "claim_id"));
  encodeReserveClaimForClaimId(e, f);
  e.fixed(B(f, "operator_signature"));
}

function encodeStateRoot(e: Encoder, f: Fields): void {
  e.u16(N(f, "protocol_version"));
  e.u64(U(f, "state_epoch"));
  e.fixed(B(f, "prev_state_root"));
  e.u8(N(f, "transition_type"));
  e.fixed(B(f, "satusd_asset_family_id"));
  e.u64(U(f, "sat_usd_supply_atoms"));
  e.u64(U(f, "reserve_btc_sats"));
  e.u64(U(f, "reserved_pending_claim_sats"));
  e.u64(U(f, "collateral_ratio_ppm"));
  e.u8(N(f, "emergency_tier"));
  e.fixed(B(f, "oracle_set_hash"));
  e.u64(U(f, "oracle_set_epoch"));
  e.u64(U(f, "latest_oracle_epoch_seen"));
  e.u64(U(f, "latest_oracle_price_e8"));
  e.fixed(B(f, "issuer_positions_root"));
  e.fixed(B(f, "operator_registry_root"));
  e.fixed(B(f, "lock_record_root"));
  e.fixed(B(f, "lock_consumed_root"));
  e.fixed(B(f, "lock_refund_root"));
  e.fixed(B(f, "redemption_nullifier_root"));
  e.fixed(B(f, "pending_claim_root"));
  e.fixed(B(f, "live_da_root"));
  e.fixed(B(f, "archival_da_root"));
  e.fixed(B(f, "l1_anchor_hash"));
  e.u32(N(f, "l1_anchor_height"));
  e.u64(U(f, "l1_anchor_mtp"));
  e.u64(U(f, "l1_anchor_chain_time"));
}

function encodeIssuerPosition(e: Encoder, f: Fields): void {
  e.fixed(B(f, "issuer_id"));
  e.enumU8(N(f, "status"));
  e.seq(listB(f, "multisig_pubkeys"), (e, pk) => e.fixed(pk));
  e.u8(N(f, "multisig_threshold"));
  e.u64(U(f, "reserve_deposits_sats"));
  e.u64(U(f, "minted_satusd_atoms"));
  e.u64(U(f, "pending_mint_atoms"));
  e.u64(U(f, "collateral_ratio_ppm"));
  e.opt(f["last_deposit_txid"] ?? null, (e, v) => e.fixed(hexToBytes(v)));
  e.opt(f["freeze_reason"] ?? null, (e, v) => e.enumU8(v as number));
  e.u32(N(f, "registered_at_height"));
}

function encodePendingClaim(e: Encoder, f: Fields): void {
  e.fixed(B(f, "claim_id"));
  e.fixed(B(f, "operator_id"));
  e.u64(U(f, "reserved_sats"));
  e.u32(N(f, "claim_created_height"));
  e.u32(N(f, "claim_expiry_height"));
  e.enumU8(N(f, "status"));
}

const ENCODERS: Record<string, (e: Encoder, f: Fields) => void> = {
  IssuerPosition: encodeIssuerPosition,
  PendingClaim: encodePendingClaim,
  RedeemIntent: encodeRedeemIntent,
  ClaimClock: encodeClaimClock,
  OracleMessage: encodeOracleMessage,
  LockRecord: encodeLockRecord,
  LockFinalizeRecord: encodeLockFinalize,
  LockRefundRecord: encodeLockRefund,
  BtcHtlcPayoutRecord: encodeBtcHtlcPayout,
  BtcPayoutConfirmation: encodeBtcPayoutConfirmation,
  RedemptionRecord: encodeRedemptionRecord,
  ReserveClaim: encodeReserveClaim,
  StateRoot: encodeStateRoot,
};

export function encodeByType(type: string, fields: Fields): Uint8Array {
  const fn = ENCODERS[type];
  if (!fn) throw new Error(`no encoder for type ${type}`);
  const e = new Encoder();
  fn(e, fields);
  return e.bytes();
}

// --------------------------------------------------------------- derivations

function u32be(v: number): Uint8Array {
  return new Uint8Array([(v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff]);
}

// Type-specific hashes attached to struct vectors.
export function structHashes(type: string, fields: Fields): Record<string, string> {
  switch (type) {
    case "RedeemIntent":
      return {
        redeem_intent_hash: sha256Hex(domainTag("SATUSD_REDEEM_INTENT_V1"), encodeByType(type, fields)),
      };
    case "ReserveClaim": {
      const e = new Encoder();
      encodeReserveClaimForClaimId(e, fields);
      return { claim_id: sha256Hex(domainTag("SATUSD_CLAIM_ID_V1"), e.bytes()) };
    }
    case "LockRecord": {
      const o = fields["lock_anchor_outpoint"];
      return {
        redemption_nullifier: sha256Hex(
          domainTag("SATUSD_REDEMPTION_NULLIFIER_V1"),
          hexToBytes(o.txid),
          u32be(o.vout as number),
          B(fields, "lock_script_key"),
          B(fields, "redeem_intent_hash"),
        ),
      };
    }
    case "IssuerPosition":
      return {
        issuer_position_hash: sha256Hex(domainTag("SATUSD_ISSUER_POSITION_V1"), encodeByType(type, fields)),
      };
    case "PendingClaim":
      return {
        pending_claim_hash: sha256Hex(domainTag("SATUSD_PENDING_CLAIM_V1"), encodeByType(type, fields)),
      };
    case "StateRoot":
      return {
        state_root_hash: hashBytesHex(bytesToHex(encodeByType(type, fields))),
      };
    default:
      return {};
  }
}

// Standalone derive vectors.
export function deriveOutput(type: string, inputs: Fields): string {
  switch (type) {
    case "asset_family_id":
      return sha256Hex(
        domainTag("SATUSD_ASSET_FAMILY_V1"),
        B(inputs, "genesis_asset_id"),
        B(inputs, "group_key"),
        new Uint8Array([N(inputs, "chain_id")]),
      );
    case "lock_tweak":
      return sha256Hex(
        domainTag("SATUSD_LOCK_TWEAK_V1"),
        B(inputs, "redeem_intent_hash"),
        B(inputs, "payment_hash"),
      );
    default:
      throw new Error(`no derivation for type ${type}`);
  }
}
