//! Tokens for tests that drive a router built on an in-memory `AuthState`.

use pnpr::{AuthState, TokenBackend};
use pnpr_auth::LAST_USED_GRANULARITY_SECS;
use std::{sync::Arc, time::Duration};

/// A token for `username`, stored as an account first: pnpr honors only
/// tokens whose owner is an account.
pub(crate) async fn issue_token(auth: &AuthState, username: &str) -> String {
    auth.users.create_user(username, "password").await.unwrap();
    auth.tokens.issue(username).await.unwrap()
}

/// Set the stored last-used time of `raw` back far enough that its next
/// use is due for a write, and return that time.
pub(crate) async fn age_last_use(tokens: &Arc<dyn TokenBackend>, raw: &str) -> u64 {
    let record = tokens
        .lookup_record(raw)
        .await
        .unwrap()
        .unwrap();
    let aged = record.last_used_at - 2 * LAST_USED_GRANULARITY_SECS;
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
