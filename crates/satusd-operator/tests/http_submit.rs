//! End-to-end proof that the operator submits transitions over the live HTTP
//! boundary (not the in-process `StateNode` Rust API): boot the real state-node
//! `router` on an ephemeral port, POST an `ISSUER_REGISTER` via the operator's
//! `submit_transition` client, and confirm the node committed and persisted it.

use satusd_operator::{submit_transition, SubmitError};
use satusd_state::node::StateNode;
use serde_json::json;

/// Boot the state-node router on an OS-assigned port; return its `host:port`.
/// The server thread is detached (process exit reaps it).
fn spawn_node() -> String {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener =
        rt.block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap() });
    let addr = listener.local_addr().unwrap();
    let node = StateNode::genesis([0x01; 32], [0x02; 32], 0, 5_000_000_000_000);
    std::thread::spawn(move || {
        rt.block_on(async { satusd_state_node::serve(listener, node).await });
    });
    addr.to_string()
}

fn issuer_register_body(issuer_id: [u8; 32]) -> serde_json::Value {
    json!({
        "transition": "issuer_register",
        "issuer_id": hex::encode(issuer_id),
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
    })
}

#[test]
fn operator_submits_transition_over_http() {
    let base = spawn_node();

    // The listen socket is bound before the thread starts accepting, so the
    // backlog absorbs the connect; retry briefly to cover thread startup.
    let body = issuer_register_body([0xaa; 32]);
    let root = (0..50)
        .find_map(|_| match submit_transition(&base, &body) {
            Ok(r) => Some(r),
            Err(SubmitError::Connect(_)) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
                None
            }
            Err(e) => panic!("submit failed: {e:?}"),
        })
        .expect("node reachable");

    // It committed (root moved off genesis-zero).
    assert_ne!(root, [0u8; 32], "expected a committed state root");

    // It persisted: re-submitting the same issuer hits IssuerAlreadyRegistered,
    // which is only possible if the first POST mutated the live node.
    match submit_transition(&base, &body) {
        Err(SubmitError::Rejected { status, error }) => {
            assert_eq!(status, 400);
            assert!(error.contains("IssuerAlreadyRegistered"), "{error}");
        }
        other => panic!("expected rejection on replay, got {other:?}"),
    }
}
