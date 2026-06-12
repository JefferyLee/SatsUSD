//! JSON form of `RailManifest` — the transport-layer representation
//! (LP daemon, CLI, agent SDK). The shape is pinned by the
//! cross-language vectors (`rail_vectors` emits through `to_value`),
//! so any drift fails `make verify`.

use serde_json::{json, Value};

use crate::manifest::{
    AutomationTier, Direction, EventTemplate, FeeSchedule, LivenessEscape, LpStake, OracleSpec,
    RailManifest, SettlePrimitive, Transition,
};

fn hex32(b: &[u8; 32]) -> String {
    hex::encode(b)
}

fn parse32(v: &Value, what: &str) -> Result<[u8; 32], String> {
    let s = v.as_str().ok_or_else(|| format!("{what}: not a string"))?;
    let b = hex::decode(s).map_err(|_| format!("{what}: not hex"))?;
    b.try_into().map_err(|_| format!("{what}: not 32 bytes"))
}

fn parse_u64(v: &Value, what: &str) -> Result<u64, String> {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("{what}: expected decimal string"))
}

pub fn to_value(m: &RailManifest) -> Value {
    json!({
        "spec_version": m.spec_version,
        "direction": m.direction as u8,
        "asset_id": hex32(&m.asset_id),
        "oracle_spec": match &m.oracle_spec {
            OracleSpec::None => json!({ "variant": 0 }),
            OracleSpec::Dlcspecs { k, n, oracle_pubkeys, event_template } => json!({
                "variant": 1, "k": k, "n": n,
                "oracle_pubkeys": oracle_pubkeys.iter().map(hex32).collect::<Vec<_>>(),
                "event_template": {
                    "pair": event_template.pair, "cadence_s": event_template.cadence_s,
                    "base": event_template.base, "nb_digits": event_template.nb_digits,
                    "is_signed": event_template.is_signed, "precision": event_template.precision,
                    "unit": event_template.unit,
                },
            }),
            OracleSpec::Optimistic { window_blocks, bond_sats } => json!({
                "variant": 2, "window_blocks": window_blocks, "bond_sats": bond_sats.to_string(),
            }),
            OracleSpec::InternalTwap { window_blocks, min_volume_usd } => json!({
                "variant": 3, "window_blocks": window_blocks,
                "min_volume_usd": min_volume_usd.to_string(),
            }),
        },
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
        "liveness_escape": match &m.liveness_escape {
            LivenessEscape::None => json!({ "variant": 0 }),
            LivenessEscape::Tlock { beacon, delay_s } => json!({
                "variant": 1, "beacon": hex32(beacon), "delay_s": delay_s,
            }),
            LivenessEscape::DecayScript { params } => json!({
                "variant": 2, "params": hex::encode(params),
            }),
        },
        "automation_tiers": m.automation_tiers.iter()
            .map(|(t, tier)| json!([*t as u8, *tier as u8]))
            .collect::<Vec<_>>(),
    })
}

