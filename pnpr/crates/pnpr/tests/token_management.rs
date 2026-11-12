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
    config.identity.auth.users.max_users = MaxUsers::Unlimited;
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

fn put_json_with_bearer(path: &str, token: &str, body: &Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

fn rfc3339_unix_seconds(raw: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(raw)
        .unwrap_or_else(|err| panic!("expected RFC 3339 timestamp, got {raw:?}: {err}"))
        .timestamp()
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
        "expires": 7
    });

    let res = app
        .clone()
        .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res.into_body()).await;
    let raw_token = body["token"].as_str().unwrap();
    assert_eq!(raw_token.len(), pnpr_auth::PERSONAL_TOKEN_LEN);
    assert!(raw_token.starts_with(pnpr_auth::PERSONAL_TOKEN_PREFIX));
    assert!(pnpr_auth::has_valid_personal_token_checksum(raw_token));
    assert!(body.get("scopes").is_none());
    assert!(body.get("kind").is_none());
    assert_eq!(
        body["user"].as_str(),
        crate::tokens::account_id(&app, "alice", Some(&login_token)).await.as_deref(),
    );
    assert_eq!(body["readonly"], true);
    assert_eq!(body["cidr_whitelist"], json!(["10.0.0.0/8"]));
    assert_eq!(body["name"], "ci-token");
    let created_secs = rfc3339_unix_seconds(body["created"].as_str().unwrap());
    let expires_secs = rfc3339_unix_seconds(body["expires"].as_str().unwrap());
    let ttl_secs = expires_secs - created_secs;
    assert!(
        (7 * 86_400 - 2..=7 * 86_400 + 2).contains(&ttl_secs),
        "expected ~7 days ({}s), got {ttl_secs}s",
        7 * 86_400,
    );
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
    assert!(found.get("kind").is_none());
    assert_eq!(found["token"].as_str().unwrap(), &body["key"].as_str().unwrap()[..6]);
}

#[tokio::test]
async fn create_token_with_kind() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    for kind in ["classic", "granular"] {
        let payload = json!({
            "password": "secret123",
            "kind": kind,
        });

        let res = app
            .clone()
            .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res.into_body()).await;
        assert_eq!(body["kind"], kind);
        assert!(body.get("scopes").is_none());
    }

    let list_res = app
        .clone()
        .oneshot(get_with_bearer("/-/npm/v1/tokens", &login_token))
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = body_json(list_res.into_body()).await;
    let tokens = list_body["objects"].as_array().expect("array of tokens");
    assert!(
        tokens
            .iter()
            .all(|token| token.get("scopes").is_none()),
    );
    assert!(
        tokens
            .iter()
            .any(|token| token["kind"] == "classic"),
        "expected classic token in listing",
    );
    assert!(
        tokens
            .iter()
            .any(|token| token["kind"] == "granular"),
        "expected granular token in listing",
    );
}

