//! Generates the cross-language test-vector file consumed by the TypeScript
//! reference. Run: `cargo run -p satusd-vectors`. Output: integration/vectors/vectors.json

mod det;
mod fixtures;

use std::path::PathBuf;

use satusd_crypto as crypto;
use satusd_types::derive;
use satusd_types::domain;
use satusd_types::encoding::{canonical_encode, Encode};
use satusd_types::hash::sha256;
use satusd_types::types::*;
use serde_json::{json, Value};

use det::Det;
use fixtures::*;

/// One struct vector: fields (for TS rebuild), canonical encoding, and hashes.
fn struct_vector(
    name: &str,
    type_name: &str,
    fields: Value,
    encoding: &[u8],
    hashes: Value,
) -> Value {
    json!({
        "name": name,
        "kind": "struct",
        "type": type_name,
        "fields": fields,
        "encoding_hex": hex::encode(encoding),
        "sha256_of_encoding": hex::encode(sha256(&[encoding])),
        "hashes": hashes,
    })
}

fn enc<T: Encode>(v: &T) -> Vec<u8> {
    canonical_encode(v)
}

fn main() {
    let mut vectors: Vec<Value> = Vec::new();
    let mut d = Det::new(1);

    // Per-type bulk generation. ~80 each across 13 types clears the 1000 floor.
    const PER_TYPE: usize = 80;

    for i in 0..PER_TYPE {
        let v = build_redeem_intent(&mut d, i % 2 == 0);
        vectors.push(struct_vector(
            &format!("redeem_intent_{i}"),
            "RedeemIntent",
            redeem_intent_value(&v),
            &enc(&v),
            json!({ "redeem_intent_hash": hex::encode(derive::redeem_intent_hash(&v)) }),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_claim_clock(&mut d);
        vectors.push(struct_vector(
            &format!("claim_clock_{i}"),
            "ClaimClock",
            claim_clock_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_oracle_message(&mut d);
        vectors.push(struct_vector(
            &format!("oracle_message_{i}"),
            "OracleMessage",
            oracle_message_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_lock_record(&mut d);
        let nf = derive::redemption_nullifier(
            &v.lock_anchor_outpoint,
            &v.lock_script_key,
            &v.redeem_intent_hash,
        );
        vectors.push(struct_vector(
            &format!("lock_record_{i}"),
            "LockRecord",
            lock_record_value(&v),
            &enc(&v),
            json!({ "redemption_nullifier": hex::encode(nf) }),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_lock_finalize(&mut d);
        vectors.push(struct_vector(
            &format!("lock_finalize_{i}"),
            "LockFinalizeRecord",
            lock_finalize_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_lock_refund(&mut d);
        vectors.push(struct_vector(
            &format!("lock_refund_{i}"),
            "LockRefundRecord",
            lock_refund_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_btc_htlc_payout(&mut d);
        vectors.push(struct_vector(
            &format!("btc_htlc_payout_{i}"),
            "BtcHtlcPayoutRecord",
            btc_htlc_payout_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_btc_payout_confirmation(&mut d);
        vectors.push(struct_vector(
            &format!("btc_payout_confirmation_{i}"),
            "BtcPayoutConfirmation",
            btc_payout_confirmation_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_redemption_record(&mut d);
        vectors.push(struct_vector(
            &format!("redemption_record_{i}"),
            "RedemptionRecord",
            redemption_record_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_reserve_claim(&mut d);
        vectors.push(struct_vector(
            &format!("reserve_claim_{i}"),
            "ReserveClaim",
            reserve_claim_value(&v),
            &enc(&v),
            json!({ "claim_id": hex::encode(derive::claim_id(&v)) }),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_state_root(&mut d);
        vectors.push(struct_vector(
            &format!("state_root_{i}"),
            "StateRoot",
            state_root_value(&v),
            &enc(&v),
            json!({}),
        ));
    }

    // Standalone derivations that are not a single struct encoding.
    for i in 0..PER_TYPE {
        let genesis: [u8; 32] = d.arr();
        let group_key: [u8; 33] = d.arr();
        let chain_id = d.u8() % 3;
        let out = derive::asset_family_id(&genesis, &group_key, chain_id);
        vectors.push(json!({
            "name": format!("asset_family_id_{i}"),
            "kind": "derive",
            "type": "asset_family_id",
            "inputs": {
                "genesis_asset_id": hex::encode(genesis),
                "group_key": hex::encode(group_key),
                "chain_id": chain_id,
            },
            "output": hex::encode(out),
        }));
    }

    for i in 0..PER_TYPE {
        let rih: [u8; 32] = d.arr();
        let ph: [u8; 32] = d.arr();
        let out = derive::lock_tweak(&rih, &ph);
        vectors.push(json!({
            "name": format!("lock_tweak_{i}"),
            "kind": "derive",
            "type": "lock_tweak",
            "inputs": {
                "redeem_intent_hash": hex::encode(rih),
                "payment_hash": hex::encode(ph),
            },
            "output": hex::encode(out),
        }));
    }

    // Edge cases: all-zero RedeemIntent (both operator variants).
    let zero_intent_none = RedeemIntent {
        version: 0,
        network: 0,
        redemption_id: [0; 32],
        satusd_asset_family_id: [0; 32],
        amount_satusd_atoms: 0,
        user_btc_refund_pubkey: [0; 32],
        user_btc_claim_pubkey: [0; 32],
        user_asset_refund_key: [0; 32],
        operator_id: None,
        mode: 1,
        payment_hash: [0; 32],
        asset_lock_csv_delta: 0,
        btc_htlc_csv_delta: 0,
        max_operator_fee_bps: 0,
        l1_anchor_height: 0,
        l1_anchor_hash: [0; 32],
        expiry_height: 0,
        nonce: [0; 32],
    };
    vectors.push(struct_vector(
        "redeem_intent_edge_zero_none",
        "RedeemIntent",
        redeem_intent_value(&zero_intent_none),
        &enc(&zero_intent_none),
        json!({ "redeem_intent_hash": hex::encode(derive::redeem_intent_hash(&zero_intent_none)) }),
    ));

    let max_intent_some = RedeemIntent {
        version: u16::MAX,
        network: 255,
        redemption_id: [0xff; 32],
        satusd_asset_family_id: [0xff; 32],
        amount_satusd_atoms: u64::MAX,
        user_btc_refund_pubkey: [0xff; 32],
        user_btc_claim_pubkey: [0xff; 32],
        user_asset_refund_key: [0xff; 32],
        operator_id: Some([0xff; 32]),
        mode: 0,
        payment_hash: [0xff; 32],
        asset_lock_csv_delta: u32::MAX,
        btc_htlc_csv_delta: u32::MAX,
        max_operator_fee_bps: u16::MAX,
        l1_anchor_height: u32::MAX,
        l1_anchor_hash: [0xff; 32],
        expiry_height: u32::MAX,
        nonce: [0xff; 32],
    };
    vectors.push(struct_vector(
        "redeem_intent_edge_max_some",
        "RedeemIntent",
        redeem_intent_value(&max_intent_some),
        &enc(&max_intent_some),
        json!({ "redeem_intent_hash": hex::encode(derive::redeem_intent_hash(&max_intent_some)) }),
    ));

    // Empty variable-length fields on a LockRecord (lineage_verified_by = []).
    let mut empty_lock = build_lock_record(&mut d);
    empty_lock.lineage_verified_by = vec![];
    vectors.push(struct_vector(
        "lock_record_edge_empty_lineage",
        "LockRecord",
        lock_record_value(&empty_lock),
        &enc(&empty_lock),
        json!({ "redemption_nullifier": hex::encode(derive::redemption_nullifier(
            &empty_lock.lock_anchor_outpoint, &empty_lock.lock_script_key, &empty_lock.redeem_intent_hash)) }),
    ));

    // ---- Crypto primitives (cross-checked against circomlibjs / @noble/curves) ----

    // poseidon2: two field inputs given as 31-byte BE values (< Fr, no reduction).
    for i in 0..PER_TYPE {
        let a = d.bytes(31);
        let b = d.bytes(31);
        let out = crypto::poseidon::poseidon2_be(&a, &b);
        vectors.push(json!({
            "name": format!("poseidon2_{i}"),
            "kind": "crypto",
            "op": "poseidon2",
            "inputs": { "a": hex::encode(&a), "b": hex::encode(&b) },
            "output": hex::encode(out),
        }));
    }

    // hash_bytes: 31-byte-limb pack + Poseidon fold, over varied lengths.
    let hb_lengths = [0usize, 1, 30, 31, 32, 62, 63, 100];
    for (i, &len) in hb_lengths.iter().enumerate() {
        let input = d.bytes(len);
        let out = crypto::poseidon::hash_bytes_be(&input);
        vectors.push(json!({
            "name": format!("hash_bytes_len{len}_{i}"),
            "kind": "crypto",
            "op": "hash_bytes",
            "inputs": { "input": hex::encode(&input) },
            "output": hex::encode(out),
        }));
    }

    // burn_sink: asset_family_id -> NUMS internal key, burn tweak, sink script key.
    for i in 0..PER_TYPE {
        let afid: [u8; 32] = d.arr();
        let internal = crypto::nums::protocol_burn_internal_key(&afid);
        let tweak = crypto::nums::protocol_burn_tweak(&afid);
        let sink = crypto::nums::protocol_sink_script_key(&afid);
        vectors.push(json!({
            "name": format!("burn_sink_{i}"),
            "kind": "crypto",
            "op": "burn_sink",
            "inputs": { "asset_family_id": hex::encode(afid) },
            "internal_key": hex::encode(internal),
            "burn_tweak": hex::encode(tweak),
            "sink_script_key": hex::encode(sink),
        }));
    }

    // tap_tweak: a valid x-only internal (a NUMS key) + arbitrary tweak.
    for i in 0..PER_TYPE {
        let salt: [u8; 32] = d.arr();
        let internal = crypto::nums::derive_nums_key(domain::BURN_SINK, &salt);
        let tweak: [u8; 32] = d.arr();
        let out = crypto::nums::tap_tweak(&internal, &tweak);
        vectors.push(json!({
            "name": format!("tap_tweak_{i}"),
            "kind": "crypto",
            "op": "tap_tweak",
            "inputs": { "internal_key": hex::encode(internal), "tweak": hex::encode(tweak) },
            "output": hex::encode(out),
        }));
    }

    // Domain separator registry: name -> raw ASCII bytes (no padding).
    let domains: Vec<Value> = domain::ALL
        .iter()
        .map(|name| json!({ "name": name, "tag_hex": hex::encode(domain::domain_tag(name)) }))
        .collect();

    let doc = json!({
        "schema": "satusd-m0-vectors-v1",
        "domain_padding": "raw-ascii-no-pad",
        "count": vectors.len(),
        "domains": domains,
        "vectors": vectors,
    });

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integration/vectors");
    std::fs::create_dir_all(&out_dir).expect("create vectors dir");
    let out_path = out_dir.join("vectors.json");
    let serialized = serde_json::to_string_pretty(&doc).expect("serialize");
    std::fs::write(&out_path, serialized).expect("write vectors.json");

    println!("wrote {} vectors to {}", doc["count"], out_path.display());
}
