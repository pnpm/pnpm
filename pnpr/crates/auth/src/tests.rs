use super::{
    NewToken, OwnedTokens, PasswordChange, TokenBackend, TokenRecord, TokenStore, UpsertOutcome,
    UserBackend, UserCreation, UserRemoval, UserStore, identify, parse_htpasswd, sha256_hex,
};
use crate::validation::{
    MAX_USERNAME_CHARS, token_timestamp_from_sql, token_timestamp_to_sql, validate_username,
};
use pnpr_config::MaxUsers;
use std::sync::Arc;
use tokio::sync::Barrier;

/// Tests run with cost 4 (the bcrypt crate's minimum sane value)
/// so per-test wall-clock stays in the single-digit ms range.
/// Production paths use [`DEFAULT_BCRYPT_COST`].
const TEST_COST: u32 = 4;

const INVALID_USERNAMES: &[&str] = &[
    "",
    " alice",
    "alice ",
    "#alice",
    "alice:bob",
    "alice\nbob",
    "alice\rbob",
    "alice\0bob",
    "alice\u{7f}bob",
];

fn test_user_store() -> UserStore {
    UserStore::in_memory_with_cost(MaxUsers::Unlimited, TEST_COST)
}

#[test]
fn username_length_limit_matches_sql_schema() {
    let max = "a".repeat(MAX_USERNAME_CHARS);
    let too_long = "a".repeat(MAX_USERNAME_CHARS + 1);
    validate_username(&max).unwrap();
    let err = validate_username(&too_long).unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn token_timestamp_from_sql_clamps_negative_values() {
    assert_eq!(token_timestamp_from_sql(-1), 0);
    assert_eq!(token_timestamp_from_sql(0), 0);
    assert_eq!(token_timestamp_from_sql(42), 42);
}

#[test]
fn token_timestamp_to_sql_saturates_overflow() {
    assert_eq!(token_timestamp_to_sql(42), 42);
    assert_eq!(token_timestamp_to_sql(u64::MAX), i64::MAX);
}

#[test]
fn username_validation_rejects_htpasswd_structural_characters() {
    for username in INVALID_USERNAMES {
        let err = validate_username(username).unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
}

#[test]
fn username_validation_rejects_names_that_trim_differently() {
    for username in [" alice", "alice ", " alice "] {
        let err = validate_username(username).unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
}

#[tokio::test]
async fn user_store_rejects_invalid_username_before_persisting() {
    let store = test_user_store();
    for username in INVALID_USERNAMES {
        let err = store.add_or_login(username, "secret").await.unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
    assert!(
        store.is_empty_for_test(),
        "invalid username should be rejected before any persistence",
    );
}

#[tokio::test]
async fn user_store_rejects_invalid_username_before_lookup() {
    let store = test_user_store();
    let hash = bcrypt::hash("secret", TEST_COST).unwrap();
    for username in INVALID_USERNAMES {
        store.insert_user_for_test((*username).to_string(), hash.clone());
    }

    for username in INVALID_USERNAMES {
        let err = store.add_or_login(username, "secret").await.unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected adduser for {username:?} to be rejected",
        );
    }
}

#[tokio::test]
async fn adduser_creates_then_validates() {
    let store = test_user_store();
    let outcome = store.add_or_login("alice", "secret").await.unwrap();
    assert!(matches!(outcome, (UpsertOutcome::Created, _)));
    assert_eq!(outcome.1, "alice");

    let outcome = store.add_or_login("alice", "secret").await.unwrap();
    assert!(matches!(outcome, (UpsertOutcome::LoggedIn, _)));
    assert_eq!(outcome.1, "alice");

    // A wrong password for an existing user is rejected.
    let err = store.add_or_login("alice", "wrong").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
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
    // Higher than TEST_COST so hashing lasts long enough for both
    // tasks to clear the initial missing-user check before either
    // takes the lock — i.e. to actually exercise the race window.
    let store = Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 8));
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
        .filter(|result| matches!(result, Ok((UpsertOutcome::Created, _))))
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
    // Exactly one of the two passwords is the one that was stored: logging in
    // with it succeeds, the other is rejected as unauthorized.
    let login_a = store.add_or_login("alice", "pw-a").await;
    let login_b = store.add_or_login("alice", "pw-b").await;
    let logged_in = [login_a.as_ref(), login_b.as_ref()]
        .into_iter()
        .filter(|result| matches!(result, Ok((UpsertOutcome::LoggedIn, _))))
        .count();
    let unauthorized_logins = [&login_a, &login_b]
        .into_iter()
        .filter(|result| {
            result
                .as_ref()
                .err()
                .is_some_and(|err| err.status_code() == axum::http::StatusCode::UNAUTHORIZED)
        })
        .count();

    assert_eq!(logged_in, 1, "exactly one password must be the stored one");
    assert_eq!(unauthorized_logins, 1, "the other password must be unauthorized");
}

#[tokio::test]
async fn adduser_persists_across_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");

    let store = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    store.add_or_login("alice", "secret").await.unwrap();
    drop(store);

    // Cold-load from disk; the hashed entry should still log in.
    let reopened = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    let outcome = reopened.add_or_login("alice", "secret").await.unwrap();
    assert!(matches!(outcome, (UpsertOutcome::LoggedIn, _)));
}

