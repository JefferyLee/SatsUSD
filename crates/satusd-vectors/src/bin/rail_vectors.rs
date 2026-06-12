//! Generates cross-language test vectors for the rebuilt (MISSION v2)
//! protocol surface: spec 00 §3 encodings, RailManifest/rail_id,
//! Quote, SwapPlan, capacity, allotment plans, tapd burn keys, the
//! dlcspecs oracle (announcement/attestation TLVs), CET buckets, the
//! internal_twap marker, and the Rail-1 funding output.
//!
//! Run: `cargo run -p satusd-vectors --bin rail-vectors`.
//! Output: integration/vectors/rail-vectors.json — consumed by
//! `ts/src/verify-rail.ts` (NFR-3: Rust = TS byte-equality is a
//! merge gate).

#[path = "../det.rs"]
mod det;

use std::path::PathBuf;

use det::Det;
use satusd_allot::plan::{AllotmentInput, AllotmentPlan, RailPosition};
use satusd_oracle::event;
use satusd_oracle::oracle::Oracle;
use satusd_rail::asset_meta::AssetMeta;
use satusd_rail::capacity::capacity_sats;
use satusd_rail::encode::{tagged_hash, Encoder};
use satusd_rail::manifest::{
    AutomationTier, Direction, EventTemplate, FeeSchedule, LivenessEscape, LpStake, OracleSpec,
    RailManifest, SettlePrimitive, Transition, SPEC_VERSION,
};
use satusd_rail::quote::Quote;
use satusd_rail0::burn_key::{derive_burn_key, PrevId, TAPD_NUMS_X};
use satusd_rail0::plan::SwapPlan;
use satusd_rail1::cet::{bucket_adaptor_point, bucket_of, bucket_schedule, bucket_secret};
use satusd_rail1::funding::{funding_output, refund_leaf_script, sibling_preimage};
use satusd_verify::marker::{internal_twap, SettlementPoint};
use serde_json::{json, Value};

fn hex32(b: &[u8; 32]) -> String {
    hex::encode(b)
}

// ---- deterministic fixture builders ----

fn gen_event_template(d: &mut Det, vary: bool) -> EventTemplate {
    if !vary {
        return EventTemplate::btcusd_default();
    }
    let pairs = ["BTCUSD", "BTCEUR", "X"];
    let units = ["USD/BTC", "EUR/BTC", ""];
    EventTemplate {
        pair: pairs[d.count(2)].to_string(),
        cadence_s: d.u32(),
        base: 2 + d.u16() % 9,
        nb_digits: 1 + d.u16() % 30,
        is_signed: d.u8() & 1 == 1,
        precision: d.u32() as i32,
        unit: units[d.count(2)].to_string(),
    }
}

fn gen_oracle_spec(d: &mut Det, variant: usize) -> OracleSpec {
    match variant {
        0 => OracleSpec::None,
        1 => {
            let n = 1 + d.u16() % 3;
            let k = 1 + d.u16() % n;
            let n_keys = 1 + d.count(2);
            let oracle_pubkeys = (0..n_keys).map(|_| d.arr()).collect();
            let vary = d.u8() & 1 == 1;
            OracleSpec::Dlcspecs {
                k,
                n,
                oracle_pubkeys,
                event_template: gen_event_template(d, vary),
            }
        }
        2 => OracleSpec::Optimistic {
            window_blocks: d.u32(),
            bond_sats: d.u64(),
        },
        _ => OracleSpec::InternalTwap {
            window_blocks: d.u32(),
            min_volume_usd: d.u64(),
        },
    }
}

fn gen_liveness(d: &mut Det, variant: usize) -> LivenessEscape {
    match variant {
        0 => LivenessEscape::None,
        1 => LivenessEscape::Tlock {
            beacon: d.arr(),
            delay_s: d.u32(),
        },
        _ => {
            let n = d.count(40);
            LivenessEscape::DecayScript { params: d.bytes(n) }
        }
    }
}

