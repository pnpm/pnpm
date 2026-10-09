//! An in-memory server for the Turborepo Remote Cache API routes pnpm uses:
//! `GET`, `HEAD`, and `PUT /v8/artifacts/<hash>`.

use axum::{
    Router,
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, Method, StatusCode},
    routing::get,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, TcpListener},
    sync::{Arc, Mutex},
    thread,
};

/// One request the server answered.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub hash: String,
    pub query: Option<String>,
    pub authorization: Option<String>,
}

#[derive(Default)]
struct Shared {
    artifacts: Mutex<HashMap<String, Vec<u8>>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

/// A running server. It lives until the test process exits.
pub struct TurborepoCache {
    url: String,
    shared: Arc<Shared>,
}

impl TurborepoCache {
    #[must_use]
    pub fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind the Turborepo cache to an unused localhost port");
        listener.set_nonblocking(true).expect("set the Turborepo cache listener to nonblocking");
        let url = format!("http://{}", listener.local_addr().expect("read the listener address"));
        let shared = Arc::<Shared>::default();
        let router = Router::new()
            .route("/v8/artifacts/{hash}", get(get_artifact).put(put_artifact))
            .with_state(Arc::clone(&shared));
        thread::Builder::new()
            .name("pacquet-test-turborepo-cache".to_string())
            .spawn(move || serve(router, listener))
            .expect("spawn the Turborepo cache thread");
        TurborepoCache { url, shared }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Every stored artifact, by hash.
    #[must_use]
    pub fn artifacts(&self) -> HashMap<String, Vec<u8>> {
        self.shared.artifacts
            .lock()
            .expect("artifact lock")
            .clone()
    }

    /// Replace what is stored under `hash`, as a server that does not verify
    /// what it serves could.
    pub fn store(&self, hash: &str, body: Vec<u8>) {
        self.shared.artifacts
            .lock()
            .expect("artifact lock")
            .insert(hash.to_string(), body);
    }

    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.shared.requests
            .lock()
            .expect("request lock")
            .clone()
    }
}

fn serve(router: Router, listener: TcpListener) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create the Turborepo cache runtime")
        .block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener)
                .expect("create the Turborepo cache listener");
            axum::serve(listener, router).await.expect("serve the Turborepo cache");
        });
}

fn record(
    shared: &Shared,
    method: &Method,
    hash: &str,
    query: Option<String>,
    headers: &HeaderMap,
) {
    shared.requests
        .lock()
        .expect("request lock")
        .push(RecordedRequest {
            method: method.to_string(),
            hash: hash.to_string(),
            query,
            authorization: headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
        });
}

/// Also answers `HEAD`, which axum routes to `GET` handlers.
async fn get_artifact(
    method: Method,
    State(shared): State<Arc<Shared>>,
    Path(hash): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Vec<u8>, StatusCode> {
    record(&shared, &method, &hash, query, &headers);
    shared.artifacts
        .lock()
        .expect("artifact lock")
        .get(&hash)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)
}

async fn put_artifact(
    State(shared): State<Arc<Shared>>,
    Path(hash): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    record(&shared, &Method::PUT, &hash, query, &headers);
    shared.artifacts
        .lock()
        .expect("artifact lock")
        .insert(hash, body.to_vec());
    StatusCode::ACCEPTED
}
