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
            json!({
                "redemption_nullifier": hex::encode(nf),
                "lock_record_hash": hex::encode(derive::lock_record_hash(&v)),
            }),
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
            json!({ "state_root_hash": hex::encode(crypto::state::state_root_hash(&v)) }),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_issuer_position(&mut d, i);
        vectors.push(struct_vector(
            &format!("issuer_position_{i}"),
            "IssuerPosition",
            issuer_position_value(&v),
            &enc(&v),
            json!({ "issuer_position_hash": hex::encode(derive::issuer_position_hash(&v)) }),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_pending_claim(&mut d, i);
        vectors.push(struct_vector(
            &format!("pending_claim_{i}"),
            "PendingClaim",
            pending_claim_value(&v),
            &enc(&v),
            json!({ "pending_claim_hash": hex::encode(derive::pending_claim_hash(&v)) }),
        ));
    }

    for i in 0..PER_TYPE {
        let v = build_operator_position(&mut d, i);
        vectors.push(struct_vector(
            &format!("operator_position_{i}"),
            "OperatorPosition",
            operator_position_value(&v),
            &enc(&v),
            json!({ "operator_position_hash": hex::encode(derive::operator_position_hash(&v)) }),
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

    for i in 0..PER_TYPE {
        let requested = d.u64();
        let meta: [u8; 32] = d.arr();
        let txid: [u8; 32] = d.arr();
        let out = derive::mint_commitment(requested, &meta, &txid);
        vectors.push(json!({
            "name": format!("mint_commitment_{i}"),
            "kind": "derive",
            "type": "mint_commitment",
            "inputs": {
                "requested_mint_atoms": requested.to_string(),
                "asset_metadata_commitment": hex::encode(meta),
                "deposit_txid": hex::encode(txid),
            },
            "output": hex::encode(out),
        }));
    }

    for i in 0..PER_TYPE {
        let issuer_id: [u8; 32] = d.arr();
        let requested = d.u64();
        let deposit_sats = d.u64();
        let txid: [u8; 32] = d.arr();
        let meta: [u8; 32] = d.arr();
        let out = derive::mint_request_sighash(&issuer_id, requested, deposit_sats, &txid, &meta);
        vectors.push(json!({
            "name": format!("mint_request_sighash_{i}"),
            "kind": "derive",
            "type": "mint_request_sighash",
            "inputs": {
                "issuer_id": hex::encode(issuer_id),
                "requested_mint_atoms": requested.to_string(),
                "deposit_sats": deposit_sats.to_string(),
                "deposit_txid": hex::encode(txid),
                "asset_metadata_commitment": hex::encode(meta),
            },
            "output": hex::encode(out),
        }));
    }

    for i in 0..PER_TYPE {
        let n = 3 + (i % 3); // 3..=5 signers
        let pubkeys: Vec<[u8; 32]> = (0..n).map(|_| d.arr::<32>()).collect();
        let epoch = d.u64();
        let out = derive::oracle_set_hash(epoch, &pubkeys);
        vectors.push(json!({
            "name": format!("oracle_set_hash_{i}"),
            "kind": "derive",
            "type": "oracle_set_hash",
            "inputs": {
                "oracle_set_epoch": epoch.to_string(),
                "signer_pubkeys": Value::Array(
                    pubkeys.iter().map(|p| Value::from(hex::encode(p))).collect()
                ),
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
        json!({
            "redemption_nullifier": hex::encode(derive::redemption_nullifier(
                &empty_lock.lock_anchor_outpoint, &empty_lock.lock_script_key, &empty_lock.redeem_intent_hash)),
            "lock_record_hash": hex::encode(derive::lock_record_hash(&empty_lock)),
        }),
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

    // nums_key: domain + salt -> x-only NUMS key (§18.7). Pins the lock-anchor
    // NUMS internal key (§5.D3, empty salt).
    {
        let key = crypto::nums::derive_nums_key(domain::LOCK_ANCHOR_NUMS, &[]);
        vectors.push(json!({
            "name": "nums_key_lock_anchor",
            "kind": "crypto",
            "op": "nums_key",
            "inputs": { "domain": domain::LOCK_ANCHOR_NUMS, "salt": "" },
            "output": hex::encode(key),
        }));

        // Operator BTC HTLC NUMS internal key (§5.D5, §18.2): salted by payment_hash.
        let payment_hash = [0x42u8; 32];
        let htlc_key = crypto::nums::derive_nums_key(domain::BTC_HTLC_NUMS, &payment_hash);
        vectors.push(json!({
            "name": "nums_key_btc_htlc",
            "kind": "crypto",
            "op": "nums_key",
            "inputs": { "domain": domain::BTC_HTLC_NUMS, "salt": hex::encode(payment_hash) },
            "output": hex::encode(htlc_key),
        }));
    }

    // live_da_root: §10.3 Live DA bundle root = SHA256(concat(SHA256(section_i))),
    // section_i = id || size_be || content. Pins the per-section hashing the
    // challenger uses to recompute a claim's committed `live_da_root`.
    {
        let sections = [
            (satusd_da::section::REDEEM_INTENTS, vec![0x01u8, 0x02, 0x03]),
            (satusd_da::section::TA_LINEAGE_PROOFS, vec![0x09u8, 0x09]),
            (satusd_da::section::ORACLE_MESSAGES, vec![0x04u8; 40]),
        ];
        let bundle = satusd_da::LiveDABundle::new(
            [0xab; 32],
            7,
            sections
                .iter()
                .map(|(id, c)| satusd_da::Section {
                    id: *id,
                    content: c.clone(),
                })
                .collect(),
        );
        vectors.push(json!({
            "name": "live_da_root_3section",
            "kind": "crypto",
            "op": "live_da_root",
            "inputs": {
                "sections": sections
                    .iter()
                    .map(|(id, c)| json!({ "id": id, "content": hex::encode(c) }))
                    .collect::<Vec<_>>(),
            },
            "output": hex::encode(bundle.live_da_root()),
        }));
    }

    // tier: §5.D8 CR/tier (DL-24 corrected formula). The G3 gate requires ≥ 200
    // cross-language fixtures spanning every tier band + dimension extreme. We pin
    // the documented scenarios + boundaries, a set of range/undefined edges, and a
    // full (reserve × supply × price) sweep. Rust generates, TS re-verifies, and
    // the M4a circuit (`test_tier.mjs`) re-derives cr_ppm/tier — a 3-way match.
    let mut tier_cases: Vec<(u64, u64, u64)> = vec![
        // §5.D8 documented scenarios + threshold boundaries (price = $50k = 5e12).
        (4_000_000_000, 100_000_000, 5_000_000_000_000), // 200% Healthy
        (3_000_000_000, 100_000_000, 5_000_000_000_000), // 150% Healthy (boundary)
        (2_600_000_000, 100_000_000, 5_000_000_000_000), // 130% PauseMint (boundary)
        (2_400_000_000, 100_000_000, 5_000_000_000_000), // 120% Auction
        (2_200_000_000, 100_000_000, 5_000_000_000_000), // 110% Auction (boundary)
        (2_000_000_000, 100_000_000, 5_000_000_000_000), // 100% Settlement
        (100_000_000, 100, 5_000_000_000_000),           // tiny supply (cr_ppm = 5e10)
        // Edges (§18.3 ranges): undefined CR, zero reserve, dimension extremes.
        (1_000_000_000, 0, 5_000_000_000_000), // supply = 0 → None → Healthy
        (0, 100_000_000, 5_000_000_000_000),   // reserve = 0 → cr 0 → Settlement
        (
            2_100_000_000_000_000,
            100_000_000_000_000,
            100_000_000_000_000,
        ), // all max
        (2_100_000_000_000_000, 1_000_000, 1_000_000_000), // max reserve, min price
        (1, 100_000_000_000_000, 100_000_000_000_000), // 1-sat reserve, max supply
    ];
    // Full sweep: supply ≥ 1e6 keeps cr_ppm within u64 at the reserve/price max.
    let sweep_reserves: [u64; 12] = [
        100_000_000,
        300_000_000,
        1_000_000_000,
        3_000_000_000,
        10_000_000_000,
        30_000_000_000,
        100_000_000_000,
        1_000_000_000_000,
        10_000_000_000_000,
        100_000_000_000_000,
        1_000_000_000_000_000,
        2_100_000_000_000_000,
    ];
    let sweep_supplies: [u64; 6] = [
        1_000_000,
        10_000_000,
        100_000_000,
        10_000_000_000,
        1_000_000_000_000,
        100_000_000_000_000,
    ];
    let sweep_prices: [u64; 3] = [1_000_000_000, 5_000_000_000_000, 100_000_000_000_000];
    for &reserve in &sweep_reserves {
        for &supply in &sweep_supplies {
            for &price in &sweep_prices {
                tier_cases.push((reserve, supply, price));
            }
        }
    }
    // Targeted CR sweep: solve `reserve = cr_ppm · supply · 1e8 / price` so each
    // case lands in a chosen band (incl. the narrow PauseMint/Auction ranges and
    // the exact 110/130/150% boundaries), across several supply/price scales.
    let target_ppms: [u64; 9] = [
        900_000, 1_050_000, 1_100_000, 1_150_000, 1_250_000, 1_300_000, 1_350_000, 1_450_000,
        1_500_000,
    ];
    for &target in &target_ppms {
        for &supply in &[1_000_000u64, 100_000_000, 10_000_000_000, 1_000_000_000_000] {
            for &price in &sweep_prices {
                let reserve = (target as u128 * supply as u128 * 100_000_000u128) / price as u128;
                if reserve > 0 && reserve <= 2_100_000_000_000_000 {
                    tier_cases.push((reserve as u64, supply, price));
                }
            }
        }
    }
    for (i, (reserve, supply, price)) in tier_cases.iter().enumerate() {
        let cr = satusd_types::tier::collateral_ratio_ppm(*reserve, *supply, *price);
        let tier = satusd_types::tier::recompute_tier(*reserve, *supply, *price);
        vectors.push(json!({
            "name": format!("tier_{i}"),
            "kind": "crypto",
            "op": "tier",
            "inputs": {
                "reserve_sats": reserve.to_string(),
                "supply_atoms": supply.to_string(),
                "price_e8": price.to_string(),
            },
            "cr_ppm": cr.map(|c| c.to_string()),
            "tier": tier.as_u8(),
        }));
    }

    // batch_root: binary Poseidon merkle over 4 field-element leaves (§6.8 batch
    // roots; M4a circuit `batch_root` gadget matches this).
    for i in 0..PER_TYPE {
        let leaves: Vec<[u8; 32]> = (0..4)
            .map(|_| {
                let mut b = [0u8; 32];
                b[1..].copy_from_slice(&d.bytes(31)); // 31-byte BE, < Fr
                b
            })
            .collect();
        let out = crypto::poseidon::batch_root_be(&leaves);
        vectors.push(json!({
            "name": format!("batch_root_{i}"),
            "kind": "crypto",
            "op": "batch_root",
            "inputs": {
                "leaves": Value::Array(leaves.iter().map(|l| Value::from(hex::encode(l))).collect()),
            },
            "output": hex::encode(out),
        }));
    }

    // smt: SMT membership fold (ADR-0015; M4b circuit `SmtFold` + TS `smtFoldHex`
    // re-fold these to the same root → 3-way). Inclusion folds the occupied leaf,
    // non-membership folds the empty leaf (0). siblings indexed by depth (0 = top).
    {
        use crypto::poseidon::fr_to_be_bytes;
        use crypto::smt::{leaf_hash, SparseMerkleTree};
        let occupied: [([u8; 32], [u8; 32]); 3] = [
            ([0x11; 32], [0x01; 32]),
            ([0x22; 32], [0x02; 32]),
            ([0xab; 32], [0x03; 32]),
        ];
        let mut t = SparseMerkleTree::new();
        for (k, v) in &occupied {
            t.insert(*k, v);
        }
        let root = hex::encode(t.root());
        let emit = |vectors: &mut Vec<Value>, name: String, key: &[u8; 32], leaf: [u8; 32]| {
            let sibs = t.prove(key);
            vectors.push(json!({
                "name": name,
                "kind": "crypto",
                "op": "smt",
                "inputs": {
                    "key": hex::encode(key),
                    "leaf": hex::encode(leaf),
                    "siblings": Value::Array(sibs.iter().map(|s| Value::from(hex::encode(s))).collect()),
                },
                "output": root,
            }));
        };
        for (i, (k, v)) in occupied.iter().enumerate() {
            emit(
                &mut vectors,
                format!("smt_incl_{i}"),
                k,
                fr_to_be_bytes(&leaf_hash(k, v)),
            );
        }
        for (i, k) in [[0x99u8; 32], [0x55u8; 32]].iter().enumerate() {
            emit(&mut vectors, format!("smt_excl_{i}"), k, [0u8; 32]);
        }

        // smt_insert: a §5.D17 set-insert (M4b `LockInsert`). One exclusion path
        // proves the key is absent (old_root) and that setting it to `leaf` gives
        // new_root — `root_after_update` with the non-membership precondition.
        use crypto::smt::root_after_update;
        let old_root = t.root();
        for (i, (k, v)) in [([0x99u8; 32], [0x09u8; 32]), ([0x55u8; 32], [0x05u8; 32])]
            .iter()
            .enumerate()
        {
            let sibs = t.prove(k);
            let leaf = fr_to_be_bytes(&leaf_hash(k, v));
            let new_root = root_after_update(k, v, &sibs);
            vectors.push(json!({
                "name": format!("smt_insert_{i}"),
                "kind": "crypto",
                "op": "smt_insert",
                "inputs": {
                    "key": hex::encode(k),
                    "leaf": hex::encode(leaf),
                    "siblings": Value::Array(sibs.iter().map(|s| Value::from(hex::encode(s))).collect()),
                },
                "old_root": hex::encode(old_root),
                "new_root": hex::encode(new_root),
            }));
        }

        // lock_finalize: the §5.D17 active→consumed transition (M4b `LockFinalize`)
        // — lock_record membership + lock_consumed insert (same key) + nullifier
        // insert, mirroring claim.rs's SMT logic.
        let member = [1u8; 32]; // SET_MEMBER
        let k_lock = [0x4c; 32];
        let k_nf = [0x6e; 32];
        let lock_leaf = fr_to_be_bytes(&leaf_hash(&k_lock, &member));
        let nf_leaf = fr_to_be_bytes(&leaf_hash(&k_nf, &member));

        let mut lr = SparseMerkleTree::new();
        lr.insert([0x01; 32], &member); // another lock, for a non-trivial path
        lr.insert(k_lock, &member);
        let lock_membership_siblings = lr.prove(&k_lock);

        let consumed = SparseMerkleTree::new(); // k_lock not yet consumed
        let consumed_siblings = consumed.prove(&k_lock);
        let nfs = SparseMerkleTree::new(); // nullifier unused
        let nf_siblings = nfs.prove(&k_nf);

        let hl =
            |v: &[[u8; 32]]| Value::Array(v.iter().map(|s| Value::from(hex::encode(s))).collect());
        vectors.push(json!({
            "name": "lock_finalize_0",
            "kind": "crypto",
            "op": "lock_finalize",
            "inputs": {
                "lock_leaf": hex::encode(lock_leaf),
                "lock_key": hex::encode(k_lock),
                "lock_membership_siblings": hl(&lock_membership_siblings),
                "consumed_siblings": hl(&consumed_siblings),
                "nf_leaf": hex::encode(nf_leaf),
                "nf_key": hex::encode(k_nf),
                "nf_siblings": hl(&nf_siblings),
            },
            "lock_record_root": hex::encode(lr.root()),
            "consumed_old": hex::encode(consumed.root()),
            "consumed_new": hex::encode(root_after_update(&k_lock, &member, &consumed_siblings)),
            "nf_old": hex::encode(nfs.root()),
            "nf_new": hex::encode(root_after_update(&k_nf, &member, &nf_siblings)),
        }));
    }

    // oracle_eddsa: an EdDSA-BabyJub price attestation over
    // oracle_message_hash = Poseidon([set_epoch, price_epoch, ts_ms, price_e8])
    // (§5.D7; M4c circuit `OracleEddsa` recomputes the hash + verifies the sig).
    // Pinned circomlibjs reference signer (privkey 0x11×32).
    {
        let (set_epoch, price_epoch, ts_ms, price_e8) =
            (7u64, 100u64, 1_700_000_000_000u64, 5_000_000_000_000u64);
        let pubkey = "323a1772ccd2bf78ca0f82e4de1d4d48ded87f6f26d92d6a99e5998ac88901a6";
        let sig = "e1c966e0d52d5f5b20161c5b653101c10c7935521980770d838d826fbc93c42e35351a47e0b0d02009c15179e144ba6780244d10d391fbda0d5411f7b8562a02";
        let msg = crypto::poseidon::oracle_message_hash(set_epoch, price_epoch, ts_ms, price_e8);
        let pk: [u8; 32] = hex::decode(pubkey).unwrap().try_into().unwrap();
        let s: [u8; 64] = hex::decode(sig).unwrap().try_into().unwrap();
        assert!(
            crypto::eddsa::verify_eddsa_babyjub(&pk, &msg, &s),
            "oracle_eddsa vector must verify (babyjubjub-rs)"
        );
        vectors.push(json!({
            "name": "oracle_eddsa_0",
            "kind": "crypto",
            "op": "oracle_eddsa",
            "inputs": {
                "oracle_set_epoch": set_epoch.to_string(),
                "price_epoch": price_epoch.to_string(),
                "timestamp_ms": ts_ms.to_string(),
                "price_e8": price_e8.to_string(),
                "pubkey": pubkey,
                "sig": sig,
            },
            "output": hex::encode(msg),
        }));
    }

    // state_commit: state_root_hash = hash_bytes(canonical_encode(StateRoot)),
    // folded as 31-byte big-endian limbs via poseidon2 (M4 circuit `StateCommit`).
    {
        let s = build_state_root(&mut d);
        let enc = canonical_encode(&s);
        let limbs: Vec<Value> = enc
            .chunks(31)
            .map(|c| {
                let mut buf = [0u8; 32];
                buf[32 - c.len()..].copy_from_slice(c); // BE, left-padded < Fr
                Value::from(hex::encode(buf))
            })
            .collect();
        vectors.push(json!({
            "name": "state_commit_0",
            "kind": "crypto",
            "op": "state_commit",
            "inputs": { "limbs": Value::Array(limbs), "byte_len": enc.len() },
            "output": hex::encode(crypto::state::state_root_hash(&s)),
        }));
    }

    // state_commit_fields: Poseidon-over-fields state commitment (ADR-006, M7) —
    // 42 field elements (scalars + each 32-byte value as hi/lo limbs), poseidon2-fold.
    let fields_hex = |s: &StateRoot| -> Vec<Value> {
        crypto::state::state_field_elements(s)
            .iter()
            .map(|f| Value::from(hex::encode(f)))
            .collect()
    };
    {
        let s = build_state_root(&mut d);
        vectors.push(json!({
            "name": "state_commit_fields_0",
            "kind": "crypto",
            "op": "state_commit_fields",
            "inputs": { "fields": Value::Array(fields_hex(&s)) },
            "output": hex::encode(crypto::state::state_commit_fields(&s)),
        }));
    }

    // m7_transition: the monolithic REDEEM_FAST_FINALIZE (single redemption) —
    // the §5.D17 lock state machine folded into the Poseidon-over-fields state
    // commitment, binding the lock roots embedded in prev/new state (ADR-006 M7).
    {
        use crypto::poseidon::fr_to_be_bytes;
        use crypto::smt::{leaf_hash, root_after_update, SparseMerkleTree};

        let member = [1u8; 32]; // SET_MEMBER
        let k_lock = [0x4c; 32];
        let k_nf = [0x6e; 32];
        let lock_leaf = fr_to_be_bytes(&leaf_hash(&k_lock, &member));
        let nf_leaf = fr_to_be_bytes(&leaf_hash(&k_nf, &member));

        let mut lr = SparseMerkleTree::new();
        lr.insert([0x01; 32], &member); // a second lock, for a non-trivial path
        lr.insert(k_lock, &member);
        let lock_membership_siblings = lr.prove(&k_lock);
        let consumed = SparseMerkleTree::new(); // k_lock not yet consumed
        let consumed_siblings = consumed.prove(&k_lock);
        let nfs = SparseMerkleTree::new(); // nullifier unused
        let nf_siblings = nfs.prove(&k_nf);
        let consumed_new = root_after_update(&k_lock, &member, &consumed_siblings);
        let nf_new = root_after_update(&k_nf, &member, &nf_siblings);

        let amount = 4_000u64;
        let mut prev = build_state_root(&mut d);
        prev.sat_usd_supply_atoms = 100_000_000;
        prev.lock_record_root = lr.root();
        prev.lock_consumed_root = consumed.root();
        prev.redemption_nullifier_root = nfs.root();
        let mut new = prev.clone();
        new.state_epoch = prev.state_epoch + 1;
        new.lock_consumed_root = consumed_new;
        new.redemption_nullifier_root = nf_new;
        new.sat_usd_supply_atoms = prev.sat_usd_supply_atoms - amount;

        let hl =
            |v: &[[u8; 32]]| Value::Array(v.iter().map(|s| Value::from(hex::encode(s))).collect());
        vectors.push(json!({
            "name": "m7_transition_0",
            "kind": "crypto",
            "op": "m7_transition",
            "inputs": {
                "prev_fields": Value::Array(fields_hex(&prev)),
                "new_fields": Value::Array(fields_hex(&new)),
                "lock_leaf": hex::encode(lock_leaf),
                "lock_key": hex::encode(k_lock),
                "lock_membership_siblings": hl(&lock_membership_siblings),
                "consumed_siblings": hl(&consumed_siblings),
                "nf_leaf": hex::encode(nf_leaf),
                "nf_key": hex::encode(k_nf),
                "nf_siblings": hl(&nf_siblings),
                "amount": amount.to_string(),
            },
            "prev_commit": hex::encode(crypto::state::state_commit_fields(&prev)),
            "new_commit": hex::encode(crypto::state::state_commit_fields(&new)),
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
