use super::{router_with_auth, token_timestamp_millis};
use crate::auth::{AuthState, TokenBackendKind, TokenStore, UserBackendKind};
use crate::config::Config;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

#[test]
fn token_timestamp_millis_saturates_before_i64_conversion() {
    assert_eq!(token_timestamp_millis(42), 42_000);
    assert_eq!(token_timestamp_millis(u64::MAX), i64::MAX / 1000 * 1000);
}

fn static_config(storage: std::path::PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = "http://example.test".to_string();
    config
}

fn adduser_body(username: &str, password: &str) -> Value {
    json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": "foo@bar.net",
        "type": "user",
        "roles": [],
    })
}

async fn body_json(body: Body) -> Value {
    let bytes = to_bytes(body, usize::MAX).await.expect("read body");
    serde_json::from_slice(&bytes).expect("body parses as JSON")
}

/// The adduser handler must bind token issuance and the response
/// identity to the canonical username returned by the backend, not the
/// username from the request. A [`UserBackendKind::Fixed`] fixture
/// returns a canonical name (`Alice`) that differs from the request
/// (`alice`) so the difference is observable.
#[tokio::test]
async fn adduser_issues_token_for_canonical_username() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState {
        users: Arc::new(UserBackendKind::Fixed { canonical: "Alice".to_string() }),
        tokens: Arc::new(TokenBackendKind::Store(TokenStore::in_memory())),
    };
    let app = router_with_auth(static_config(tmp.path().to_path_buf()), auth);

    let request = Request::put("/-/user/org.couchdb.user:alice")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&adduser_body("alice", "secret")).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    assert_eq!(payload["id"].as_str(), Some("org.couchdb.user:Alice"));
    assert_eq!(payload["ok"].as_str(), Some("you are authenticated as 'Alice'"));
    let token = payload["token"].as_str().expect("token in response").to_string();

    let request = Request::get("/-/whoami")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response.into_body()).await;
    assert_eq!(payload["username"].as_str(), Some("Alice"));
}