fn gen_manifest(d: &mut Det, i: usize) -> RailManifest {
    let transitions = [
        Transition::Lock,
        Transition::Settle,
        Transition::Refund,
        Transition::Dispute,
    ];
    let tiers = [
        AutomationTier::Consensus,
        AutomationTier::AnyoneCanAct,
        AutomationTier::FederationCrypto,
    ];
    let mut automation: Vec<(Transition, AutomationTier)> = Vec::new();
    for t in transitions {
        if d.u8() & 1 == 1 {
            automation.push((t, tiers[d.count(2)]));
        }
    }
    // Shuffle-ish: emission order must not matter (canonical sort).
    if d.u8() & 1 == 1 {
        automation.reverse();
    }
    RailManifest {
        spec_version: if i.is_multiple_of(7) {
            d.u16()
        } else {
            SPEC_VERSION
        },
        direction: match i % 3 {
            0 => Direction::Redeem,
            1 => Direction::Mint,
            _ => Direction::Both,
        },
        asset_id: d.arr(),
        oracle_spec: gen_oracle_spec(d, i % 4),
        settle_primitive: match i % 3 {
            0 => SettlePrimitive::AtomicSwap,
            1 => SettlePrimitive::DlcTaproot,
            _ => SettlePrimitive::OptimisticClaim,
        },
        max_size_sats: d.u64(),
        fee_schedule: FeeSchedule {
            retain_bps: d.u16() % 1000,
            service_bps: d.u16() % 1000,
            fixed_sats: u64::from(d.u16()),
        },
        price_dev_bound_bps: d.u16(),
        quote_validity_s: d.u32(),
        settle_window_s: d.u32(),
        refund_delta_blocks: d.u32(),
        lp_stake: LpStake {
            amount_sats: d.u64(),
            slash_conditions: (0..d.count(3)).map(|_| d.arr()).collect(),
        },
        dispute_hook: if d.u8() & 1 == 0 { [0u8; 32] } else { d.arr() },
        liveness_escape: gen_liveness(d, i % 3),
        automation_tiers: automation,
    }
}

// ---- JSON field emission (TS rebuilds the structs from these) ----

fn event_template_value(t: &EventTemplate) -> Value {
    json!({
        "pair": t.pair,
        "cadence_s": t.cadence_s,
        "base": t.base,
        "nb_digits": t.nb_digits,
        "is_signed": t.is_signed,
        "precision": t.precision,
        "unit": t.unit,
    })
}

fn oracle_spec_value(o: &OracleSpec) -> Value {
    match o {
        OracleSpec::None => json!({ "variant": 0 }),
        OracleSpec::Dlcspecs {
            k,
            n,
            oracle_pubkeys,
            event_template,
        } => json!({
            "variant": 1,
            "k": k,
            "n": n,
            "oracle_pubkeys": oracle_pubkeys.iter().map(hex32).collect::<Vec<_>>(),
            "event_template": event_template_value(event_template),
        }),
        OracleSpec::Optimistic {
            window_blocks,
            bond_sats,
        } => json!({
            "variant": 2,
            "window_blocks": window_blocks,
            "bond_sats": bond_sats.to_string(),
        }),
        OracleSpec::InternalTwap {
            window_blocks,
            min_volume_usd,
        } => json!({
            "variant": 3,
            "window_blocks": window_blocks,
            "min_volume_usd": min_volume_usd.to_string(),
        }),
    }
}

fn liveness_value(l: &LivenessEscape) -> Value {
    match l {
        LivenessEscape::None => json!({ "variant": 0 }),
        LivenessEscape::Tlock { beacon, delay_s } => json!({
            "variant": 1, "beacon": hex32(beacon), "delay_s": delay_s,
        }),
        LivenessEscape::DecayScript { params } => json!({
            "variant": 2, "params": hex::encode(params),
        }),
    }
}