#[tokio::test]
async fn adduser_writes_bcrypt_2y_format() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");
    let store = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    store.add_or_login("alice", "secret").await.unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    let (user, hash) = raw
        .trim_end()
        .split_once(':')
        .expect("user:hash line");
    assert_eq!(user, "alice");
    assert!(hash.starts_with("$2y$"), "expected $2y$ prefix for htpasswd compat, got {hash:?}");
}

#[tokio::test]
async fn max_users_minus_one_disables_registration() {
    let store = UserStore::in_memory_with_cost(MaxUsers::Disabled, TEST_COST);
    let err = store.add_or_login("alice", "secret").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn in_memory_store_honors_the_registration_cap() {
    // An unset `auth.htpasswd.file` must not re-open sign-ups that the
    // configured cap denied: the in-memory store enforces it too.
    let store = UserStore::in_memory_with_max_users(MaxUsers::Disabled);
    let err = store.add_or_login("alice", "secret").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn max_users_caps_new_registrations() {
    let store = UserStore::in_memory_with_cost(MaxUsers::Limited(2), TEST_COST);
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
    assert!(matches!(err, pnpr_error::RegistryError::InvalidHtpasswdFile { .. }), "got {err:?}");
}

#[test]
fn parse_htpasswd_accepts_blank_and_comment_lines() {
    let raw = "\n# comment\nalice:$2y$10$abcdef\n";
    let map = parse_htpasswd(raw).unwrap();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("alice"));
}

#[test]
fn parse_htpasswd_preserves_legacy_whitespace_normalization() {
    let map = parse_htpasswd(" alice :$2y$10$abcdef\n").unwrap();
    assert_eq!(map.get("alice").map(String::as_str), Some("$2y$10$abcdef"));
}

#[test]
fn parse_htpasswd_rejects_invalid_usernames() {
    for raw in [" #alice:$2y$10$abcdef\n", "alice\u{1}admin:$2y$10$abcdef\n"] {
        assert!(parse_htpasswd(raw).is_err(), "expected {raw:?} to be rejected");
    }
}

#[tokio::test]
async fn tokens_round_trip() {
    let tokens = TokenStore::in_memory();
    let token = tokens.issue("alice").await.unwrap();
    assert_eq!(
        tokens
            .lookup(&token)
            .await
            .unwrap()
            .as_deref(),
        Some("alice"),
    );
    assert!(
        tokens
            .lookup("not-a-token")
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn lookup_record_surfaces_token_restrictions() {
    let tokens = TokenStore::in_memory();
    let raw = "restricted-token";
    tokens.inner
        .lock()
        .expect("TokenStore mutex poisoned")
        .insert(
            sha256_hex(raw.as_bytes()),
            TokenRecord::new("alice", 1, 1)
                .with_readonly(true)
                .with_cidr_whitelist(vec!["203.0.113.0/24".to_string()]),
        );

    let record = tokens
        .lookup_record(raw)
        .await
        .unwrap()
        .expect("seeded token resolves");
    assert_eq!(record.username, "alice");
    assert!(record.readonly, "readonly flag must survive the lookup");
    assert_eq!(record.cidr_whitelist, vec!["203.0.113.0/24".to_string()]);

    assert!(
        tokens
            .lookup_record("not-a-token")
            .await
            .unwrap()
            .is_none(),
        "an unknown token resolves to no record",
    );
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
    store.check_readiness().await.unwrap();
    let raw = store.issue("alice").await.unwrap();
    drop(store);

    let reopened = TokenStore::open(path.clone()).unwrap();
    reopened.check_readiness().await.unwrap();
    assert_eq!(
        reopened
            .lookup(&raw)
            .await
            .unwrap()
            .as_deref(),
        Some("alice"),
        "token issued before restart must still resolve after reload",
    );

    std::fs::remove_file(&path).unwrap();
    assert!(reopened.check_readiness().await.is_err());
    assert!(!path.exists(), "check_readiness must not recreate missing database");
}

#[tokio::test]
async fn tokens_issue_with_options_persists_and_expires() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");

    let store = TokenStore::open(path.clone()).unwrap();
    let (raw, record) = store
        .issue_token(
            "alice",
            NewToken::new()
                .with_readonly(true)
                .with_cidr_whitelist(vec!["127.0.0.1/32".to_string()])
                .with_name(Some("ci-token".to_string()))
                .with_expires_at(Some(2_000_000_000)),
        )
        .await
        .unwrap();
    assert_eq!(record.username, "alice");
    assert!(record.readonly);
    assert_eq!(record.name.as_deref(), Some("ci-token"));
    assert_eq!(record.expires_at, Some(2_000_000_000));
    drop(store);

    let reopened = TokenStore::open(path.clone()).unwrap();
    let looked_up = reopened
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(looked_up.username, "alice");
    assert!(looked_up.readonly);
    assert_eq!(looked_up.cidr_whitelist, vec!["127.0.0.1/32".to_string()]);
    assert_eq!(looked_up.name.as_deref(), Some("ci-token"));
    assert_eq!(looked_up.expires_at, Some(2_000_000_000));
}

#[tokio::test]
async fn tokens_expired_do_not_authenticate() {
    let store = TokenStore::in_memory();
    let (raw, _) = store
        .issue_token("alice", NewToken::new().with_expires_at(Some(1)))
        .await
        .unwrap();

    assert!(
        store
            .lookup(&raw)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        store
            .lookup_record(&raw)
            .await
            .unwrap()
            .is_none(),
    );
}

#[tokio::test]
async fn tokens_touch_updates_last_used_at() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");
    let store = TokenStore::open(path.clone()).unwrap();
    let raw = store.issue("alice").await.unwrap();
    let record_before = store
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();

    store
        .touch(&raw, record_before.last_used_at + 100)
        .await
        .unwrap();
    let record_after = store
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record_after.last_used_at, record_before.last_used_at + 100);

    drop(store);
    let reopened = TokenStore::open(path).unwrap();
    let record_reopened = reopened
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record_reopened.last_used_at, record_before.last_used_at + 100);
}

