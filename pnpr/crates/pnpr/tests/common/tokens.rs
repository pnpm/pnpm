//! Tokens for tests that drive a router built on an in-memory `AuthState`.

use pnpr::{AuthState, TokenBackend};
use pnpr_auth::LAST_USED_GRANULARITY_MILLIS;
use std::{sync::Arc, time::Duration};

/// A token for `username`, stored as an account first: pnpr honors only
/// tokens whose owner is an account.
pub(crate) async fn issue_token(auth: &AuthState, username: &str) -> String {
    let id = match auth.users.create_user(username, "password").await.unwrap() {
        pnpr::UserCreation::Created(id) => id,
        pnpr::UserCreation::NameTaken => auth.users
            .account_id(username)
            .await
            .unwrap()
            .unwrap(),
    };
    auth.tokens.issue(&id).await.unwrap()
}

/// Set the stored last-used time of `raw` back far enough that its next
/// use is due for a write, and return that time.
pub(crate) async fn age_last_use(tokens: &Arc<dyn TokenBackend>, raw: &str) -> u64 {
    let record = tokens
        .lookup_record(raw)
        .await
        .unwrap()
        .unwrap();
    let aged = record.last_used_at - 2 * LAST_USED_GRANULARITY_MILLIS;
    tokens.touch(raw, aged).await.unwrap();
    aged
}

/// Wait until a use recorded off the request path has advanced the stored
/// last-used time of `raw` past `previous`.
pub(crate) async fn wait_for_last_use_after(
    tokens: &Arc<dyn TokenBackend>,
    raw: &str,
    previous: u64,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while tokens
            .lookup_record(raw)
            .await
            .unwrap()
            .unwrap()
            .last_used_at
            <= previous
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the last-used write lands");
}

/// Explicit account records let policy fixtures refer to known immutable IDs:
/// each account's ID is its name.
pub(crate) fn seeded_auth(config: &pnpr::Config, accounts: &[(&str, &str)]) -> AuthState {
    let path = config.storage.hosted_dir.join("test-accounts.sqlite");
    let auth = pnpr::SqliteAuth::open(&path, config.identity.auth.users.max_users).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    for (name, password) in accounts {
        connection
            .execute(
                "INSERT OR IGNORE INTO users (id, username, status, credential_kind, password_hash, created_at)
                 VALUES (?1, ?1, 'active', 'bcrypt', ?2, 1000)",
                rusqlite::params![name, bcrypt::hash(password, 4).unwrap()],
            )
            .unwrap();
    }
    AuthState::sharing(auth)
}

pub(crate) async fn account_id(
    app: &axum::Router,
    name: &str,
    token: Option<&str>,
) -> Option<String> {
    use tower::ServiceExt as _;
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::get(format!("/-/user/{name}"))
                .header("authorization", format!("Bearer {}", token.unwrap_or("invalid")))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    if !response.status().is_success() {
        return None;
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"].as_str().map(str::to_string)
}