fn manifest_value(m: &RailManifest) -> Value {
    json!({
        "spec_version": m.spec_version,
        "direction": m.direction as u8,
        "asset_id": hex32(&m.asset_id),
        "oracle_spec": oracle_spec_value(&m.oracle_spec),
        "settle_primitive": m.settle_primitive as u8,
        "max_size_sats": m.max_size_sats.to_string(),
        "fee_schedule": {
            "retain_bps": m.fee_schedule.retain_bps,
            "service_bps": m.fee_schedule.service_bps,
            "fixed_sats": m.fee_schedule.fixed_sats.to_string(),
        },
        "price_dev_bound_bps": m.price_dev_bound_bps,
        "quote_validity_s": m.quote_validity_s,
        "settle_window_s": m.settle_window_s,
        "refund_delta_blocks": m.refund_delta_blocks,
        "lp_stake": {
            "amount_sats": m.lp_stake.amount_sats.to_string(),
            "slash_conditions": m.lp_stake.slash_conditions.iter().map(hex32).collect::<Vec<_>>(),
        },
        "dispute_hook": hex32(&m.dispute_hook),
        "liveness_escape": liveness_value(&m.liveness_escape),
        // Emission order (possibly unsorted) — TS must sort by
        // transition discriminant before encoding.
        "automation_tiers": m.automation_tiers.iter()
            .map(|(t, tier)| json!([*t as u8, *tier as u8]))
            .collect::<Vec<_>>(),
    })
}

fn quote_value(q: &Quote) -> Value {
    json!({
        "rail_id": hex32(&q.rail_id),
        "price_msat_per_usd": q.price_msat_per_usd.to_string(),
        "amount_micro_usd": q.amount_micro_usd.to_string(),
        "expiry_unix_s": q.expiry_unix_s.to_string(),
        "lock_template_commitment": hex32(&q.lock_template_commitment),
    })
}