#[tokio::test]
async fn tokens_clamp_negative_persisted_timestamps() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    crate::token_store::sqlite::init_tokens_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO tokens
         (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params!["token-hash", "alice", -1_i64, -42_i64, 0_i64, "[]"],
    )
    .unwrap();
    drop(conn);

    let store = TokenStore::open(path).unwrap();
    let records = store.list_for_user("alice").await.unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].1.created_at, 0);
    assert_eq!(records[0].1.last_used_at, 0);
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
    let row = rows
        .next()
        .unwrap()
        .expect("at least one row");
    let stored: String = row.get(0).unwrap();
    assert_ne!(stored, raw, "raw token must not be persisted");
    assert_eq!(stored.len(), 64, "SHA-256 hex is 64 chars");
}

#[tokio::test]
async fn token_issue_rolls_back_memory_when_sqlite_persistence_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");
    let store = TokenStore::open(path.clone()).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("DROP TABLE tokens", [])
        .unwrap();

    let err = store.issue("alice").await.unwrap_err();

    assert_eq!(err.status_code(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        store.inner
            .lock()
            .expect("TokenStore mutex poisoned")
            .is_empty(),
        "failed persistence must not leave an in-memory bearer token active",
    );
}

#[tokio::test]
async fn identify_recognizes_bearer_and_ignores_basic() {
    let tokens = TokenStore::in_memory();
    let token = tokens.issue("alice").await.unwrap();

    let header = format!("Bearer {token}");
    assert_eq!(identify(Some(&header), &tokens).await.unwrap().as_deref(), Some("alice"));

    // Basic credentials are not accepted on requests — the header is
    // ignored (treated as anonymous), so request handling never pays a bcrypt.
    let basic = "Basic YWxpY2U6c2VjcmV0";
    assert!(identify(Some(basic), &tokens).await.unwrap().is_none());

    assert!(identify(None, &tokens).await.unwrap().is_none());
    assert!(identify(Some("Bearer total-nonsense"), &tokens).await.unwrap().is_none());
}

