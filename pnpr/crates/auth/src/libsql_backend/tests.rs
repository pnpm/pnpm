use super::{
    Builder, Duration, LibsqlAuth, MaxUsers, RegistryError, Result, TokenBackend, UpsertOutcome,
    UserBackend, params, retry_database_conflicts,
    schema::{ensure_user_counter, is_transaction_conflict},
    sha256_hex, with_auth_timeout,
};

/// In-memory libsql database, exercising the same driver and SQL the
/// networked backend uses without a server.
async fn local_backend(max_users: MaxUsers) -> LibsqlAuth {
    let db = Builder::new_local(":memory:").build().await.unwrap();
    LibsqlAuth::from_database(db, max_users).await.unwrap()
}

#[tokio::test]
async fn with_auth_timeout_surfaces_auth_database_timeout_when_a_read_stalls() {
    let result: Result<()> = with_auth_timeout(Duration::from_millis(5), async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok::<(), RegistryError>(())
    })
    .await;
    assert!(matches!(result, Err(RegistryError::AuthDatabaseTimeout)));
}

#[tokio::test]
async fn with_auth_timeout_passes_a_fast_read_through() {
    let result: Result<u32> =
        with_auth_timeout(Duration::from_secs(30), async { Ok::<_, RegistryError>(7) }).await;
    assert_eq!(result.unwrap(), 7);
}

#[tokio::test]
async fn add_or_login_creates_then_logs_in() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    assert!(matches!(
        backend.add_or_login("alice", "secret").await.unwrap(),
        (UpsertOutcome::Created, _),
    ));
    assert!(matches!(
        backend.add_or_login("alice", "secret").await.unwrap(),
        (UpsertOutcome::LoggedIn, _),
    ));
}

#[tokio::test]
async fn add_or_login_rejects_existing_user_with_wrong_password() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    backend.add_or_login("alice", "secret").await.unwrap();
    let err = backend.add_or_login("alice", "different").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn max_users_disabled_rejects_registration() {
    let backend = local_backend(MaxUsers::Disabled).await;
    let err = backend.add_or_login("alice", "x").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn add_or_login_rejects_invalid_username_before_insert() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    let err = backend.add_or_login("alice ", "secret").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);

    let mut rows = backend.conn.query("SELECT COUNT(*) FROM users", ()).await.unwrap();
    let total: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(total, 0, "invalid username must not be inserted");
}

#[tokio::test]
async fn add_or_login_rejects_existing_invalid_username() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    let hash = bcrypt::hash("secret", 4).unwrap();
    backend
        .conn
        .execute(
            "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
            params!["alice ", hash],
        )
        .await
        .unwrap();

    let err = backend.add_or_login("alice ", "secret").await.unwrap_err();

    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn add_or_login_propagates_corrupt_hash_errors() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    backend
        .conn
        .execute(
            "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
            params!["alice", "not-a-bcrypt-hash"],
        )
        .await
        .unwrap();

    let err = backend.add_or_login("alice", "secret").await.unwrap_err();

    assert!(matches!(err, RegistryError::Bcrypt(_)), "got {err:?}");
}

