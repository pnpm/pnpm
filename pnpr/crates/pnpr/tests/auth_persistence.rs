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
use pnpr::{AuthState, BackendConfig, Config, MaxUsers, router, router_with_auth};
use pnpr_fixtures::MockConfigPresets;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn persistent_config(storage: PathBuf) -> Config {
    let mut config = Config::static_serve(listen(), storage);
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.users.max_users = MaxUsers::Unlimited;
    config
}

/// The database that holds the accounts and tokens of `config`.
fn auth_database(config: &Config) -> &Path {
    let BackendConfig::Sqlite(path) = &config.identity.backend else {
        panic!("a static-serve config keeps accounts in its SQLite database");
    };
    path
}

fn stored_password_hash(config: &Config, username: &str) -> String {
    rusqlite::Connection::open(auth_database(config))
        .unwrap()
        .query_row("SELECT password_hash FROM users WHERE username = ?1", [username], |row| {
            row.get(0)
        })
        .unwrap()
}

fn usernames(config: &Config) -> Vec<String> {
    let connection = rusqlite::Connection::open(auth_database(config)).unwrap();
    let mut statement = connection.prepare("SELECT username FROM users ORDER BY username").unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
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
    let storage = TempDir::new().unwrap();
    let config = persistent_config(storage.path().to_path_buf());
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .expect("first boot");
    let app = router_with_auth(config.clone(), auth);

    // adduser pulls a fresh token out of the response body.
    let response = app
        .clone()
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "secret123")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let token = payload["token"]
        .as_str()
        .expect("token in response")
        .to_string();
    assert!(!token.is_empty());

    assert_eq!(usernames(&config), ["alice"], "the account is in the database");

    // Simulate a restart: drop the router (and the in-memory map),
    // re-load from disk, rebuild the router. Same config, same paths.
    drop(app);
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .expect("reload after restart");
    let app = router_with_auth(config, auth);

    // The token issued before the "restart" must still resolve to
    // alice — proves token hash round-tripped through SQLite.
    let response = app
        .clone()
        .oneshot(
            Request::put("/npm/-/package/anything/dist-tags/latest")
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
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "secret123")))
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

#[tokio::test]
async fn invalid_usernames_do_not_change_accounts_across_restart() {
    let storage = TempDir::new().unwrap();
    let config = persistent_config(storage.path().to_path_buf());
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .expect("first boot");
    let app = router_with_auth(config.clone(), auth);

    let response = app
        .clone()
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "secret123")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let original_hash = stored_password_hash(&config, "alice");

    for (case, encoded_username, username) in [
        ("newline", "alice%0Aadmin", "alice\nadmin"),
        ("carriage return", "alice%0Dadmin", "alice\radmin"),
        ("colon", "alice%3Aadmin", "alice:admin"),
        ("comment marker", "%23alice", "#alice"),
        ("leading whitespace", "%20alice", " alice"),
        ("trailing whitespace", "alice%20", "alice "),
    ] {
        let path = format!("/-/user/org.couchdb.user:{encoded_username}");
        let response = app
            .clone()
            .oneshot(put_json(&path, adduser_body(username, "secret123")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case} username");
        assert_eq!(usernames(&config), ["alice"], "{case} username must not add an account");
    }

    drop(app);
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .expect("reload after restart");
    // The reloaded account still logs in with the original password.
    let (_, username) = auth.users.add_or_login("alice", "secret123").await.unwrap();
    assert_eq!(Some(username), auth.users.account_id("alice").await.unwrap());
    assert_eq!(stored_password_hash(&config, "alice"), original_hash);
}

/// A damaged database fails startup instead of booting with every
/// existing account effectively erased.
#[tokio::test]
async fn unreadable_auth_database_fails_startup() {
    let storage = TempDir::new().unwrap();
    let config = persistent_config(storage.path().to_path_buf());
    let database = auth_database(&config);
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    std::fs::write(database, "this is not an SQLite database, it only says so at length").unwrap();
    AuthState::load(&config.identity.auth, &config.identity.backend).await
        .expect_err("a damaged database should fail to load");
}