/// RFC 7235 §2.1: "the scheme is case-insensitive". All of
/// `Bearer`, `BEARER`, and `bearer` (and the mixed-case forms
/// some clients emit) must resolve the same way; `Basic` is ignored in any
/// case.
#[tokio::test]
async fn identify_parses_auth_scheme_case_insensitively() {
    let tokens = TokenStore::in_memory();
    let token = tokens.issue("alice").await.unwrap();

    for scheme in ["Bearer", "bearer", "BEARER", "BeArEr"] {
        let header = format!("{scheme} {token}");
        assert_eq!(
            identify(Some(&header), &tokens).await.unwrap().as_deref(),
            Some("alice"),
            "Bearer scheme {scheme:?} should be recognized",
        );
    }
    for scheme in ["Basic", "basic", "BASIC", "bAsIc"] {
        let header = format!("{scheme} YWxpY2U6c2VjcmV0");
        assert!(
            identify(Some(&header), &tokens).await.unwrap().is_none(),
            "Basic scheme {scheme:?} must be ignored",
        );
    }
}

/// An empty `cidr_whitelist` means the token carries no address restriction,
/// so silently defaulting a malformed one to empty would turn a CIDR-confined
/// token into an unrestricted one — the wrong direction to fail. Refuse the
/// store instead.
#[tokio::test]
async fn an_unreadable_cidr_whitelist_is_refused_rather_than_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    crate::token_store::sqlite::init_tokens_schema(&conn).unwrap();
    conn.execute(
        "INSERT INTO tokens
         (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params!["token-hash", "alice", 0_i64, 0_i64, 0_i64, "{ not json"],
    )
    .unwrap();
    drop(conn);

    let err = TokenStore::open(path).expect_err("a corrupt cidr_whitelist must not load");
    let message = err.to_string();
    assert!(message.contains("cidr_whitelist"), "the error should name the column: {message}");
    assert!(
        message.contains("token-hash"),
        "the error should name the row, so an operator can find it: {message}",
    );
}

