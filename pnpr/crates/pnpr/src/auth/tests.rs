use super::{
    TokenBackend, TokenStore, UpsertOutcome, UserBackend, UserStore, identify, parse_htpasswd,
};
use crate::config::MaxUsers;
use std::sync::Arc;
use tokio::sync::Barrier;

/// Tests run with cost 4 (the bcrypt crate's minimum sane value)
/// so per-test wall-clock stays in the single-digit ms range.
/// Production paths use [`DEFAULT_BCRYPT_COST`].
const TEST_COST: u32 = 4;

fn test_user_store() -> UserStore {
    UserStore {
        users: std::sync::Mutex::new(std::collections::HashMap::new()),
        path: None,
        max_users: MaxUsers::Unlimited,
        bcrypt_cost: TEST_COST,
    }
}

#[tokio::test]
async fn adduser_creates_then_validates() {
    let store = test_user_store();
    let outcome = store.add_or_login("alice", "secret").await.unwrap();
    assert!(matches!(outcome, UpsertOutcome::Created));

    let outcome = store.add_or_login("alice", "secret").await.unwrap();
    assert!(matches!(outcome, UpsertOutcome::LoggedIn));

    assert!(store.verify("alice", "secret").await.is_some());
    assert!(store.verify("alice", "wrong").await.is_none());
    assert!(store.verify("bob", "secret").await.is_none());
}

#[tokio::test]
async fn adduser_rejects_existing_user_with_wrong_password() {
    let store = test_user_store();
    store.add_or_login("alice", "secret").await.unwrap();
    let err = store.add_or_login("alice", "different").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adduser_rejects_same_username_concurrent_registration_with_different_password() {
    let store = Arc::new(UserStore {
        users: std::sync::Mutex::new(std::collections::HashMap::new()),
        path: None,
        max_users: MaxUsers::Unlimited,
        // Higher than TEST_COST so hashing lasts long enough for both
        // tasks to clear the initial missing-user check before either
        // takes the lock — i.e. to actually exercise the race window.
        bcrypt_cost: 8,
    });
    let barrier = Arc::new(Barrier::new(3));

    let spawn_adduser = |password: &'static str| {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            store.add_or_login("alice", password).await
        })
    };

    let add_a = spawn_adduser("pw-a");
    let add_b = spawn_adduser("pw-b");
    barrier.wait().await;

    let result_a = add_a.await.unwrap();
    let result_b = add_b.await.unwrap();
    let created = [result_a.as_ref(), result_b.as_ref()]
        .into_iter()
        .filter(|result| matches!(result, Ok(UpsertOutcome::Created)))
        .count();
    let unauthorized = [&result_a, &result_b]
        .into_iter()
        .filter(|result| {
            result
                .as_ref()
                .err()
                .is_some_and(|err| err.status_code() == axum::http::StatusCode::UNAUTHORIZED)
        })
        .count();

    assert_eq!(created, 1, "exactly one concurrent adduser should create the account");
    assert_eq!(unauthorized, 1, "the losing registration must be rejected");
    assert_ne!(
        store.verify("alice", "pw-a").await.is_some(),
        store.verify("alice", "pw-b").await.is_some(),
    );
}

#[tokio::test]
async fn adduser_persists_across_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");

    let store = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    store.add_or_login("alice", "secret").await.unwrap();
    drop(store);

    // Cold-load from disk; the hashed entry should still verify.
    let reopened = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    let outcome = reopened.add_or_login("alice", "secret").await.unwrap();
    assert!(matches!(outcome, UpsertOutcome::LoggedIn));
    assert!(reopened.verify("alice", "secret").await.is_some());
}

#[tokio::test]
async fn adduser_writes_bcrypt_2y_format() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");
    let store = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    store.add_or_login("alice", "secret").await.unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    let (user, hash) = raw.trim_end().split_once(':').expect("user:hash line");
    assert_eq!(user, "alice");
    assert!(hash.starts_with("$2y$"), "expected $2y$ prefix for htpasswd compat, got {hash:?}");
}

#[tokio::test]
async fn max_users_minus_one_disables_registration() {
    let store = UserStore {
        users: std::sync::Mutex::new(std::collections::HashMap::new()),
        path: None,
        max_users: MaxUsers::Disabled,
        bcrypt_cost: TEST_COST,
    };
    let err = store.add_or_login("alice", "secret").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn max_users_caps_new_registrations() {
    let store = UserStore {
        users: std::sync::Mutex::new(std::collections::HashMap::new()),
        path: None,
        max_users: MaxUsers::Limited(2),
        bcrypt_cost: TEST_COST,
    };
    store.add_or_login("alice", "x").await.unwrap();
    store.add_or_login("bob", "x").await.unwrap();
    let err = store.add_or_login("carol", "x").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::FORBIDDEN);
    // Existing users may still log in once the cap is hit.
    store.add_or_login("alice", "x").await.unwrap();
}

