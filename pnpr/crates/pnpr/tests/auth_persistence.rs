//! Acceptance tests for issue [#11974] — pnpr must keep user
//! accounts and bearer tokens across process restarts so an
//! operator can run it as a hosted registry without losing every
//! account on the next container redeploy.
//!
//! [#11974]: https://github.com/pnpm/pnpm/issues/11974

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{
    AuthConfig, AuthState, Config, HtpasswdConfig, MaxUsers, TokensConfig, router_with_auth,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    process::Command,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn persistent_config(storage: PathBuf, htpasswd: PathBuf, tokens_db: PathBuf) -> Config {
    let mut config = Config::static_serve(listen(), storage);
    config.public_url = "http://example.test".to_string();
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

/// Boot pnpr → adduser → restart pnpr with the same storage dir →
/// existing token still authenticates, existing username can log
/// back in with the same password.
#[tokio::test]
async fn user_and_token_survive_restart() {
    let auth_dir = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let htpasswd = auth_dir.path().join("htpasswd");
    let tokens_db = auth_dir.path().join("tokens.db");

    let config =
        persistent_config(storage.path().to_path_buf(), htpasswd.clone(), tokens_db.clone());
    let auth = AuthState::load(&config.auth, &config.backend).await.expect("first boot");
    let app = router_with_auth(config.clone(), auth);

    // adduser pulls a fresh token out of the response body.
    let response = app
        .clone()
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "secret")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let token = payload["token"].as_str().expect("token in response").to_string();
    assert!(!token.is_empty());

    // Both files should now exist on disk.
    assert!(htpasswd.exists(), "htpasswd should be created on first registration");
    assert!(tokens_db.exists(), "tokens.db should be created on first token issue");

    // Simulate a restart: drop the router (and the in-memory map),
    // re-load from disk, rebuild the router. Same config, same paths.
    drop(app);
    let auth = AuthState::load(&config.auth, &config.backend).await.expect("reload after restart");
    let app = router_with_auth(config, auth);

    // The token issued before the "restart" must still resolve to
    // alice — proves token hash round-tripped through SQLite.
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/package/anything/dist-tags/latest")
                .header("content-type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(serde_json::to_string("1.0.0").unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    // 404 from the dist-tag handler is fine — the point is we got
    // past the 401 gate, which proves the token still authenticates.
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED, "token should still authenticate");

    // The existing username must accept the same password and not
    // be re-registered as a brand-new user.
    let response = app
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "secret")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let ok_msg = payload["ok"].as_str().unwrap_or("");
    assert!(
        ok_msg.contains("authenticated"),
        "second adduser should be a login, not a registration; got ok={ok_msg:?}",
    );
}

/// Corrupt the htpasswd file → server returns a parse diagnostic on
/// startup, not a silent empty user list. The acceptance criterion
/// is that the operator sees an error rather than the server happily
/// booting up with every existing account effectively erased.
#[tokio::test]
async fn corrupt_htpasswd_fails_startup_with_diagnostic() {
    let auth_dir = TempDir::new().unwrap();
    let htpasswd = auth_dir.path().join("htpasswd");
    std::fs::write(&htpasswd, "this-line-has-no-colon-and-is-not-a-comment\n").unwrap();

    let config = persistent_config(
        TempDir::new().unwrap().path().to_path_buf(),
        htpasswd.clone(),
        auth_dir.path().join("tokens.db"),
    );
    let err = AuthState::load(&config.auth, &config.backend)
        .await
        .expect_err("malformed htpasswd should fail to load");
    let message = err.to_string();
    assert!(
        message.contains("htpasswd") && message.contains(&htpasswd.display().to_string()),
        "error must name the htpasswd file and explain what went wrong; got {message:?}",
    );
}

/// htpasswd file produced by pnpr is readable by Apache's `htpasswd
/// -v` — cross-tool compatibility. Skipped silently when the host
/// doesn't have htpasswd installed (some CI images don't).
#[tokio::test]
async fn htpasswd_file_is_verifiable_by_apache_htpasswd_tool() {
    if Command::new("htpasswd").arg("-h").output().is_err() {
        eprintln!("apache htpasswd not on PATH — skipping cross-tool compat test");
        return;
    }

    let auth_dir = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let htpasswd = auth_dir.path().join("htpasswd");

    let config = persistent_config(
        storage.path().to_path_buf(),
        htpasswd.clone(),
        auth_dir.path().join("tokens.db"),
    );
    let auth = AuthState::load(&config.auth, &config.backend).await.expect("first boot");
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "compat-secret")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // `htpasswd -v` (verify-only) exits 0 when the password matches.
    let status = Command::new("htpasswd")
        .args(["-v", "-b"])
        .arg(&htpasswd)
        .arg("alice")
        .arg("compat-secret")
        .status()
        .expect("spawn htpasswd");
    assert!(
        status.success(),
        "apache htpasswd should verify the pnpr-written hash; exit={status:?}",
    );

    // And a wrong password should be rejected by the same tool.
    let status = Command::new("htpasswd")
        .args(["-v", "-b"])
        .arg(&htpasswd)
        .arg("alice")
        .arg("wrong-password")
        .status()
        .expect("spawn htpasswd");
    assert!(
        !status.success(),
        "apache htpasswd should reject a wrong password against the pnpr-written hash",
    );
}

/// `auth.htpasswd.max_users: -1` blocks new registrations — but
/// existing users can still log in. Wired end-to-end through the
/// adduser HTTP endpoint to confirm the policy surfaces as a 403,
/// not a silent 201.
#[tokio::test]
async fn max_users_minus_one_disables_registration_end_to_end() {
    let auth_dir = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let mut config = Config::static_serve(listen(), storage.path().to_path_buf());
    config.auth = AuthConfig {
        htpasswd: HtpasswdConfig {
            file: Some(auth_dir.path().join("htpasswd")),
            max_users: MaxUsers::Disabled,
        },
        tokens: TokensConfig { file: Some(auth_dir.path().join("tokens.db")) },
    };
    let auth = AuthState::load(&config.auth, &config.backend).await.unwrap();
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(put_json("/-/user/org.couchdb.user:newbie", adduser_body("newbie", "anything")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