/// The admin operations every [`UserBackend`] supports, starting from an
/// empty store.
pub(crate) async fn assert_admin_round_trip(users: &dyn UserBackend) {
    assert_eq!(users.create_user("alice", "first").await.unwrap(), UserCreation::Created);
    assert_eq!(users.create_user("alice", "other").await.unwrap(), UserCreation::NameTaken);
    assert_eq!(users.list_users().await.unwrap(), ["alice"]);

    assert_eq!(users.set_password("alice", "second").await.unwrap(), PasswordChange::Changed);
    assert!(users.add_or_login("alice", "first").await.is_err());
    assert!(matches!(
        users.add_or_login("alice", "second").await.unwrap(),
        (UpsertOutcome::LoggedIn, _),
    ));
    assert_eq!(users.set_password("bob", "x").await.unwrap(), PasswordChange::NoSuchUser);

    assert_eq!(users.delete_user("alice").await.unwrap(), UserRemoval::Removed);
    assert_eq!(users.delete_user("alice").await.unwrap(), UserRemoval::NoSuchUser);
    assert!(
        users
            .list_users()
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn user_store_supports_the_admin_operations() {
    assert_admin_round_trip(&test_user_store()).await;
}

#[tokio::test]
async fn admin_created_users_bypass_disabled_registration_and_persist() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");
    let store = UserStore::open_with_cost(path.clone(), MaxUsers::Disabled, TEST_COST).unwrap();
    assert_eq!(store.create_user("alice", "secret").await.unwrap(), UserCreation::Created);
    assert_eq!(store.create_user("bob", "secret").await.unwrap(), UserCreation::Created);
    assert_eq!(store.delete_user("bob").await.unwrap(), UserRemoval::Removed);

    let reopened = UserStore::open_with_cost(path, MaxUsers::Disabled, TEST_COST).unwrap();
    assert_eq!(reopened.list_users().await.unwrap(), ["alice"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_edits_leave_the_file_matching_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");
    let store =
        Arc::new(UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap());
    let mut edits = tokio::task::JoinSet::new();
    for index in 0..32 {
        let store = Arc::clone(&store);
        edits.spawn(async move {
            let name = format!("user{index}");
            store.create_user(&name, "x").await.unwrap();
            if index % 2 == 0 {
                store.delete_user(&name).await.unwrap();
            }
        });
    }
    edits.join_all().await;

    let reopened = UserStore::open_with_cost(path, MaxUsers::Unlimited, TEST_COST).unwrap();
    assert_eq!(reopened.list_users().await.unwrap(), store.list_users().await.unwrap());
}

#[tokio::test]
async fn a_failed_write_changes_nothing_and_can_be_repeated() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("htpasswd");
    let store = UserStore::open_with_cost(path.clone(), MaxUsers::Unlimited, TEST_COST).unwrap();
    // A directory where the file belongs makes the write's rename fail.
    std::fs::create_dir(&path).unwrap();

    assert!(store.create_user("alice", "x").await.is_err());
    assert!(
        store
            .list_users()
            .await
            .unwrap()
            .is_empty(),
    );

    std::fs::remove_dir(&path).unwrap();
    assert_eq!(store.create_user("alice", "x").await.unwrap(), UserCreation::Created);
    let reopened = UserStore::open_with_cost(path, MaxUsers::Unlimited, TEST_COST).unwrap();
    assert_eq!(reopened.list_users().await.unwrap(), ["alice"]);
}

#[tokio::test]
async fn local_tokens_of_a_removed_owner_stop_authenticating() {
    let users = Arc::new(test_user_store());
    let tokens = OwnedTokens::new(TokenStore::in_memory(), Arc::clone(&users));
    users.create_user("alice", "x").await.unwrap();
    let token = tokens.issue("alice").await.unwrap();
    assert_eq!(
        tokens
            .lookup(&token)
            .await
            .unwrap()
            .as_deref(),
        Some("alice"),
    );

    users.delete_user("alice").await.unwrap();
    assert!(
        tokens
            .lookup(&token)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        tokens
            .lookup_record(&token)
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        tokens
            .list_for_user("alice")
            .await
            .unwrap()
            .len(),
        1,
    );
}

#[tokio::test]
async fn tokens_list_per_owner_across_revocation_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tokens.db");
    let store = TokenStore::open(path.clone()).unwrap();
    let alice = store.issue("alice").await.unwrap();
    store.issue("alice").await.unwrap();
    store.issue("bob").await.unwrap();
    assert_eq!(
        store
            .list_for_user("alice")
            .await
            .unwrap()
            .len(),
        2,
    );

    store
        .revoke_by_key(&sha256_hex(alice.as_bytes()))
        .await
        .unwrap();
    assert_eq!(
        store
            .list_for_user("alice")
            .await
            .unwrap()
            .len(),
        1,
    );

    let reopened = TokenStore::open(path).unwrap();
    assert_eq!(
        reopened
            .list_for_user("alice")
            .await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        reopened
            .list_for_user("bob")
            .await
            .unwrap()
            .len(),
        1,
    );
    assert!(
        reopened
            .list_for_user("carol")
            .await
            .unwrap()
            .is_empty(),
    );
}

#[cfg(unix)]
#[test]
fn write_atomic_preserves_file_permissions_and_defaults_to_0600() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("htpasswd");

    super::write_atomic(&file, b"user1:$2y$04$test\n").unwrap();
    let perms = std::fs::metadata(&file)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(perms, 0o600);

    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    super::write_atomic(&file, b"user1:$2y$04$test\nuser2:$2y$04$test2\n").unwrap();
    let perms = std::fs::metadata(&file)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(perms, 0o640);
}

