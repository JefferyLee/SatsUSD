//! Live DA mirror service (PRD §7.4, §10.1): the operator POSTs the LiveDABundle
//! by `live_da_root`, the challenger fetches it. Minimal in-memory store; multi-
//! mirror selection (DA-04/05) and persistent storage are M5-ops follow-ups.
//!
//!   cargo run -p satusd-state-node --bin da_mirror

use satusd_state_node::da_mirror;

#[tokio::main]
async fn main() {
    let addr = "127.0.0.1:8089";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("satusd DA mirror listening on http://{addr}");
    println!("  POST /v1/da/live/{{root_hex}}  upload (body = bundle bytes)");
    println!("  GET  /v1/da/live/{{root_hex}}  fetch");
    axum::serve(listener, da_mirror::router()).await.unwrap();
}