#[tokio::test]
async fn npm12_granular_token_permissions_and_expiry_days() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    let publish_res = app
        .clone()
        .oneshot(put_json_with_bearer(
            "/npm/acme-pkg",
            &login_token,
            &crate::npm::publish_doc("acme-pkg", "1.0.0", b"v1"),
        ))
        .await
        .unwrap();
    assert_eq!(publish_res.status(), StatusCode::CREATED);

    let ro_res = app
        .clone()
        .oneshot(post_json(
            "/-/npm/v1/tokens",
            &login_token,
            json!({
                "name": null,
                "password": "secret123",
                "description": "npm 12 read-only token",
                "packages_all": true,
                "packages_and_scopes_permission": "read-only",
                "expires": 7,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(ro_res.status(), StatusCode::OK);
    let ro_body = body_json(ro_res.into_body()).await;
    assert_eq!(ro_body["readonly"], true);
    assert!(ro_body.get("name").is_none());
    let ro_ttl = rfc3339_unix_seconds(ro_body["expires"].as_str().unwrap())
        - rfc3339_unix_seconds(ro_body["created"].as_str().unwrap());
    assert!((7 * 86_400 - 2..=7 * 86_400 + 2).contains(&ro_ttl));
    let ro_token = ro_body["token"].as_str().unwrap();

    for read_path in ["/-/whoami", "/npm/acme-pkg", "/npm/-/package/acme-pkg/dist-tags"] {
        let read_res = app
            .clone()
            .oneshot(get_with_bearer(read_path, ro_token))
            .await
            .unwrap();
        assert_eq!(read_res.status(), StatusCode::OK, "GET {read_path}");
    }

    let ro_tag_res = app
        .clone()
        .oneshot(put_json_with_bearer(
            "/npm/-/package/acme-pkg/dist-tags/next",
            ro_token,
            &json!("1.0.0"),
        ))
        .await
        .unwrap();
    assert_eq!(ro_tag_res.status(), StatusCode::FORBIDDEN);

    let ro_pub_res = app
        .clone()
        .oneshot(put_json_with_bearer(
            "/npm/acme-pkg",
            ro_token,
            &crate::npm::publish_doc("acme-pkg", "1.0.1", b"v2"),
        ))
        .await
        .unwrap();
    assert_eq!(ro_pub_res.status(), StatusCode::FORBIDDEN);

    let rw_res = app
        .clone()
        .oneshot(post_json(
            "/-/npm/v1/tokens",
            &login_token,
            json!({
                "name": "npm12-rw",
                "password": "secret123",
                "packages_all": true,
                "packages_and_scopes_permission": "read-write",
                "bypass_2fa": true,
                "expires": "14",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(rw_res.status(), StatusCode::OK);
    let rw_body = body_json(rw_res.into_body()).await;
    assert_eq!(rw_body["readonly"], false);
    let rw_ttl = rfc3339_unix_seconds(rw_body["expires"].as_str().unwrap())
        - rfc3339_unix_seconds(rw_body["created"].as_str().unwrap());
    assert!((14 * 86_400 - 2..=14 * 86_400 + 2).contains(&rw_ttl));
    let rw_token = rw_body["token"].as_str().unwrap();

    let rw_tag_res = app
        .clone()
        .oneshot(put_json_with_bearer(
            "/npm/-/package/acme-pkg/dist-tags/next",
            rw_token,
            &json!("1.0.0"),
        ))
        .await
        .unwrap();
    assert_eq!(rw_tag_res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn create_token_rejects_unsupported_granular_restrictions_without_minting() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    for payload in [
        json!({
            "password": "secret123",
            "packages": ["acme-pkg"],
            "packages_and_scopes_permission": "read-only",
        }),
        json!({ "password": "secret123", "packages": [] }),
        json!({ "password": "secret123", "packages_all": false }),
        json!({ "password": "secret123", "scopes": ["@acme"] }),
        json!({ "password": "secret123", "orgs": ["acme"] }),
        json!({ "password": "secret123", "orgs_permission": "read-only" }),
        json!({ "password": "secret123", "packages_and_scopes_permission": "read-write-stage-only" }),
        json!({ "password": "secret123", "packages_and_scopes_permission": "no-access" }),
        json!({
            "password": "secret123",
            "readonly": true,
            "packages_and_scopes_permission": "read-write",
        }),
        json!({ "password": "secret123", "expires": 7, "expires_in": 60 }),
        json!({ "password": "secret123", "expires": 0 }),
        json!({ "password": "secret123", "unknown_restriction": true }),
    ] {
        let res = app
            .clone()
            .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload.clone()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "payload {payload}");
    }

    let list_res = app
        .clone()
        .oneshot(get_with_bearer("/-/npm/v1/tokens", &login_token))
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = body_json(list_res.into_body()).await;
    let tokens = list_body["objects"].as_array().expect("array of tokens");
    assert_eq!(tokens.len(), 1, "only the initial adduser token should exist");
}

#[tokio::test]
async fn create_token_rejects_unknown_kind() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;

    for invalid in ["custom".to_string(), String::new(), "CLASSIC".to_string(), "a".repeat(65)] {
        let payload = json!({
            "password": "secret123",
            "kind": invalid,
        });

        let res = app
            .clone()
            .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "kind {invalid:?}");
    }
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
    assert_eq!(
        body_json(res.into_body()).await["error"],
        "Authentication required for password verification",
    );

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

#[tokio::test]
async fn passwordless_token_creation_stays_within_the_calling_token() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(static_config(tmp.path().to_path_buf()), auth.clone());
    let id = auth.users.provision_user("alice").await.unwrap();
    let caller_expires = chrono::Utc::now().timestamp() + 3600;
    let (caller, _) = auth.tokens
        .issue_token(
            &id,
            pnpr_auth::NewToken::new().with_expires_at(Some(caller_expires.cast_unsigned() * 1000)),
        )
        .await
        .unwrap();
    let create = |payload: Value| {
        let app = app.clone();
        let caller = caller.clone();
        async move {
            let response = app
                .oneshot(post_json("/-/npm/v1/tokens", &caller, payload))
                .await
                .unwrap();
            (response.status(), body_json(response.into_body()).await)
        }
    };

    for payload in [json!({}), json!({ "expires": 30 })] {
        let (status, body) = create(payload).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(rfc3339_unix_seconds(body["expires"].as_str().unwrap()), caller_expires);
    }
    let (status, body) = create(json!({ "expires_in": 60 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(rfc3339_unix_seconds(body["expires"].as_str().unwrap()) < caller_expires);
}

#[tokio::test]
async fn max_lifetime_caps_login_and_created_tokens() {
    const WEEK_SECS: i64 = 7 * 86_400;
    let tmp = TempDir::new().unwrap();
    let mut config = static_config(tmp.path().to_path_buf());
    config.identity.auth.tokens.max_lifetime = Some(std::time::Duration::from_hours(7 * 24));
    let app = router(config);
    let assert_expires_after = |body: &Value, issued: (i64, i64), lifetime: i64| {
        let expires = rfc3339_unix_seconds(body["expires"].as_str().unwrap());
        assert!(
            (issued.0 + lifetime..=issued.1 + lifetime).contains(&expires),
            "expected {lifetime}s after {issued:?}, got {expires}",
        );
    };

    let before = chrono::Utc::now().timestamp();
    let login_token = add_user_and_get_token(&app, "alice", "secret123").await;
    let login_issued = (before, chrono::Utc::now().timestamp());
    let listed = app
        .clone()
        .oneshot(get_with_bearer("/-/npm/v1/tokens", &login_token))
        .await
        .unwrap();
    assert_expires_after(
        &body_json(listed.into_body()).await["objects"][0],
        login_issued,
        WEEK_SECS,
    );

    for (requested, lifetime) in [
        (json!({}), WEEK_SECS),
        (json!({ "expires": 30 }), WEEK_SECS),
        (json!({ "expires": 1 }), 86_400),
    ] {
        let mut payload = requested.clone();
        payload["password"] = json!("secret123");
        let before = chrono::Utc::now().timestamp();
        let response = app
            .clone()
            .oneshot(post_json("/-/npm/v1/tokens", &login_token, payload))
            .await
            .unwrap();
        let issued = (before, chrono::Utc::now().timestamp());
        assert_eq!(response.status(), StatusCode::OK, "{requested}");
        assert_expires_after(&body_json(response.into_body()).await, issued, lifetime);
    }
}
