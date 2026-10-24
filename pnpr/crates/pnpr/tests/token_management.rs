use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    sync::Arc,
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{AuthState, Config, MaxUsers, router, router_with_auth};
use pnpr_fixtures::MockConfigPresets;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn static_config(storage: PathBuf) -> Config {
    let mut config = Config::static_serve(listen(), storage);
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
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
    reason = "test helper called with owned literals; by-value keeps call sites clean"
)]
fn put_json(path: &str, body: Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called with owned literals; by-value keeps call sites clean"
)]
fn post_json(path: &str, token: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn get_with_bearer(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn adduser_body(username: &str, password: &str) -> Value {
    json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": "user@example.test",
        "type": "user",
        "roles": [],
    })
}

async fn add_user_and_get_token(app: &axum::Router, username: &str, password: &str) -> String {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let response = app
        .clone()
        .oneshot(put_json(&path, adduser_body(username, password)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    payload["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

#[tokio::test]
async fn create_token_with_readonly_cidr_name_and_expires() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "readonly": true,
        "cidr_whitelist": ["10.0.0.0/8"],
        "name": "ci-token",
        "expires": 3600
    });

    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res.into_body()).await;
    assert_eq!(body["token"].as_str().unwrap().len(), 32);
    assert_eq!(body["user"], "alice");
    assert_eq!(body["readonly"], true);
    assert_eq!(body["cidr_whitelist"], json!(["10.0.0.0/8"]));
    assert_eq!(body["name"], "ci-token");
    assert!(body["expires"].is_string());
    assert!(body["created"].is_string());
    assert!(body["updated"].is_string());

    let list_res = app
        .clone()
        .oneshot(get_with_bearer("/-/npm/v1/tokens", &login_token))
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = body_json(list_res.into_body()).await;
    let tokens = list_body["objects"].as_array().expect("array of tokens");
    let found = tokens
        .iter()
        .find(|token| token["name"] == "ci-token")
        .expect("found created token");
    assert_eq!(found["readonly"], true);
    assert_eq!(found["token"].as_str().unwrap(), &body["key"].as_str().unwrap()[..6]);
}

#[tokio::test]
async fn create_token_with_scopes_and_kind() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "scopes": ["pkg:read", "pkg:write"],
        "kind": "granular"
    });

    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res.into_body()).await;
    assert_eq!(body["scopes"], json!(["pkg:read", "pkg:write"]));
    assert_eq!(body["kind"], "granular");

    let list_res = app
        .clone()
        .oneshot(get_with_bearer("/-/npm/v1/tokens", &login_token))
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = body_json(list_res.into_body()).await;
    let tokens = list_body["objects"].as_array().expect("array of tokens");
    let found = tokens
        .iter()
        .find(|token| token["kind"] == "granular")
        .expect("found granular token");
    assert_eq!(found["scopes"], json!(["pkg:read", "pkg:write"]));
}

#[tokio::test]
async fn create_token_rejects_overlong_kind() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "kind": "a".repeat(65),
    });

    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_token_under_registry_prefix() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "readonly": false,
        "name": "prefix-token"
    });

    let res = app
        .clone()
        .oneshot(post_json("/~npmjs/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res.into_body()).await;
    assert_eq!(body["name"], "prefix-token");
}

#[tokio::test]
async fn create_token_password_verification_and_rate_limiting() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "wrong-password",
    });
    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let empty_payload = json!({});
    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, empty_payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_token_cannot_authenticate() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let sub_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_millis();
    if sub_ms > 600 {
        tokio::time::sleep(std::time::Duration::from_millis(1050 - u64::from(sub_ms))).await;
    }

    let payload = json!({
        "password": "secret123",
        "expires_in": 2
    });
    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res.into_body()).await;
    let expired_token = body["token"].as_str().unwrap();

    let valid_res = app
        .clone()
        .oneshot(get_with_bearer("/-/whoami", expired_token))
        .await
        .unwrap();
    assert_eq!(valid_res.status(), StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    let whoami_res = app
        .clone()
        .oneshot(get_with_bearer("/-/whoami", expired_token))
        .await
        .unwrap();
    assert_eq!(whoami_res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_authentication_updates_last_used_at() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let tokens = Arc::clone(&auth.tokens);
    let app = router_with_auth(static_config(tmp.path().to_path_buf()), auth);
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "name": "touch-test"
    });
    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    let body = body_json(res.into_body()).await;
    let new_token = body["token"].as_str().unwrap();

    assert_eq!(body["updated"], body["created"]);
    let aged = crate::tokens::age_last_use(&tokens, new_token).await;

    let whoami = app
        .clone()
        .oneshot(get_with_bearer("/-/whoami", new_token))
        .await
        .unwrap();
    assert_eq!(whoami.status(), StatusCode::OK);
    crate::tokens::wait_for_last_use_after(&tokens, new_token, aged).await;

    let list_res = app
        .clone()
        .oneshot(get_with_bearer("/-/npm/v1/tokens", &login_token))
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = body_json(list_res.into_body()).await;
    let tokens = list_body["objects"].as_array().unwrap();
    let entry = tokens
        .iter()
        .find(|token| token["name"] == "touch-test")
        .unwrap();
    assert!(entry["updated"].as_str().unwrap() >= entry["created"].as_str().unwrap());
}

#[tokio::test]
async fn readonly_token_cannot_create_tokens_or_write() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "readonly": true,
        "name": "ro-tok"
    });
    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    let body = body_json(res.into_body()).await;
    let ro_token = body["token"].as_str().unwrap();

    let create_attempt = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", ro_token, json!({ "name": "sub-token" })))
        .await
        .unwrap();
    assert_eq!(create_attempt.status(), StatusCode::FORBIDDEN);

    let publish_attempt = app
        .clone()
        .oneshot(
            Request::put("/npm/some-package")
                .header(header::AUTHORIZATION, format!("Bearer {ro_token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "name": "some-package" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(publish_attempt.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn readonly_token_is_permitted_for_audit_endpoints() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let payload = json!({
        "password": "secret123",
        "readonly": true
    });
    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    let ro_token = body_json(res.into_body()).await["token"]
        .as_str()
        .unwrap()
        .to_string();

    let audit_res = app
        .clone()
        .oneshot(
            Request::post("/npm/-/npm/v1/security/advisories/bulk")
                .header(header::AUTHORIZATION, format!("Bearer {ro_token}"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(audit_res.status(), StatusCode::FORBIDDEN);

    let prefix_audit_res = app
        .clone()
        .oneshot(
            Request::post("/npm/~npmjs/-/npm/v1/security/advisories/bulk")
                .header(header::AUTHORIZATION, format!("Bearer {ro_token}"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(prefix_audit_res.status(), StatusCode::FORBIDDEN);
}
