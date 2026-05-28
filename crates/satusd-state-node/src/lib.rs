//! SatUSD state node — HTTP service (PRD §7.3) wrapping the `StateNode` engine.
//!
//! Endpoints:
//!   GET  /v1/state                  → current state root + headline scalars
//!   POST /v1/transition/simulate    → dry-run a transition (no commit)
//!   POST /v1/transition/submit      → apply + commit a transition
//!
//! Body is a `{"transition": "...", ...}` tagged JSON object (see `dto`).

pub mod da_mirror;
pub mod dto;

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use dto::TransitionRequest;
use satusd_state::node::{NodeError, StateNode};

type Shared = Arc<Mutex<StateNode>>;

fn apply_to(node: &mut StateNode, req: TransitionRequest) -> Result<[u8; 32], String> {
    let fmt = |e: NodeError| format!("{e:?}");
    match req {
        TransitionRequest::IssuerRegister(d) => {
            node.issuer_register((*d).into_domain()?).map_err(fmt)
        }
        TransitionRequest::OperatorRegister(d) => {
            node.operator_register((*d).into_domain()?).map_err(fmt)
        }
        TransitionRequest::MintCommit(d) => {
            let id = d.issuer_id.0;
            node.mint_commit(id, &(*d).into_witness()).map_err(fmt)
        }
        TransitionRequest::MintFinalize(d) => {
            let id = d.issuer_id.0;
            node.mint_finalize(id, &d.into_witness()).map_err(fmt)
        }
        TransitionRequest::RedeemLock(d) => node.redeem_lock((*d).into_witness()).map_err(fmt),
        TransitionRequest::LockRefund(d) => node.lock_refund((*d).into_witness()).map_err(fmt),
        TransitionRequest::SubmitClaim(d) => {
            let (redemptions, oracle_messages, signer_set, l1, shard, expiry, tip) = d.parts()?;
            node.submit_claim(
                redemptions,
                oracle_messages,
                signer_set,
                l1,
                shard,
                expiry,
                tip,
            )
            .map_err(fmt)
        }
        TransitionRequest::FinalizeClaim(d) => node
            .finalize_claim(
                d.claim_id.0,
                &d.committee(),
                &d.approvals(),
                d.current_height,
            )
            .map_err(fmt),
        TransitionRequest::ReclaimStaleClaim(d) => node
            .reclaim_stale_claim(d.claim_id.0, d.current_height)
            .map_err(fmt),
    }
}

fn outcome(result: Result<[u8; 32], String>) -> (StatusCode, Json<Value>) {
    match result {
        Ok(h) => (
            StatusCode::OK,
            Json(json!({ "new_state_root": hex::encode(h) })),
        ),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))),
    }
}

async fn get_state(State(s): State<Shared>) -> Json<Value> {
    let node = s.lock().unwrap();
    let st = node.state();
    Json(json!({
        "state_root_hash": hex::encode(node.state_root_hash()),
        "state_epoch": st.state_epoch,
        "sat_usd_supply_atoms": st.sat_usd_supply_atoms,
        "reserve_btc_sats": st.reserve_btc_sats,
        "collateral_ratio_ppm": st.collateral_ratio_ppm,
        "emergency_tier": st.emergency_tier,
        "l1_anchor_height": st.l1_anchor_height,
    }))
}

async fn get_pending_claims(State(s): State<Shared>) -> Json<Value> {
    let node = s.lock().unwrap();
    let claims: Vec<Value> = node
        .pending_claims()
        .iter()
        .map(|(id, p)| {
            json!({
                "claim_id": hex::encode(id),
                "operator_id": hex::encode(p.operator_id),
                "reserved_sats": p.reserved_sats,
                "claim_created_height": p.claim_created_height,
                "claim_expiry_height": p.claim_expiry_height,
                "status": p.status as u8,
            })
        })
        .collect();
    Json(json!({ "pending_claims": claims }))
}

async fn submit(
    State(s): State<Shared>,
    Json(req): Json<TransitionRequest>,
) -> (StatusCode, Json<Value>) {
    let mut node = s.lock().unwrap();
    outcome(apply_to(&mut node, req))
}