#[tokio::test]
async fn open_rejects_corrupt_htpasswd_at_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");
    std::fs::write(&path, "no-colon-here\nalice:plaintext\n").unwrap();
    let err = UserStore::open(path, MaxUsers::Unlimited).unwrap_err();
    assert!(matches!(err, crate::error::RegistryError::InvalidHtpasswdFile { .. }), "got {err:?}");
}

#[test]
fn parse_htpasswd_accepts_blank_and_comment_lines() {
    let raw = "\n# comment\nalice:$2y$10$abcdef\n";
    let map = parse_htpasswd(raw).unwrap();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("alice"));
}

#[tokio::test]
async fn tokens_round_trip() {
    let tokens = TokenStore::in_memory();
    let token = tokens.issue("alice").await.unwrap();
    assert_eq!(tokens.lookup(&token).await.as_deref(), Some("alice"));
    assert!(tokens.lookup("not-a-token").await.is_none());
}

#[tokio::test]
async fn tokens_are_unique_per_issue() {
    let tokens = TokenStore::in_memory();
    let token_a = tokens.issue("alice").await.unwrap();
    let token_b = tokens.issue("alice").await.unwrap();
    assert_ne!(token_a, token_b, "every call to issue() should mint a fresh token");
}

#[tokio::test]
async fn tokens_persist_across_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");

    let store = TokenStore::open(path.clone()).unwrap();
    let raw = store.issue("alice").await.unwrap();
    drop(store);

    let reopened = TokenStore::open(path).unwrap();
    assert_eq!(
        reopened.lookup(&raw).await.as_deref(),
        Some("alice"),
        "token issued before restart must still resolve after reload",
    );
}

#[tokio::test]
async fn tokens_db_stores_hash_not_raw() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");
    let store = TokenStore::open(path.clone()).unwrap();
    let raw = store.issue("alice").await.unwrap();

    // Open the SQLite file directly and confirm the raw token
    // never appears in any row.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let mut stmt = conn.prepare("SELECT token_hash FROM tokens").unwrap();
    let mut rows = stmt.query([]).unwrap();
    let row = rows.next().unwrap().expect("at least one row");
    let stored: String = row.get(0).unwrap();
    assert_ne!(stored, raw, "raw token must not be persisted");
    assert_eq!(stored.len(), 64, "SHA-256 hex is 64 chars");
}

#[tokio::test]
async fn identify_recognizes_bearer_and_basic() {
    let users = test_user_store();
    users.add_or_login("alice", "secret").await.unwrap();
    let tokens = TokenStore::in_memory();
    let token = tokens.issue("alice").await.unwrap();

    let header = format!("Bearer {token}");
    assert_eq!(identify(Some(&header), &users, &tokens).await.as_deref(), Some("alice"));

    // Basic: base64(alice:secret) = YWxpY2U6c2VjcmV0
    let basic = "Basic YWxpY2U6c2VjcmV0";
    assert_eq!(identify(Some(basic), &users, &tokens).await.as_deref(), Some("alice"));

    let wrong = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"alice:wrong"),
    );
    assert!(identify(Some(&wrong), &users, &tokens).await.is_none());

    assert!(identify(None, &users, &tokens).await.is_none());
    assert!(identify(Some("Bearer total-nonsense"), &users, &tokens).await.is_none());
}

/// RFC 7235 §2.1: "the scheme is case-insensitive". All of
/// `Bearer`, `BEARER`, and `bearer` (and the mixed-case forms
/// some clients emit) must resolve the same way.
#[tokio::test]
async fn identify_parses_auth_scheme_case_insensitively() {
    let users = test_user_store();
    users.add_or_login("alice", "secret").await.unwrap();
    let tokens = TokenStore::in_memory();
    let token = tokens.issue("alice").await.unwrap();

    for scheme in ["Bearer", "bearer", "BEARER", "BeArEr"] {
        let header = format!("{scheme} {token}");
        assert_eq!(
            identify(Some(&header), &users, &tokens).await.as_deref(),
            Some("alice"),
            "Bearer scheme {scheme:?} should be recognized",
        );
    }
    for scheme in ["Basic", "basic", "BASIC", "bAsIc"] {
        let header = format!("{scheme} YWxpY2U6c2VjcmV0");
        assert_eq!(
            identify(Some(&header), &users, &tokens).await.as_deref(),
            Some("alice"),
            "Basic scheme {scheme:?} should be recognized",
        );
    }
}