#[cfg(unix)]
#[test]
fn token_store_sets_0600_permissions_on_sqlite_file() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tokens.db");
    let _store = TokenStore::open(path.clone()).unwrap();

    let perms = std::fs::metadata(&path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(perms, 0o600);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let _store2 = TokenStore::open(path.clone()).unwrap();
    let perms2 = std::fs::metadata(&path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(perms2, 0o640);
}

#[tokio::test]
async fn verify_bcrypt_handles_passwords_exceeding_72_bytes() {
    let password_71 = "a".repeat(71);
    let hash = super::hash_bcrypt(password_71.clone(), TEST_COST).await.unwrap();

    assert!(super::verify_bcrypt(password_71, hash.clone()).await.unwrap());

    // 72-byte password must safely evaluate to false without error or truncation-match.
    let password_72 = "a".repeat(72);
    assert!(!super::verify_bcrypt(password_72, hash).await.unwrap());
}

#[test]
fn parse_htpasswd_accepts_sha1_entries() {
    let raw = "alice:{SHA}qUqP5cyxm6YcTAhz05Hph5gvu9M=\nbob:$2y$04$somevalidhashhere12345678901234567890123456789012\n";
    let users = super::parse_htpasswd(raw).unwrap();
    assert_eq!(users.get("alice"), Some(&"{SHA}qUqP5cyxm6YcTAhz05Hph5gvu9M=".to_string()));
    assert!(users.contains_key("bob"));
}

#[tokio::test]
async fn sha1_user_authenticates_and_upgrades_to_bcrypt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("htpasswd");
    // "test" hashed with SHA-1 in Apache/Verdaccio format
    std::fs::write(&path, "verdaccio_user:{SHA}qUqP5cyxm6YcTAhz05Hph5gvu9M=\n").unwrap();

    let store =
        UserStore::open_with_cost(path.clone(), pnpr_config::MaxUsers::Unlimited, TEST_COST)
            .unwrap();

    // Wrong password fails
    let err = store.add_or_login("verdaccio_user", "wrong").await.unwrap_err();
    assert!(matches!(err, pnpr_error::RegistryError::Unauthenticated { .. }));

    // Correct password succeeds
    let (outcome, user) = store.add_or_login("verdaccio_user", "test").await.unwrap();
    assert_eq!(outcome, super::UpsertOutcome::LoggedIn);
    assert_eq!(user, "verdaccio_user");

    // File on disk is now upgraded to bcrypt
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("{SHA}"));
    assert!(content.contains("verdaccio_user:$2y$"));

    // Verify returning user can log in with new bcrypt hash
    let (outcome2, user2) = store.add_or_login("verdaccio_user", "test").await.unwrap();
    assert_eq!(outcome2, super::UpsertOutcome::LoggedIn);
    assert_eq!(user2, "verdaccio_user");
}