#[tokio::test]
async fn max_users_caps_registration() {
    let backend = local_backend(MaxUsers::Limited(1)).await;
    backend.add_or_login("alice", "x").await.unwrap();
    let err = backend.add_or_login("bob", "x").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::FORBIDDEN);
    // The capped-out registrant can't sneak in, but existing users
    // still log in.
    backend.add_or_login("alice", "x").await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registration_cap_is_strict_under_concurrency() {
    let backend = std::sync::Arc::new(local_backend(MaxUsers::Limited(1)).await);
    let mut handles = Vec::new();
    for index in 0..6 {
        let backend = std::sync::Arc::clone(&backend);
        handles.push(tokio::spawn(async move {
            backend.add_or_login(&format!("user{index}"), "x").await
        }));
    }
    let mut created = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok((UpsertOutcome::Created, _)) => created += 1,
            Err(RegistryError::TooManyUsers { max: 1 }) => {}
            other => panic!("unexpected concurrent registration result: {other:?}"),
        }
    }
    assert_eq!(created, 1, "exactly one registration may win the cap of 1");

    let mut rows = backend.conn.query("SELECT COUNT(*) FROM users", ()).await.unwrap();
    let total: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(total, 1, "the cap must be strictly enforced, never exceeded");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registration_cap_is_strict_across_backend_instances() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.db");
    let mut backends = Vec::new();
    for _ in 0..6 {
        let db = Builder::new_local(&path).build().await.unwrap();
        backends.push(LibsqlAuth::from_database(db, MaxUsers::Limited(1)).await.unwrap());
    }
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(backends.len()));
    let mut handles = Vec::new();
    for (index, backend) in backends.into_iter().enumerate() {
        let barrier = std::sync::Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            backend.add_or_login(&format!("user{index}"), "x").await
        }));
    }
    let mut created = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok((UpsertOutcome::Created, _)) => created += 1,
            Err(RegistryError::TooManyUsers { max: 1 }) => {}
            other => panic!("unexpected concurrent registration result: {other:?}"),
        }
    }
    assert_eq!(created, 1);
    let db = Builder::new_local(&path).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Limited(1)).await.unwrap();
    assert_eq!(backend.user_count().await.unwrap(), 1);
    let mut rows = backend
        .conn
        .query("SELECT value FROM auth_counters WHERE name = 'users'", ())
        .await
        .unwrap();
    let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn ensure_user_counter_reconciles_a_stale_counter() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    backend
        .conn
        .execute(
            "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
            params!["alice", "not-used-by-this-test"],
        )
        .await
        .unwrap();
    backend
        .conn
        .execute("UPDATE auth_counters SET value = 0 WHERE name = ?1", params!["users"])
        .await
        .unwrap();

    ensure_user_counter(&backend.conn).await.unwrap();

    let mut rows = backend
        .conn
        .query("SELECT value FROM auth_counters WHERE name = ?1", params!["users"])
        .await
        .unwrap();
    let value: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(value, 1, "startup reconciliation must lift stale counters to the user count");
}

#[tokio::test]
async fn registration_cap_self_heals_an_overcounted_counter() {
    let backend = local_backend(MaxUsers::Limited(1)).await;
    backend.add_or_login("alice", "x").await.unwrap();
    backend.conn.execute("DELETE FROM users WHERE username = ?1", params!["alice"]).await.unwrap();

    assert!(
        matches!(backend.add_or_login("bob", "x").await.unwrap(), (UpsertOutcome::Created, _),),
    );

    let mut rows = backend
        .conn
        .query("SELECT value FROM auth_counters WHERE name = ?1", params!["users"])
        .await
        .unwrap();
    let value: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(value, 1, "counter should match the newly created user");
}

#[tokio::test]
async fn tokens_round_trip_and_revoke() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    let token = backend.issue("alice").await.unwrap();
    assert_eq!(backend.lookup(&token).await.unwrap().as_deref(), Some("alice"));
    assert!(backend.lookup("not-a-token").await.unwrap().is_none());

    let key = sha256_hex(token.as_bytes());
    let listed = backend.list_for_user("alice").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, key);
    assert_eq!(listed[0].1.username, "alice");

    assert!(backend.revoke_by_key(&key).await.unwrap().is_some());
    assert!(backend.lookup(&token).await.unwrap().is_none());
    assert!(backend.revoke_by_key(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn tokens_store_hash_not_raw() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    let raw = backend.issue("alice").await.unwrap();
    let mut rows = backend.conn.query("SELECT token_hash FROM tokens", ()).await.unwrap();
    let row = rows.next().await.unwrap().expect("one token row");
    let stored: String = row.get(0).unwrap();
    assert_ne!(stored, raw, "raw token must not be persisted");
    assert_eq!(stored.len(), 64, "SHA-256 hex is 64 chars");
}

/// A store failure must surface as `Err`, never a silent `Ok(None)` —
/// otherwise a database outage would read as "token not found" and the
/// caller would answer 401 instead of 5xx.
#[tokio::test]
async fn reads_propagate_a_backend_error_instead_of_swallowing_it() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    backend.issue("alice").await.unwrap();
    // Break the store out from under the reads: a query against a
    // dropped table errors rather than returning an empty result.
    backend.conn.execute("DROP TABLE tokens", ()).await.unwrap();
    assert!(backend.lookup("anything").await.is_err());
    assert!(backend.find_by_key("anything").await.is_err());
    assert!(backend.list_for_user("alice").await.is_err());
}

