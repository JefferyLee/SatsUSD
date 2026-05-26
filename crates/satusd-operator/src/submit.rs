//! Operator → state-node transition submission over HTTP (PRD §7.3).
//!
//! The operator submits transitions (e.g. a batched `ReserveClaim`) by POSTing
//! the tagged-JSON DTO to the running state node's `/v1/transition/submit`
//! endpoint, rather than calling the `StateNode` Rust API in-process. This is a
//! minimal blocking HTTP/1.1 client over `std::net` — localhost MVP, no TLS.

use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Debug)]
pub enum SubmitError {
    /// Could not reach the node.
    Connect(std::io::Error),
    /// Socket read/write failed mid-exchange.
    Io(std::io::Error),
    /// The node rejected the transition (non-200); carries its `error` string.
    Rejected { status: u16, error: String },
    /// The response was not the expected shape.
    BadResponse(String),
}

/// POST a tagged transition body to `http://{base}/v1/transition/submit` and
/// return the committed `new_state_root`. `base` is `host:port`
/// (e.g. `"127.0.0.1:8088"`); `body` is a `{"transition": "...", ...}` object.
pub fn submit_transition(base: &str, body: &serde_json::Value) -> Result<[u8; 32], SubmitError> {
    let payload = serde_json::to_vec(body).expect("json value is serializable");
    let mut stream = TcpStream::connect(base).map_err(SubmitError::Connect)?;
    let head = format!(
        "POST /v1/transition/submit HTTP/1.1\r\nHost: {base}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes()).map_err(SubmitError::Io)?;
    stream.write_all(&payload).map_err(SubmitError::Io)?;

    // `Connection: close` makes the server close after the response, so reading
    // to EOF captures exactly the headers + body.
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(SubmitError::Io)?;

    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| SubmitError::BadResponse("no header terminator".into()))?;
    let status = String::from_utf8_lossy(&raw[..split])
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| SubmitError::BadResponse("no status line".into()))?;
    let json: serde_json::Value = serde_json::from_slice(&raw[split + 4..])
        .map_err(|e| SubmitError::BadResponse(e.to_string()))?;

    if status != 200 {
        return Err(SubmitError::Rejected {
            status,
            error: json
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("")
                .to_string(),
        });
    }
    let root_hex = json
        .get("new_state_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SubmitError::BadResponse("missing new_state_root".into()))?;
    hex::decode(root_hex)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| SubmitError::BadResponse("new_state_root not 32 bytes".into()))
}