/// Dry-run against a clone; the live node is untouched.
async fn simulate(
    State(s): State<Shared>,
    Json(req): Json<TransitionRequest>,
) -> (StatusCode, Json<Value>) {
    let mut clone = s.lock().unwrap().clone();
    outcome(apply_to(&mut clone, req))
}

/// Build the HTTP router around a `StateNode`.
pub fn router(node: StateNode) -> Router {
    let shared: Shared = Arc::new(Mutex::new(node));
    Router::new()
        .route("/v1/state", get(get_state))
        .route("/v1/pending_claims", get(get_pending_claims))
        .route("/v1/transition/simulate", post(simulate))
        .route("/v1/transition/submit", post(submit))
        .with_state(shared)
}

/// Serve the router on an already-bound listener (used by the binary and tests).
pub async fn serve(listener: tokio::net::TcpListener, node: StateNode) {
    axum::serve(listener, router(node)).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuer_register_over_dto_path() {
        // The JSON → DTO → node path applies an ISSUER_REGISTER and advances epoch.
        let body = json!({
            "transition": "issuer_register",
            "issuer_id": hex::encode([0xaa; 32]),
            "status": 0,
            "multisig_pubkeys": [hex::encode([0x02; 33]), hex::encode([0x03; 33])],
            "multisig_threshold": 2,
            "reserve_deposits_sats": 0,
            "minted_satusd_atoms": 0,
            "pending_mint_atoms": 0,
            "collateral_ratio_ppm": 0,
            "last_deposit_txid": null,
            "registered_at_height": 100,
            "pending_mint_commitment": null,
        });
        let req: TransitionRequest = serde_json::from_value(body).unwrap();
        let mut node = StateNode::genesis([0x01; 32], [0x02; 32], 0, 5_000_000_000_000);
        apply_to(&mut node, req).expect("issuer register via dto");
        assert_eq!(node.state().state_epoch, 1);
        assert!(node.issuer(&[0xaa; 32]).is_some());
    }

    #[test]
    fn reclaim_dto_path_dispatches() {
        // The reclaim_stale_claim tag deserializes and dispatches; an unknown claim
        // on a fresh node surfaces UnknownClaim (proves the wiring, not a panic).
        let body = json!({
            "transition": "reclaim_stale_claim",
            "claim_id": hex::encode([0x55; 32]),
            "current_height": 840_200,
        });
        let req: TransitionRequest = serde_json::from_value(body).unwrap();
        let mut node = StateNode::genesis([0x01; 32], [0x02; 32], 0, 5_000_000_000_000);
        let err = apply_to(&mut node, req).unwrap_err();
        assert!(err.contains("UnknownClaim"), "{err}");
    }

    #[test]
    fn submit_claim_dto_deserializes() {
        // The (large) submit_claim body deserializes into the tagged variant.
        let body = json!({
            "transition": "submit_claim",
            "redemptions": [],
            "oracle_messages": [],
            "oracle_signer_set": [],
            "l1_anchor": {
                "l1_anchor_height": 840_000,
                "l1_anchor_hash": hex::encode([0x0c; 32]),
                "l1_anchor_mtp": 1_699_996_400u64,
                "l1_anchor_chain_time": 1_700_000_000u64,
                "recent_header_chain": [],
                "oracle_epoch": 7,
                "selected_oracle_price_e8": 5_000_000_000_000u64,
                "max_epoch_lag_sec": 600,
                "oracle_future_tolerance": 300,
            },
            "reserve_shard_id": 0,
            "claim_expiry_height": 840_100,
            "btc_tip_height": 110,
        });
        assert!(matches!(
            serde_json::from_value::<TransitionRequest>(body),
            Ok(TransitionRequest::SubmitClaim(_))
        ));
    }

    #[test]
    fn bad_hex_length_rejected_at_deserialize() {
        let body = json!({
            "transition": "issuer_register",
            "issuer_id": "aabb", // too short for 32 bytes
            "status": 0,
            "multisig_pubkeys": [],
            "multisig_threshold": 1,
            "reserve_deposits_sats": 0,
            "minted_satusd_atoms": 0,
            "pending_mint_atoms": 0,
            "collateral_ratio_ppm": 0,
            "last_deposit_txid": null,
            "registered_at_height": 0,
            "pending_mint_commitment": null,
        });
        assert!(serde_json::from_value::<TransitionRequest>(body).is_err());
    }
}