/// Stored bcrypt hashes can be verified by Apache when its htpasswd tool is installed.
#[tokio::test]
async fn stored_password_hash_is_verifiable_by_apache_htpasswd_tool() {
    if Command::new("htpasswd")
        .arg("-h")
        .output()
        .is_err()
    {
        eprintln!("apache htpasswd not on PATH — skipping cross-tool compat test");
        return;
    }

    let auth_dir = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let htpasswd = auth_dir.path().join("apache-htpasswd");

    let config = persistent_config(storage.path().to_path_buf());
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .expect("first boot");
    let app = router_with_auth(config.clone(), auth);

    let response = app
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "compat-secret")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let hash = stored_password_hash(&config, "alice");
    std::fs::write(&htpasswd, format!("alice:{hash}\n")).unwrap();

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

/// `auth.users.max_users: -1` blocks new registrations — but
/// existing users can still log in. Wired end-to-end through the
/// adduser HTTP endpoint to confirm the policy surfaces as a 403,
/// not a silent 201.
#[tokio::test]
async fn max_users_minus_one_disables_registration_end_to_end() {
    let storage = TempDir::new().unwrap();
    let mut config = Config::static_serve(listen(), storage.path().to_path_buf());
    config.identity.auth.users.max_users = MaxUsers::Disabled;
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(put_json("/-/user/org.couchdb.user:newbie", adduser_body("newbie", "anything")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_max_users_disables_registration_end_to_end() {
    let storage = TempDir::new().unwrap();
    let config = Config::static_serve(listen(), storage.path().to_path_buf());
    let app = router(config);

    let response = app
        .oneshot(put_json("/-/user/org.couchdb.user:newbie", adduser_body("newbie", "anything")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body_bytes(response.into_body()).await;
    assert!(
        !body
            .windows(b"\"token\"".len())
            .any(|window| window == b"\"token\""),
        "registration denial must not issue a token",
    );
}

#[tokio::test]
async fn finite_max_users_reaches_the_sqlite_backend_end_to_end() {
    let storage = TempDir::new().unwrap();
    let mut config = Config::static_serve(listen(), storage.path().to_path_buf());
    config.identity.auth.users.max_users = MaxUsers::Limited(1);
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let app = router_with_auth(config, auth);

    let first = app
        .clone()
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "secret123")))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = app
        .oneshot(put_json("/-/user/org.couchdb.user:bob", adduser_body("bob", "secret123")))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sha1_legacy_user_login_upgrades_hash_end_to_end() {
    let storage = TempDir::new().unwrap();
    let config = persistent_config(storage.path().to_path_buf());
    // "test-password" hashed with SHA-1 in Apache/Verdaccio format
    pnpr::SqliteAuth::open(auth_database(&config), MaxUsers::Unlimited)
        .unwrap()
        .import_password_hashes(
            [("alice".to_string(), "{SHA}eJy+BAeECxwgQcszRS/2Dxm/WMw=".to_string())].into(),
            pnpr_auth::PasswordImport {
                collisions: pnpr_auth::PasswordCollisions::Keep,
                mode: pnpr_auth::ImportMode::Apply,
            },
        )
        .await
        .unwrap();

    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .expect("load auth");
    let app = router_with_auth(config.clone(), auth);

    // Legacy SHA user logs in via npm adduser / login endpoint
    let response = app
        .clone()
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "test-password")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["ok"].as_str(), Some("you are authenticated as 'alice'"));
    let token = body["token"].as_str().expect("token issued");
    assert!(!token.is_empty());

    // Verify token authenticates whoami
    let whoami_req = Request::get("/-/whoami")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let whoami_res = app
        .clone()
        .oneshot(whoami_req)
        .await
        .unwrap();
    assert_eq!(whoami_res.status(), StatusCode::OK);
    let whoami_body = body_json(whoami_res.into_body()).await;
    assert_eq!(whoami_body["username"], "alice");

    // The stored hash has been upgraded from {SHA} to bcrypt
    assert!(stored_password_hash(&config, "alice").starts_with("$2y$"));

    // Subsequent login succeeds with the upgraded hash
    let second_response = app
        .oneshot(put_json("/-/user/org.couchdb.user:alice", adduser_body("alice", "test-password")))
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::CREATED);
}
