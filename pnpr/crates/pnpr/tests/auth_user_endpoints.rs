//! Acceptance tests for the npm-CLI-compatible user and token
//! endpoints — `whoami`, profile, token listing, token revocation,
//! and logout. The token-listing endpoint backs `npm token list`;
//! the rest gate behind a valid bearer token in the same way as
//! `npm whoami` / `npm profile get` / `npm logout` expect.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{
    AuthConfig, AuthState, Config, HtpasswdConfig, MaxUsers, TokensConfig, router, router_with_auth,
};
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn static_config(storage: PathBuf) -> Config {
    let mut config = Config::static_serve(listen(), storage);
    config.public_url = "http://example.test".to_string();
    config
}

fn persistent_config(storage: PathBuf, htpasswd: PathBuf, tokens_db: PathBuf) -> Config {
    let mut config = static_config(storage);
    config.auth = AuthConfig {
        htpasswd: HtpasswdConfig { file: Some(htpasswd), max_users: MaxUsers::Unlimited },
        tokens: TokensConfig { file: Some(tokens_db) },
    };
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn put_json(path: &str, body: Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn get_with_bearer(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn delete_with_bearer(path: &str, token: &str) -> Request<Body> {
    Request::delete(path)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
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

/// Register a user via the adduser PUT and return the bearer token.
async fn add_user_and_get_token(
    app: axum::Router,
    username: &str,
    password: &str,
) -> (axum::Router, String) {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let response =
        app.clone().oneshot(put_json(&path, adduser_body(username, password))).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let token = payload["token"].as_str().expect("token in response").to_string();
    (app, token)
}

#[tokio::test]
async fn whoami_returns_username_for_authenticated_caller() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let response = app.oneshot(get_with_bearer("/-/whoami", &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response.into_body()).await;
    assert_eq!(payload["username"].as_str(), Some("alice"));
}

#[tokio::test]
async fn whoami_returns_401_when_unauthenticated() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/-/whoami").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn whoami_returns_401_for_unknown_bearer() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let response = app.oneshot(get_with_bearer("/-/whoami", "not-a-real-token")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn profile_returns_user_info() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let response = app.oneshot(get_with_bearer("/-/npm/v1/user", &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response.into_body()).await;
    assert_eq!(payload["name"].as_str(), Some("alice"));
    assert_eq!(payload["tfa"].as_bool(), Some(false));
    // npm CLI's table renderer expects these keys to be present even
    // when empty — make sure we don't omit them.
    assert!(payload.get("email").is_some(), "email field must be present");
    assert!(payload.get("cidr_whitelist").is_some(), "cidr_whitelist field must be present");
}

#[tokio::test]
async fn profile_returns_401_when_unauthenticated() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/-/npm/v1/user").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_list_returns_only_callers_tokens() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, _bob_token) = add_user_and_get_token(app, "bob", "secret").await;

    // Issue a second token to alice so the listing has more than one.
    let (app, _alice_token_2) = add_user_and_get_token(app, "alice", "secret").await;

    let response = app.oneshot(get_with_bearer("/-/npm/v1/tokens", &alice_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response.into_body()).await;
    let objects = payload["objects"].as_array().expect("objects array");
    assert_eq!(objects.len(), 2, "alice owns two tokens, bob's must not leak");
    for entry in objects {
        assert_eq!(entry["user"].as_str(), Some("alice"));
        let key = entry["key"].as_str().expect("key field");
        assert_eq!(key.len(), 64, "key is the SHA-256 hex of the raw token");
        let preview = entry["token"].as_str().expect("token preview");
        assert_eq!(preview.len(), 6, "token preview is the leading 6 chars of the key");
        assert!(key.starts_with(preview), "preview must match the key prefix");
    }
}

#[tokio::test]
async fn token_list_returns_401_when_unauthenticated() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/-/npm/v1/tokens").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn revoke_token_by_key_removes_the_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, victim_token) = add_user_and_get_token(app, "alice", "secret").await;

    // Read the key for the victim token via the list endpoint.
    let response =
        app.clone().oneshot(get_with_bearer("/-/npm/v1/tokens", &alice_token)).await.unwrap();
    let payload = body_json(response.into_body()).await;
    let objects = payload["objects"].as_array().unwrap();
    let alice_key = objects[0]["key"].as_str().unwrap();
    let alice_other = objects[1]["key"].as_str().unwrap();
    // Pick whichever key matches the victim token's hash by querying
    // each: revoke one and check that the other still authenticates.
    let target_key = if uses_token(&app, &victim_token, alice_key).await {
        alice_key.to_string()
    } else {
        alice_other.to_string()
    };

    let path = format!("/-/npm/v1/tokens/token/{target_key}");
    let response = app.clone().oneshot(delete_with_bearer(&path, &alice_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The revoked token must no longer authenticate.
    let response = app.clone().oneshot(get_with_bearer("/-/whoami", &victim_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // The other token still works.
    let response = app.oneshot(get_with_bearer("/-/whoami", &alice_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// Returns true when `key` is the hash of `raw_token`. We can't
/// inspect the store from the test, so we drive a revocation through
/// the public API and check whether the raw token stopped working.
/// Used by [`revoke_token_by_key_removes_the_token`] to pick which of
/// the two listed keys belongs to the token we want to revoke.
async fn uses_token(app: &axum::Router, raw_token: &str, candidate_key: &str) -> bool {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in &digest {
        write!(hex, "{byte:02x}").unwrap();
    }
    let _ = app; // app is unused but kept for symmetry with the other helpers
    hex == candidate_key
}

#[tokio::test]
async fn revoke_token_by_key_404s_for_unknown_key() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let path = format!("/-/npm/v1/tokens/token/{}", "0".repeat(64));
    let response = app.oneshot(delete_with_bearer(&path, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoke_token_by_key_rejects_revoking_someone_elses_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret").await;

    // Pull bob's key out of bob's listing.
    let response =
        app.clone().oneshot(get_with_bearer("/-/npm/v1/tokens", &bob_token)).await.unwrap();
    let payload = body_json(response.into_body()).await;
    let bob_key = payload["objects"][0]["key"].as_str().unwrap().to_string();

    // Alice cannot revoke bob's token.
    let path = format!("/-/npm/v1/tokens/token/{bob_key}");
    let response = app.clone().oneshot(delete_with_bearer(&path, &alice_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Bob's token must still work.
    let response = app.oneshot(get_with_bearer("/-/whoami", &bob_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn revoke_token_by_key_requires_auth() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let path = format!("/-/npm/v1/tokens/token/{}", "0".repeat(64));
    let response = app.oneshot(Request::delete(&path).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_deletes_the_callers_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let path = format!("/-/user/token/{token}");
    let response = app.clone().oneshot(delete_with_bearer(&path, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The token must no longer authenticate.
    let response = app.oneshot(get_with_bearer("/-/whoami", &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_returns_404_for_unknown_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let path = "/-/user/token/not-a-real-token";
    let response = app.oneshot(delete_with_bearer(path, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logout_requires_caller_to_own_the_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret").await;

    // Alice asks to log out using bob's token — forbidden.
    let path = format!("/-/user/token/{bob_token}");
    let response = app.clone().oneshot(delete_with_bearer(&path, &alice_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Bob's token must still work.
    let response = app.oneshot(get_with_bearer("/-/whoami", &bob_token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn logout_requires_auth() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let response = app
        .oneshot(Request::delete("/-/user/token/anything").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Every auth-endpoint response — success, 401, 403, 404 alike —
/// must mark itself uncacheable so an intermediary HTTP cache that
/// ignores `Vary` can't latch onto one user's identity and serve it
/// to another.
#[tokio::test]
async fn auth_endpoints_set_private_no_cache_headers() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret").await;

    // Bob's key — used to drive the 403 cross-user revoke branch below.
    let response =
        app.clone().oneshot(get_with_bearer("/-/npm/v1/tokens", &bob_token)).await.unwrap();
    let bob_key =
        body_json(response.into_body()).await["objects"][0]["key"].as_str().unwrap().to_string();

    // (request, authenticated-or-not, expected-status). Each entry
    // exercises a branch that must still carry the privacy headers.
    let cases: Vec<(Request<Body>, StatusCode)> = vec![
        (get_with_bearer("/-/whoami", &alice_token), StatusCode::OK),
        (Request::get("/-/whoami").body(Body::empty()).unwrap(), StatusCode::UNAUTHORIZED),
        (get_with_bearer("/-/npm/v1/user", &alice_token), StatusCode::OK),
        (get_with_bearer("/-/npm/v1/tokens", &alice_token), StatusCode::OK),
        (
            delete_with_bearer(&format!("/-/npm/v1/tokens/token/{bob_key}"), &alice_token),
            StatusCode::FORBIDDEN,
        ),
        (
            delete_with_bearer(&format!("/-/npm/v1/tokens/token/{}", "0".repeat(64)), &alice_token),
            StatusCode::NOT_FOUND,
        ),
        (delete_with_bearer("/-/user/token/not-real", &alice_token), StatusCode::NOT_FOUND),
    ];

    for (request, expected_status) in cases {
        let path = request.uri().path().to_string();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected_status, "unexpected status for {path}");
        let cache_control = response
            .headers()
            .get("cache-control")
            .map(|value| value.to_str().unwrap().to_string())
            .unwrap_or_default();
        let vary = response
            .headers()
            .get("vary")
            .map(|value| value.to_str().unwrap().to_string())
            .unwrap_or_default();
        assert_eq!(
            cache_control, "private, no-store",
            "{path}: cache-control must lock the response to the caller",
        );
        assert_eq!(
            vary, "Authorization",
            "{path}: Vary must include Authorization so shared caches partition by credentials",
        );
    }
}

/// Revocation must persist across a restart — once a token is
/// revoked, reopening the `SQLite` store must not reload it.
#[tokio::test]
async fn revocation_survives_restart() {
    let auth_dir = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let htpasswd = auth_dir.path().join("htpasswd");
    let tokens_db = auth_dir.path().join("tokens.db");

    let config =
        persistent_config(storage.path().to_path_buf(), htpasswd.clone(), tokens_db.clone());
    let auth = AuthState::load(&config.auth, &config.backend).await.expect("first boot");
    let app = router_with_auth(config.clone(), auth);
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // Logout (revoke own token).
    let path = format!("/-/user/token/{token}");
    let response = app.clone().oneshot(delete_with_bearer(&path, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    drop(app);
    let auth = AuthState::load(&config.auth, &config.backend).await.expect("reload after restart");
    let app = router_with_auth(config, auth);

    // Token must remain revoked after restart.
    let response = app.oneshot(get_with_bearer("/-/whoami", &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