fn main() {
    let mut vectors: Vec<Value> = Vec::new();
    let mut d = Det::new(0x5a71); // independent stream from main.rs

    // ---- tagged_hash (spec 00 §3.2) ----
    for (i, (tag, msg)) in [
        ("SatUSD/rail-manifest/v1", b"".to_vec()),
        ("SatUSD/quote/v1", b"abc".to_vec()),
        ("SatUSD/oracle/nonce/v1", d.bytes(64)),
        ("DLC/oracle/announcement/v0", d.bytes(7)),
        ("DLC/oracle/attestation/v0", b"0".to_vec()),
        ("BIP0340/challenge", d.bytes(96)),
        ("TapTweak", d.bytes(100)),
        ("SatUSD/vector", d.bytes(33)),
    ]
    .iter()
    .enumerate()
    {
        vectors.push(json!({
            "name": format!("tagged_hash_{i}"),
            "kind": "tagged_hash",
            "inputs": { "tag": tag, "msg": hex::encode(msg) },
            "output": hex::encode(tagged_hash(tag, msg)),
        }));
    }

    // ---- BigSize boundaries (spec 00 §3.4) ----
    for (i, v) in [
        0u64,
        1,
        0xfc,
        0xfd,
        0xffff,
        0x1_0000,
        0xffff_ffff,
        0x1_0000_0000,
        u64::MAX,
    ]
    .iter()
    .enumerate()
    {
        let mut e = Encoder::new();
        e.put_bigsize(*v);
        vectors.push(json!({
            "name": format!("bigsize_{i}"),
            "kind": "bigsize",
            "inputs": { "value": v.to_string() },
            "output": hex::encode(e.into_bytes()),
        }));
    }

    // ---- RailManifest / rail_id (spec 02 §2) ----
    // The two implementation-pinned fixtures first, then generated
    // coverage across every enum branch.
    let rail0 = satusd_rail0::rail::rail0_manifest(satusd_rail0::rail::Rail0Params {
        asset_id: [0x11; 32],
        max_size_sats: 5_000_000,
        fee_schedule: FeeSchedule {
            retain_bps: 10,
            service_bps: 5,
            fixed_sats: 200,
        },
        price_dev_bound_bps: 100,
        quote_validity_s: 30,
        lp_stake: LpStake {
            amount_sats: 10_000_000,
            slash_conditions: vec![[0x22; 32]],
        },
    })
    .expect("rail0 fixture");
    let mut named: Vec<(String, RailManifest)> = vec![("rail_manifest_rail0".into(), rail0)];
    for i in 0..24 {
        named.push((format!("rail_manifest_{i}"), gen_manifest(&mut d, i)));
    }
    for (name, m) in &named {
        vectors.push(json!({
            "name": name,
            "kind": "rail_manifest",
            "fields": manifest_value(m),
            "encoding_hex": hex::encode(m.canonical_encode()),
            "rail_id": hex32(&m.rail_id()),
        }));
    }

    // ---- AssetMeta (spec 01 §2.2) ----
    {
        let mut metas = vec![(
            "asset_meta_genesis_11".to_string(),
            AssetMeta::genesis([0x11; 32]),
        )];
        for i in 0..3 {
            metas.push((
                format!("asset_meta_gen_{i}"),
                AssetMeta {
                    spec_version: d.u16(),
                    name: ["SatUSD", "X", "Sat\u{4e2d}USD"][i].to_string(),
                    decimals: d.u8(),
                    mission_commitment: d.arr(),
                },
            ));
        }
        for (name, m) in &metas {
            vectors.push(json!({
                "name": name,
                "kind": "asset_meta",
                "fields": {
                    "spec_version": m.spec_version,
                    "name": m.name,
                    "decimals": m.decimals,
                    "mission_commitment": hex32(&m.mission_commitment),
                },
                "encoding_hex": hex::encode(m.canonical_encode()),
            }));
        }
    }

    // ---- Quote (spec 02 §3.1) ----
    let mut quotes: Vec<Quote> = (0..10)
        .map(|_| Quote {
            rail_id: d.arr(),
            price_msat_per_usd: 100_000 + d.u64() % 10_000_000,
            amount_micro_usd: d.u64() % 100_000_000_000_000,
            expiry_unix_s: d.u64() % 4_000_000_000,
            lock_template_commitment: d.arr(),
        })
        .collect();
    // Saturation edge: price × amount overflows u64 sats → MAX.
    quotes.push(Quote {
        rail_id: [0xee; 32],
        price_msat_per_usd: u64::MAX,
        amount_micro_usd: u64::MAX,
        expiry_unix_s: u64::MAX,
        lock_template_commitment: [0xee; 32],
    });
    // Dust edge: 1 µUSD floors to 0 sats.
    quotes.push(Quote {
        rail_id: [0u8; 32],
        price_msat_per_usd: 1_000_000,
        amount_micro_usd: 1,
        expiry_unix_s: 0,
        lock_template_commitment: [0u8; 32],
    });
    for (i, q) in quotes.iter().enumerate() {
        vectors.push(json!({
            "name": format!("quote_{i}"),
            "kind": "quote",
            "fields": quote_value(q),
            "encoding_hex": hex::encode(q.canonical_encode()),
            "signing_payload": hex32(&q.signing_payload()),
            "user_sats": q.user_sats().to_string(),
        }));
    }

    // ---- SwapPlan (Rail-0 fee math + lock template commitment) ----
    {
        let manifest = |retain: u16, service: u16, fixed: u64, max: u64| {
            satusd_rail0::rail::rail0_manifest(satusd_rail0::rail::Rail0Params {
                asset_id: [0x11; 32],
                max_size_sats: max,
                fee_schedule: FeeSchedule {
                    retain_bps: retain,
                    service_bps: service,
                    fixed_sats: fixed,
                },
                price_dev_bound_bps: 100,
                quote_validity_s: 30,
                lp_stake: LpStake {
                    amount_sats: 10_000_000,
                    slash_conditions: vec![],
                },
            })
            .expect("rail0 manifest")
        };
        let quote = |m: &RailManifest, price: u64, amount: u64| Quote {
            rail_id: m.rail_id(),
            price_msat_per_usd: price,
            amount_micro_usd: amount,
            expiry_unix_s: 1_700_000_000,
            lock_template_commitment: [0; 32],
        };
        // (manifest, price, amount) cases: worked example, rounding
        // edges (half-up), oversize, dust, big-but-fine.
        let cases: Vec<(RailManifest, u64, u64, &str)> = vec![
            (
                manifest(10, 5, 200, 5_000_000),
                1_000_000,
                1_000_000_000,
                "worked_example",
            ),
            (
                manifest(10, 5, 0, 5_000_000),
                1_000_000,
                9_999_000,
                "round_half_up_low",
            ),
            (
                manifest(10, 5, 0, 5_000_000),
                1_000_000,
                11_000_000,
                "round_half_up_high",
            ),
            (
                manifest(0, 0, 0, u64::MAX),
                6_500_000,
                77_777_777,
                "zero_fees",
            ),
            (
                manifest(999, 999, 65_535, u64::MAX),
                9_999_999,
                99_999_999_999,
                "max_bps",
            ),
            (
                manifest(10, 5, 200, 5_000_000),
                1_000_000,
                10_000_000_000,
                "over_max_size",
            ),
            (
                manifest(10, 5, 200, 5_000_000),
                1_000_000,
                100_000,
                "dust_payout",
            ),
            (
                manifest(10, 5, 200, 5_000_000),
                1_000_000,
                700_000,
                "exact_dust_boundary",
            ),
        ];
        for (i, (m, price, amount, label)) in cases.iter().enumerate() {
            let q = quote(m, *price, *amount);
            let result = match SwapPlan::from_quote(m, &q) {
                Ok(p) => json!({
                    "burn_micro_usd": p.burn_micro_usd.to_string(),
                    "gross_sats": p.gross_sats.to_string(),
                    "service_fee_sats": p.service_fee_sats.to_string(),
                    "user_sats": p.user_sats.to_string(),
                    "retained_sats": p.retained_sats.to_string(),
                    "lock_template_commitment": hex32(&p.lock_template_commitment()),
                }),
                Err(e) => json!({ "error": format!("{e:?}") }),
            };
            vectors.push(json!({
                "name": format!("swap_plan_{i}_{label}"),
                "kind": "swap_plan",
                "inputs": { "manifest": manifest_value(m), "quote": quote_value(&q) },
                "result": result,
            }));
        }
    }

    // ---- capacity (spec 02 §6.1, α = 1/2) ----
    for (i, (fees, stake)) in [
        (0u64, 0u64),
        (0, 1),
        (1, 0),
        (0, 10_000_000),
        (123_456, 10_000_000),
        (u64::MAX, 0),
        (u64::MAX, u64::MAX),
    ]
    .iter()
    .enumerate()
    {
        vectors.push(json!({
            "name": format!("capacity_{i}"),
            "kind": "capacity",
            "inputs": {
                "retained_fees_cum_sats": fees.to_string(),
                "lp_stake_sats": stake.to_string(),
            },
            "output": capacity_sats(*fees, *stake).to_string(),
        }));
    }

    // ---- allotment plans (spec 04 §2, FR-6) ----
    {
        let pos = |id: [u8; 32], fees: u64, stake: u64| RailPosition {
            rail_id: id,
            retained_fees_cum_sats: fees,
            lp_stake_sats: stake,
        };
        let mut cases: Vec<(String, AllotmentInput)> = vec![
            (
                "allotment_pinned_42".into(),
                AllotmentInput {
                    epoch: 42,
                    reserve_total_sats: 50_000_000,
                    positions: vec![
                        pos([0x11; 32], 123_456, 10_000_000),
                        pos([0x22; 32], 0, 2_000_000),
                    ],
                },
            ),
            (
                "allotment_oversubscribed".into(),
                AllotmentInput {
                    epoch: 1,
                    reserve_total_sats: 3_000_000,
                    positions: vec![pos([0x01; 32], 0, 8_000_000), pos([0x02; 32], 0, 4_000_000)],
                },
            ),
            (
                "allotment_unsorted_input".into(),
                AllotmentInput {
                    epoch: 7,
                    reserve_total_sats: 100_000_000,
                    positions: vec![
                        pos([0x02; 32], 1_000, 10_000_000),
                        pos([0x01; 32], 0, 4_000_000),
                    ],
                },
            ),
            (
                "allotment_empty".into(),
                AllotmentInput {
                    epoch: 9,
                    reserve_total_sats: 1_000_000,
                    positions: vec![],
                },
            ),
            (
                "allotment_zero_positions".into(),
                AllotmentInput {
                    epoch: 10,
                    reserve_total_sats: 1_000_000,
                    positions: vec![pos([0xaa; 32], 0, 0)],
                },
            ),
        ];
        for i in 0..6 {
            let n = 1 + d.count(15);
            let positions = (0..n)
                .map(|_| pos(d.arr(), d.u64() % 1_000_000_000, d.u64() % 100_000_000_000))
                .collect();
            cases.push((
                format!("allotment_gen_{i}"),
                AllotmentInput {
                    epoch: d.u32(),
                    reserve_total_sats: d.u64() % 10_000_000_000_000,
                    positions,
                },
            ));
        }
        for (name, input) in &cases {
            let plan = AllotmentPlan::compute(input).expect("no duplicate ids in fixtures");
            vectors.push(json!({
                "name": name,
                "kind": "allotment",
                "inputs": {
                    "epoch": input.epoch,
                    "reserve_total_sats": input.reserve_total_sats.to_string(),
                    "positions": input.positions.iter().map(|p| json!({
                        "rail_id": hex32(&p.rail_id),
                        "retained_fees_cum_sats": p.retained_fees_cum_sats.to_string(),
                        "lp_stake_sats": p.lp_stake_sats.to_string(),
                    })).collect::<Vec<_>>(),
                },
                "input_hash": hex32(&plan.input_hash),
                "encoding_hex": hex::encode(plan.canonical_encode()),
                "plan_hash": hex32(&plan.plan_hash()),
                "unallocated_sats": plan.unallocated_sats.to_string(),
            }));
        }
    }

    // ---- tapd burn key (spec 01 §4) ----
    {
        let mut prevs: Vec<PrevId> = vec![PrevId {
            // The unit-test pinned fixture.
            txid_wire: [0xaa; 32],
            vout: 1,
            asset_id: [0xbb; 32],
            script_key_xonly: TAPD_NUMS_X,
        }];
        for _ in 0..7 {
            prevs.push(PrevId {
                txid_wire: d.arr(),
                vout: d.u32(),
                asset_id: d.arr(),
                script_key_xonly: d.arr(),
            });
        }
        for (i, p) in prevs.iter().enumerate() {
            vectors.push(json!({
                "name": format!("burn_key_{i}"),
                "kind": "burn_key",
                "inputs": {
                    "txid_wire": hex32(&p.txid_wire),
                    "vout": p.vout,
                    "asset_id": hex32(&p.asset_id),
                    "script_key_xonly": hex32(&p.script_key_xonly),
                },
                "output": hex32(&derive_burn_key(p)),
            }));
        }
    }

    // ---- oracle: digits, nonces, attestation messages (spec 03 §3) ----
    for (i, price) in [0u32, 1, 60_123, 1_048_575, u32::MAX].iter().enumerate() {
        let ds = event::digits(*price);
        vectors.push(json!({
            "name": format!("oracle_digits_{i}"),
            "kind": "oracle_digits",
            "inputs": { "price_usd": price },
            "digits": ds.to_vec(),
            "recomposed": event::price_from_digits(&ds),
        }));
    }
    for (i, (seed, ts, idx)) in [
        ([7u8; 32], 1_700_000_000u64, 0u16),
        ([7u8; 32], 1_700_000_000, 19),
        ([9u8; 32], 1_800_000_000, 7),
    ]
    .iter()
    .enumerate()
    {
        vectors.push(json!({
            "name": format!("oracle_nonce_{i}"),
            "kind": "oracle_nonce",
            "inputs": {
                "seed": hex32(seed),
                "event_id": event::event_id(*ts),
                "digit_index": idx,
            },
            "output": hex32(&event::nonce(seed, &event::event_id(*ts), *idx)),
        }));
    }
    for digit in [0u8, 1] {
        vectors.push(json!({
            "name": format!("attestation_msg_{digit}"),
            "kind": "attestation_msg",
            "inputs": { "digit": digit },
            "output": hex32(&event::attestation_msg(digit)),
        }));
    }

    // ---- oracle announcement + attestation TLVs (FR-5) ----
    // TS recomputes BOTH full TLV byte strings from (seed, ts, price)
    // — transitively pinning seed→key, nonce derivation, BIP-340
    // signing with a predetermined nonce, and the dlcspecs wire
    // layout. TS additionally asserts S = R + e·P = s·G per digit
    // (the DLC anticipation contract).
    for (i, (seed, ts, price)) in [
        ([42u8; 32], 1_700_000_000u64, 60_123u32), // the unit-test fixture
        ([1u8; 32], 1_750_000_000, 0),
        ([2u8; 32], 1_800_000_000, 1_048_575),
        ([3u8; 32], 1_900_000_000, u32::MAX), // clamps to max outcome
    ]
    .iter()
    .enumerate()
    {
        let o = Oracle::from_seed(seed).expect("seed");
        let ann = o.announce(*ts).expect("announce");
        let att = o.attest(*ts, *price).expect("attest");
        vectors.push(json!({
            "name": format!("oracle_pair_{i}"),
            "kind": "oracle_pair",
            "inputs": {
                "master_seed": hex32(seed),
                "unix_ts": ts.to_string(),
                "price_usd": price,
            },
            "pubkey": hex32(&o.pubkey),
            "announcement_tlv_hex": hex::encode(&ann.tlv_bytes),
            "attestation_tlv_hex": hex::encode(&att.tlv_bytes),
        }));
    }

    // ---- CET buckets (Rail-1) ----
    for (i, (price, m)) in [
        (0u32, 4u8),
        (65_535, 4),
        (65_536, 4),
        (u32::MAX, 4),
        (60_123, 1),
        (60_123, 20),
    ]
    .iter()
    .enumerate()
    {
        vectors.push(json!({
            "name": format!("bucket_of_{i}"),
            "kind": "bucket_of",
            "inputs": { "price_usd": price, "m": m },
            "output": bucket_of(*price, *m),
        }));
    }
    {
        let m = 4u8;
        let buckets = bucket_schedule(m, |mid| u64::from(mid) * 1_000);
        vectors.push(json!({
            "name": "bucket_schedule_m4",
            "kind": "bucket_schedule",
            "inputs": { "m": m, "user_sats_factor": "1000" },
            "buckets": buckets.iter().map(|b| json!([
                b.index, b.price_lo, b.price_hi, b.user_sats.to_string()
            ])).collect::<Vec<_>>(),
        }));
    }
    for (i, (seed, ts, price)) in [
        ([9u8; 32], 1_700_000_100u64, 60_123u32),
        ([11u8; 32], 1_700_000_200, 98_765),
    ]
    .iter()
    .enumerate()
    {
        let m = 4u8;
        let o = Oracle::from_seed(seed).expect("seed");
        let ann = o.announce(*ts).expect("announce");
        let att = o.attest(*ts, *price).expect("attest");
        let win = bucket_of(*price, m);
        let point = bucket_adaptor_point(&ann, &o.pubkey, m, win).expect("point");
        let other = bucket_adaptor_point(&ann, &o.pubkey, m, (win + 1) % (1 << m)).expect("point");
        let t = bucket_secret(&att, m, win).expect("secret");
        vectors.push(json!({
            "name": format!("bucket_point_{i}"),
            "kind": "bucket_point",
            "inputs": {
                "master_seed": hex32(seed),
                "unix_ts": ts.to_string(),
                "price_usd": price,
                "m": m,
            },
            "winning_bucket": win,
            "adaptor_point": hex::encode(point.serialize()),
            "other_bucket_point": hex::encode(other.serialize()),
            "bucket_secret": hex32(&t),
        }));
    }

    // ---- internal_twap marker (spec 03 §5.2) ----
    {
        let pt = |price: u64, vol: u64| SettlementPoint {
            price_msat_per_usd: price,
            volume_micro_usd: vol,
        };
        let mut cases: Vec<(String, Vec<SettlementPoint>, u64)> = vec![
            (
                "twap_whale_volume".into(),
                vec![pt(990_000, 1), pt(1_000_000, 100), pt(1_010_000, 1)],
                0,
            ),
            (
                "twap_tie_lower".into(),
                vec![pt(999_000, 50), pt(1_001_000, 50)],
                0,
            ),
            (
                "twap_outlier_trimmed".into(),
                vec![
                    pt(1_000_000, 30),
                    pt(1_001_000, 30),
                    pt(999_000, 30),
                    pt(1_400_000, 10_000),
                ],
                0,
            ),
            ("twap_below_floor".into(), vec![pt(1_000_000, 5)], 10),
            ("twap_at_floor".into(), vec![pt(1_000_000, 5)], 5),
            ("twap_empty".into(), vec![], 0),
            (
                "twap_band_edge".into(),
                vec![pt(1_000_000, 10), pt(1_050_000, 1), pt(1_050_001, 1)],
                0,
            ),
        ];
        for i in 0..5 {
            let n = 1 + d.count(20);
            let points = (0..n)
                .map(|_| pt(900_000 + d.u64() % 200_000, 1 + d.u64() % 1_000_000))
                .collect();
            cases.push((format!("twap_gen_{i}"), points, d.u64() % 2_000_000));
        }
        for (name, points, min_vol) in &cases {
            vectors.push(json!({
                "name": name,
                "kind": "internal_twap",
                "inputs": {
                    "points": points.iter().map(|p| json!([
                        p.price_msat_per_usd.to_string(),
                        p.volume_micro_usd.to_string(),
                    ])).collect::<Vec<_>>(),
                    "min_volume_micro_usd": min_vol.to_string(),
                },
                "output": internal_twap(points, *min_vol).map(|v| v.to_string()),
            }));
        }
    }

    // ---- Rail-1 funding output (proposal 0001 §3.1) ----
    // refund_script is an INPUT (script assembly is rust-bitcoin's
    // job); TS recomputes TapLeaf/TapBranch/TapTweak → merkle root +
    // output key, and the tapd sibling preimage.
    for i in 0..4u32 {
        // A valid x-only internal key, via k·G.
        let internal_x = satusd_oracle::schnorr::nonce_point(&d.arr())
            .expect("det scalar valid")
            .serialize();
        let ta_leaf: [u8; 32] = d.arr();
        let csv = 100 + (i as u16) * 1000;
        let user_x = satusd_oracle::schnorr::nonce_point(&d.arr())
            .expect("k")
            .serialize();
        let lp_x = satusd_oracle::schnorr::nonce_point(&d.arr())
            .expect("k")
            .serialize();
        let script = refund_leaf_script(csv, &user_x, &lp_x);
        let f = funding_output(&internal_x, &ta_leaf, &script);
        vectors.push(json!({
            "name": format!("funding_output_{i}"),
            "kind": "funding_output",
            "inputs": {
                "internal_x": hex32(&internal_x),
                "ta_leaf_hash": hex32(&ta_leaf),
                "refund_script_hex": hex::encode(script.as_bytes()),
                // Documentation only (the script already encodes them):
                "csv_delta": csv, "user_x": hex32(&user_x), "lp_x": hex32(&lp_x),
            },
            "sibling_preimage_hex": hex::encode(sibling_preimage(&script)),
            "merkle_root": hex32(&f.merkle_root),
            "output_x": hex32(&f.output_x),
        }));
    }

    let doc = json!({
        "schema": "satusd-rail-vectors-v1",
        "count": vectors.len(),
        "vectors": vectors,
    });

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integration/vectors");
    std::fs::create_dir_all(&out_dir).expect("create vectors dir");
    let out_path = out_dir.join("rail-vectors.json");
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&doc).expect("serialize"),
    )
    .expect("write rail-vectors.json");
    println!("wrote {} vectors to {}", doc["count"], out_path.display());
}
