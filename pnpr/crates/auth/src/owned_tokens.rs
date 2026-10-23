use super::{NewToken, Result, TokenBackend, TokenRecord, TokenStore, UserStore};
use async_trait::async_trait;
use std::sync::Arc;

/// The local token store, answering the authentication lookups
/// ([`TokenBackend::lookup`], [`TokenBackend::lookup_record`]) only for tokens
/// whose owner the local user store holds. The shared SQL backends join the
/// two tables for the same rule.
///
/// Only a login issues tokens, so every token starts with a stored owner.
/// Removing the owner then disables its tokens at once, before and whether or
/// not they are revoked. Listing and revocation still see such tokens, so
/// they can be cleaned up.
pub struct OwnedTokens {
    tokens: TokenStore,
    users: Arc<UserStore>,
}

impl OwnedTokens {
    #[must_use]
    pub fn new(tokens: TokenStore, users: Arc<UserStore>) -> Self {
        Self { tokens, users }
    }
}

#[async_trait]
impl TokenBackend for OwnedTokens {
    async fn issue_token(
        &self,
        username: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        self.tokens.issue_token(username, new_token).await
    }

    async fn issue(&self, username: &str) -> Result<String> {
        self.tokens.issue(username).await
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        self.tokens.touch_key(key, now).await
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let Some(owner) = self.tokens.lookup(raw).await? else {
            return Ok(None);
        };
        if self.users.holds(&owner)? { Ok(Some(owner)) } else { Ok(None) }
    }

    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = self.tokens.lookup_record(raw).await? else {
            return Ok(None);
        };
        if self.users.holds(&record.username)? { Ok(Some(record)) } else { Ok(None) }
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        self.tokens.find_by_key(key).await
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        self.tokens.list_for_user(username).await
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        self.tokens.revoke_by_key(key).await
    }

    async fn revoke_all_for_user(&self, username: &str) -> Result<usize> {
        self.tokens.revoke_all_for_user(username).await
    }

    async fn check_readiness(&self) -> Result<()> {
        self.tokens.check_readiness().await
    }
}
