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
    assert_eq!(backend.verify("alice", "secret").await.as_deref(), Some("alice"));
    assert!(backend.verify("alice", "wrong").await.is_none());
    assert!(backend.verify("bob", "secret").await.is_none());
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

#[tokio::test]
async fn tokens_round_trip_and_revoke() {
    let backend = local_backend(MaxUsers::Unlimited).await;
    let token = backend.issue("alice").await.unwrap();
    assert_eq!(backend.lookup(&token).await.as_deref(), Some("alice"));
    assert!(backend.lookup("not-a-token").await.is_none());

    let key = sha256_hex(token.as_bytes());
    let listed = backend.list_for_user("alice").await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, key);
    assert_eq!(listed[0].1.username, "alice");

    assert!(backend.revoke_by_key(&key).await.unwrap().is_some());
    assert!(backend.lookup(&token).await.is_none());
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
