//! Live DA mirror — operator uploads `LiveDABundle` bytes by `live_da_root`,
//! challenger fetches them (PRD §10.1, §7.4). Minimal in-memory store; persistence
//! and multi-mirror (DA-04/05) are M5-ops follow-ups. Upload is validated: parse
//! and recompute the root, rejecting if it doesn't match the path.
//!
//! Routes:
//! - `GET  /v1/da/live/{root}` → raw bundle bytes (404 if absent)
//! - `POST /v1/da/live/{root}` → upload (body = bundle bytes); validates root

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use satusd_da::LiveDABundle;

type Store = Arc<Mutex<HashMap<[u8; 32], Vec<u8>>>>;

fn parse_root(s: &str) -> Result<[u8; 32], ()> {
    let v = hex::decode(s).map_err(|_| ())?;
    v.try_into().map_err(|_| ())
}

async fn get_bundle(State(s): State<Store>, Path(root_hex): Path<String>) -> impl IntoResponse {
    let Ok(root) = parse_root(&root_hex) else {
        return (StatusCode::BAD_REQUEST, "bad root hex").into_response();
    };
    match s.lock().unwrap().get(&root) {
        Some(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "no bundle for that root").into_response(),
    }
}

async fn put_bundle(
    State(s): State<Store>,
    Path(root_hex): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let Ok(root) = parse_root(&root_hex) else {
        return (StatusCode::BAD_REQUEST, "bad root hex");
    };
    let bytes = body.to_vec();
    let Ok((bundle, _declared)) = LiveDABundle::decode(&bytes) else {
        return (StatusCode::BAD_REQUEST, "bundle does not decode");
    };
    if bundle.live_da_root() != root {
        return (StatusCode::BAD_REQUEST, "recomputed root != path");
    }
    s.lock().unwrap().insert(root, bytes);
    (StatusCode::CREATED, "stored")
}

/// Build the DA-mirror router (fresh in-memory store).
pub fn router() -> Router {
    let store: Store = Arc::new(Mutex::new(HashMap::new()));
    Router::new()
        .route("/v1/da/live/{root}", get(get_bundle).post(put_bundle))
        .with_state(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use satusd_da::{section, Section};
    use tower::ServiceExt;

    fn sample_bundle() -> (Vec<u8>, [u8; 32]) {
        let b = LiveDABundle::new(
            [0xab; 32],
            7,
            vec![
                Section {
                    id: section::REDEEM_INTENTS,
                    content: vec![1, 2, 3],
                },
                Section {
                    id: section::ORACLE_MESSAGES,
                    content: vec![0xaa; 16],
                },
            ],
        );
        (b.encode(), b.live_da_root())
    }

    #[tokio::test]
    async fn put_get_roundtrips_and_validates_root() {
        let app = router();
        let (bytes, root) = sample_bundle();
        let root_hex = hex::encode(root);

        // POST stores it (root matches body).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/da/live/{root_hex}"))
                    .body(Body::from(bytes.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // GET returns the same bytes.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/da/live/{root_hex}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let got = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        assert_eq!(got, bytes);

        // GET on an unknown root → 404.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/da/live/{}", "ff".repeat(32)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_rejects_root_mismatch() {
        let app = router();
        let (bytes, _root) = sample_bundle();
        // POST under the WRONG root → 400.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/da/live/{}", "00".repeat(32)))
                    .body(Body::from(bytes))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
