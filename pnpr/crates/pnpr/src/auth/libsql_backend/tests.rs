use super::*;

/// In-memory libsql database, exercising the same driver and SQL the
/// networked backend uses without a server.
async fn local_backend(max_users: MaxUsers) -> LibsqlAuth {
    let db = Builder::new_local(":memory:").build().await.unwrap();
    LibsqlAuth::from_database(db, max_users).await.unwrap()
}

#[tokio::test]
async fn add_or_login_creates_then_logs_in() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    assert!(matches!(
        backend.add_or_login("alice", "secret").await.unwrap(),
        UpsertOutcome::Created,
    ));
    assert!(matches!(
        backend.add_or_login("alice", "secret").await.unwrap(),
        UpsertOutcome::LoggedIn,
    ));
    assert_eq!(backend.verify("alice", "secret").await.unwrap().as_deref(), Some("alice"));
    assert!(backend.verify("alice", "wrong").await.unwrap().is_none());
    assert!(backend.verify("bob", "secret").await.unwrap().is_none());
}

#[tokio::test]
async fn add_or_login_rejects_existing_user_with_wrong_password() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    backend.add_or_login("alice", "secret").await.unwrap();
    let err = backend.add_or_login("alice", "different").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
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

/// The cap is strict, not best-effort: a concurrent burst of distinct
/// new users against a cap of 1 admits exactly one. The count-and-insert
/// runs in a single statement, so the guard can't be raced.
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
        if matches!(handle.await.unwrap(), Ok(UpsertOutcome::Created)) {
            created += 1;
        }
    }
    assert_eq!(created, 1, "exactly one registration may win the cap of 1");

    let mut rows = backend.conn.query("SELECT COUNT(*) FROM users", ()).await.unwrap();
    let total: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(total, 1, "the cap must be strictly enforced, never exceeded");
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