pub fn from_value(v: &Value) -> Result<RailManifest, String> {
    let os = &v["oracle_spec"];
    let oracle_spec = match os["variant"].as_u64() {
        Some(0) => OracleSpec::None,
        Some(1) => OracleSpec::Dlcspecs {
            k: os["k"].as_u64().ok_or("k")? as u16,
            n: os["n"].as_u64().ok_or("n")? as u16,
            oracle_pubkeys: os["oracle_pubkeys"]
                .as_array()
                .ok_or("oracle_pubkeys")?
                .iter()
                .map(|p| parse32(p, "oracle_pubkey"))
                .collect::<Result<_, _>>()?,
            event_template: {
                let t = &os["event_template"];
                EventTemplate {
                    pair: t["pair"].as_str().ok_or("pair")?.into(),
                    cadence_s: t["cadence_s"].as_u64().ok_or("cadence_s")? as u32,
                    base: t["base"].as_u64().ok_or("base")? as u16,
                    nb_digits: t["nb_digits"].as_u64().ok_or("nb_digits")? as u16,
                    is_signed: t["is_signed"].as_bool().ok_or("is_signed")?,
                    precision: t["precision"].as_i64().ok_or("precision")? as i32,
                    unit: t["unit"].as_str().ok_or("unit")?.into(),
                }
            },
        },
        Some(2) => OracleSpec::Optimistic {
            window_blocks: os["window_blocks"].as_u64().ok_or("window_blocks")? as u32,
            bond_sats: parse_u64(&os["bond_sats"], "bond_sats")?,
        },
        Some(3) => OracleSpec::InternalTwap {
            window_blocks: os["window_blocks"].as_u64().ok_or("window_blocks")? as u32,
            min_volume_usd: parse_u64(&os["min_volume_usd"], "min_volume_usd")?,
        },
        _ => return Err("oracle_spec.variant".into()),
    };
    let le = &v["liveness_escape"];
    let liveness_escape = match le["variant"].as_u64() {
        Some(0) => LivenessEscape::None,
        Some(1) => LivenessEscape::Tlock {
            beacon: parse32(&le["beacon"], "beacon")?,
            delay_s: le["delay_s"].as_u64().ok_or("delay_s")? as u32,
        },
        Some(2) => LivenessEscape::DecayScript {
            params: hex::decode(le["params"].as_str().ok_or("params")?)
                .map_err(|_| "params hex")?,
        },
        _ => return Err("liveness_escape.variant".into()),
    };
    let direction = match v["direction"].as_u64() {
        Some(0) => Direction::Redeem,
        Some(1) => Direction::Mint,
        Some(2) => Direction::Both,
        _ => return Err("direction".into()),
    };
    let settle_primitive = match v["settle_primitive"].as_u64() {
        Some(0) => SettlePrimitive::AtomicSwap,
        Some(1) => SettlePrimitive::DlcTaproot,
        Some(2) => SettlePrimitive::OptimisticClaim,
        _ => return Err("settle_primitive".into()),
    };
    let tier = |n: u64| match n {
        1 => Ok(AutomationTier::Consensus),
        2 => Ok(AutomationTier::AnyoneCanAct),
        3 => Ok(AutomationTier::FederationCrypto),
        _ => Err("automation tier".to_string()),
    };
    let transition = |n: u64| match n {
        0 => Ok(Transition::Lock),
        1 => Ok(Transition::Settle),
        2 => Ok(Transition::Refund),
        3 => Ok(Transition::Dispute),
        _ => Err("transition".to_string()),
    };
    let automation_tiers = v["automation_tiers"]
        .as_array()
        .ok_or("automation_tiers")?
        .iter()
        .map(|pair| {
            let a = pair.as_array().ok_or("tier pair")?;
            Ok((
                transition(a[0].as_u64().ok_or("tier key")?)?,
                tier(a[1].as_u64().ok_or("tier value")?)?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(RailManifest {
        spec_version: v["spec_version"].as_u64().ok_or("spec_version")? as u16,
        direction,
        asset_id: parse32(&v["asset_id"], "asset_id")?,
        oracle_spec,
        settle_primitive,
        max_size_sats: parse_u64(&v["max_size_sats"], "max_size_sats")?,
        fee_schedule: FeeSchedule {
            retain_bps: v["fee_schedule"]["retain_bps"].as_u64().ok_or("retain")? as u16,
            service_bps: v["fee_schedule"]["service_bps"].as_u64().ok_or("service")? as u16,
            fixed_sats: parse_u64(&v["fee_schedule"]["fixed_sats"], "fixed_sats")?,
        },
        price_dev_bound_bps: v["price_dev_bound_bps"].as_u64().ok_or("dev_bound")? as u16,
        quote_validity_s: v["quote_validity_s"].as_u64().ok_or("validity")? as u32,
        settle_window_s: v["settle_window_s"].as_u64().ok_or("settle_window")? as u32,
        refund_delta_blocks: v["refund_delta_blocks"].as_u64().ok_or("refund_delta")? as u32,
        lp_stake: LpStake {
            amount_sats: parse_u64(&v["lp_stake"]["amount_sats"], "stake")?,
            slash_conditions: v["lp_stake"]["slash_conditions"]
                .as_array()
                .ok_or("slash_conditions")?
                .iter()
                .map(|c| parse32(c, "slash_condition"))
                .collect::<Result<_, _>>()?,
        },
        dispute_hook: parse32(&v["dispute_hook"], "dispute_hook")?,
        liveness_escape,
        automation_tiers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SPEC_VERSION;

    #[test]
    fn roundtrip_preserves_rail_id() {
        let m = RailManifest {
            spec_version: SPEC_VERSION,
            direction: Direction::Redeem,
            asset_id: [0x11; 32],
            oracle_spec: OracleSpec::Dlcspecs {
                k: 1,
                n: 1,
                oracle_pubkeys: vec![[0x33; 32]],
                event_template: EventTemplate::btcusd_default(),
            },
            settle_primitive: SettlePrimitive::DlcTaproot,
            max_size_sats: 2_000_000,
            fee_schedule: FeeSchedule {
                retain_bps: 10,
                service_bps: 30,
                fixed_sats: 500,
            },
            price_dev_bound_bps: 100,
            quote_validity_s: 5,
            settle_window_s: 60,
            refund_delta_blocks: 288,
            lp_stake: LpStake {
                amount_sats: 10_000_000,
                slash_conditions: vec![[0x22; 32]],
            },
            dispute_hook: [0x44; 32],
            liveness_escape: LivenessEscape::Tlock {
                beacon: [0x55; 32],
                delay_s: 600,
            },
            automation_tiers: vec![
                (Transition::Lock, AutomationTier::AnyoneCanAct),
                (Transition::Refund, AutomationTier::Consensus),
            ],
        };
        let back = from_value(&to_value(&m)).unwrap();
        assert_eq!(m, back);
        assert_eq!(m.rail_id(), back.rail_id());
    }
}
