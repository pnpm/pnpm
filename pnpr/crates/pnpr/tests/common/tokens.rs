//! Tokens for tests that drive a router built on an in-memory `AuthState`.

use pnpr::AuthState;

/// A token for `username`, stored as an account first: pnpr honors only
/// tokens whose owner is an account.
pub(crate) async fn issue_token(auth: &AuthState, username: &str) -> String {
    auth.users.create_user(username, "password").await.unwrap();
    auth.tokens.issue(username).await.unwrap()
}