#[tokio::test]
async fn schema_migrations_runs_on_empty_sqlite_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");
    let _store = TokenStore::open(path.clone()).unwrap();

    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("SELECT MAX(version) FROM auth_schema_migrations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, super::CURRENT_SCHEMA_VERSION);

    // Verify all columns exist in tokens table
    let mut stmt = conn.prepare("PRAGMA table_info(tokens)").unwrap();
    let mut rows = stmt.query([]).unwrap();
    let mut columns = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        let name: String = row.get(1).unwrap();
        columns.push(name);
    }
    assert!(columns.contains(&"scopes".to_string()));
    assert!(columns.contains(&"kind".to_string()));
    assert!(columns.contains(&"name".to_string()));
    assert!(columns.contains(&"expires_at".to_string()));
}

#[tokio::test]
async fn schema_migrations_upgrades_legacy_sqlite_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");

    // Create legacy table without auth_schema_migrations, scopes, or kind
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tokens (
                token_hash      CHAR(64) PRIMARY KEY,
                username        VARCHAR(255) NOT NULL,
                created_at      BIGINT NOT NULL,
                last_used_at    BIGINT NOT NULL,
                readonly        SMALLINT NOT NULL DEFAULT 0,
                cidr_whitelist  VARCHAR(4096) NOT NULL DEFAULT '[]',
                name            VARCHAR(255),
                expires_at      BIGINT
            );
            INSERT INTO tokens (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
            VALUES ('deadbeef', 'alice', 1000, 1000, 0, '[]');",
        )
        .unwrap();
    }

    // Opening TokenStore must migrate schema to CURRENT_SCHEMA_VERSION
    let store = TokenStore::open(path.clone()).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("SELECT MAX(version) FROM auth_schema_migrations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, super::CURRENT_SCHEMA_VERSION);

    // Legacy record is preserved with None scopes and kind
    let record = store
        .find_by_key("deadbeef")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.username, "alice");
    assert_eq!(record.scopes, None);
    assert_eq!(record.kind, None);
}

#[tokio::test]
async fn schema_migrations_refuses_future_schema_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE auth_schema_migrations (version INTEGER PRIMARY KEY, applied_at BIGINT NOT NULL);
             INSERT INTO auth_schema_migrations (version, applied_at) VALUES (999, 123456789);",
        )
        .unwrap();
    }

    let err = TokenStore::open(path).unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("newer than supported version"),
        "expected error about newer version, got: {err_str}",
    );
}

#[tokio::test]
async fn tokens_scopes_and_kind_persist_across_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tokens.db");

    let store = TokenStore::open(path.clone()).unwrap();
    let (raw, record) = store
        .issue_token(
            "alice",
            NewToken::new()
                .with_scopes(Some(vec!["scope:read".to_string(), "scope:write".to_string()]))
                .with_kind(Some("granular".to_string())),
        )
        .await
        .unwrap();
    assert_eq!(record.scopes, Some(vec!["scope:read".to_string(), "scope:write".to_string()]));
    assert_eq!(record.kind.as_deref(), Some("granular"));
    drop(store);

    let reopened = TokenStore::open(path).unwrap();
    let looked_up = reopened
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(looked_up.scopes, Some(vec!["scope:read".to_string(), "scope:write".to_string()]));
    assert_eq!(looked_up.kind.as_deref(), Some("granular"));
}

#[tokio::test]
async fn file_backed_user_store_picks_up_external_edits() {
    let dir = tempfile::tempdir().unwrap();
    let htpasswd_path = dir.path().join("htpasswd");
    let store1 = UserStore::open(htpasswd_path.clone(), MaxUsers::Unlimited).unwrap();
    let store2 = UserStore::open(htpasswd_path, MaxUsers::Unlimited).unwrap();

    assert_eq!(store1.create_user("alice", "pass1").await.unwrap(), UserCreation::Created);
    // store2 (simulating a running server or other process) should observe alice
    assert!(
        store2
            .password_hash("alice")
            .await
            .unwrap()
            .is_some(),
    );

    // store2 updates alice's password
    assert_eq!(store2.set_password("alice", "pass2").await.unwrap(), PasswordChange::Changed);
    // store1 adds bob
    assert_eq!(store1.create_user("bob", "pass3").await.unwrap(), UserCreation::Created);

    // Both stores should observe both alice and bob without losing either edit
    assert!(
        store2
            .password_hash("bob")
            .await
            .unwrap()
            .is_some(),
    );
    assert!(
        store1
            .password_hash("alice")
            .await
            .unwrap()
            .is_some(),
    );
}

