//! SatUSD state node binary — boots the HTTP service (see crate lib for routes).

use satusd_state::node::StateNode;

#[tokio::main]
async fn main() {
    // MVP genesis: fixed asset family + oracle context (regtest, $50k).
    let node = StateNode::genesis([0x01; 32], [0x02; 32], 0, 5_000_000_000_000);
    let addr = "127.0.0.1:8088";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("satusd state node listening on http://{addr}");
    satusd_state_node::serve(listener, node).await;
}