#[tokio::test]
async fn registration_waits_for_another_database_writer() {
    let directory = tempfile::tempdir().unwrap();
    let db = Builder::new_local(directory.path().join("auth.db")).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Limited(1)).await.unwrap();
    let other_db = Builder::new_local(directory.path().join("auth.db")).build().await.unwrap();
    let other = other_db.connect().unwrap();
    let writer =
        other.transaction_with_behavior(libsql::TransactionBehavior::Immediate).await.unwrap();
    let pending = begin_registration_transaction(&backend.conn);
    tokio::pin!(pending);
    assert!(tokio::time::timeout(Duration::from_millis(20), &mut pending).await.is_err());
    writer.rollback().await.unwrap();
    pending.await.unwrap().rollback().await.unwrap();
}

#[tokio::test]
async fn registration_transaction_retries_only_remote_lock_conflicts() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    for (code, expected_attempts) in [("SQLITE_BUSY", 9), ("SQLITE_AUTH", 1)] {
        let attempts = Arc::new(AtomicUsize::new(0));
        let app = axum::Router::new()
            .route("/v3/pipeline", axum::routing::post(reject_remote_transaction))
            .with_state((code, Arc::clone(&attempts)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Builder::new_remote(format!("http://{address}"), String::new())
            .connector(tower::service_fn(move |_| tokio::net::TcpStream::connect(address)))
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        let error =
            begin_registration_transaction(&conn).await.err().expect("server rejects transaction");
        server.abort();
        assert_eq!(attempts.load(Ordering::SeqCst), expected_attempts, "{error:?}");
        assert!(matches!(error, RegistryError::Libsql(_)));
    }
}

#[test]
fn transaction_conflicts_include_extended_sqlite_codes() {
    for code in [5, 6, 261, 517, 262] {
        assert!(is_transaction_conflict(&libsql::Error::SqliteFailure(code, String::new())));
        assert!(is_transaction_conflict(&libsql::Error::RemoteSqliteFailure(
            0,
            code,
            String::new()
        )));
    }
    for code in [1, 19, 2067] {
        assert!(!is_transaction_conflict(&libsql::Error::SqliteFailure(code, String::new())));
    }
    assert!(!is_transaction_conflict(&libsql::Error::ConnectionFailed("SQLITE_BUSY".to_string())));
}

async fn reject_remote_transaction(
    axum::extract::State((code, attempts)): axum::extract::State<(
        &'static str,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    )>,
    body: String,
) -> axum::Json<serde_json::Value> {
    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let body: serde_json::Value = serde_json::from_str(&body).unwrap();
    let results: Vec<_> = body["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|request| {
            if request["type"] == "get_autocommit" {
                serde_json::json!({
                    "type": "ok",
                    "response": {"type": "get_autocommit", "is_autocommit": true},
                })
            } else {
                serde_json::json!({
                    "type": "error",
                    "error": {"code": code, "message": "test transaction failure"},
                })
            }
        })
        .collect();
    axum::Json(serde_json::json!({"baton": null, "base_url": null, "results": results}))
}

async fn begin_registration_transaction(conn: &libsql::Connection) -> Result<libsql::Transaction> {
    retry_database_conflicts(|| async {
        conn.transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await
            .map_err(RegistryError::from)
    })
    .await
}