#[tokio::test]
async fn holds_fails_closed_on_corrupt_htpasswd_and_reflects_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let htpasswd_path = dir.path().join("htpasswd");
    let store = UserStore::open(htpasswd_path.clone(), MaxUsers::Unlimited).unwrap();

    assert_eq!(store.create_user("alice", "pass1").await.unwrap(), UserCreation::Created);
    assert!(store.holds("alice").unwrap());
    assert!(!store.holds("bob").unwrap());

    // Corrupt the htpasswd file
    std::fs::write(&htpasswd_path, b"invalid htpasswd line without colon\n").unwrap();
    assert!(store.holds("alice").is_err(), "must fail closed on corrupt htpasswd");

    // Delete the htpasswd file: accounts are cleared
    std::fs::remove_file(&htpasswd_path).unwrap();
    assert!(!store.holds("alice").unwrap(), "deleted file must reflect as empty store");
}

#[tokio::test]
async fn concurrent_lookups_during_removal_and_replacement_do_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let htpasswd_path = dir.path().join("htpasswd");
    let store =
        std::sync::Arc::new(UserStore::open(htpasswd_path.clone(), MaxUsers::Unlimited).unwrap());

    assert_eq!(store.create_user("alice", "pass1").await.unwrap(), UserCreation::Created);

    let mut handles = Vec::new();
    for index in 0..10 {
        let store = std::sync::Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            for _ in 0..20 {
                let _ = store.holds("alice");
                let _ = store.password_hash("alice").await;
                let _ = store.list_users().await;
                tokio::time::sleep(std::time::Duration::from_millis((index % 3) + 1)).await;
            }
        }));
    }

    let replacer = tokio::spawn(async move {
        for round in 0..10 {
            let _ = std::fs::remove_file(&htpasswd_path);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            let hash = bcrypt::hash(format!("pass{round}"), TEST_COST).unwrap();
            std::fs::write(&htpasswd_path, format!("alice:{hash}\n")).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    });

    for handle in handles {
        handle.await.unwrap();
    }
    replacer.await.unwrap();

    assert!(store.holds("alice").unwrap());
    assert!(
        store
            .password_hash("alice")
            .await
            .unwrap()
            .is_some(),
    );
    assert_eq!(store.list_users().await.unwrap(), vec!["alice".to_string()]);
}

#[tokio::test]
async fn same_size_replacement_reloads_users_even_if_timestamp_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let htpasswd_path = dir.path().join("htpasswd");
    let store = UserStore::open(htpasswd_path.clone(), MaxUsers::Unlimited).unwrap();

    let hash_alice = bcrypt::hash("pass_alice", TEST_COST).unwrap();
    let initial_content = format!("alice:{hash_alice}\n");
    std::fs::write(&htpasswd_path, &initial_content).unwrap();

    assert!(store.holds("alice").unwrap());

    let orig_mtime = std::fs::metadata(&htpasswd_path)
        .unwrap()
        .modified()
        .unwrap();

    let hash_bob = bcrypt::hash("pass_bob00", TEST_COST).unwrap();
    let replacement = format!("bob00:{hash_bob}\n");
    assert_eq!(initial_content.len(), replacement.len());

    let tmp_path = dir.path().join("htpasswd.tmp");
    std::fs::write(&tmp_path, &replacement).unwrap();
    let tmp_file = std::fs::File::options()
        .write(true)
        .open(&tmp_path)
        .unwrap();
    tmp_file.set_modified(orig_mtime).unwrap();
    drop(tmp_file);
    std::fs::rename(&tmp_path, &htpasswd_path).unwrap();

    assert!(store.holds("bob00").unwrap());
    assert!(!store.holds("alice").unwrap());
}
