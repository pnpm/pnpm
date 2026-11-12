use super::{
    super::{NewToken, TokenBackend, TokenRecord, sha256_hex, token_store::mint_token},
    AuthSqlBackend, Result, SqlAuth, unix_millis, with_auth_timeout,
};
use async_trait::async_trait;

#[async_trait]
impl<Db> TokenBackend for SqlAuth<Db>
where
    Db: AuthSqlBackend,
{
    async fn issue_token(
        &self,
        user_id: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let raw = mint_token();
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_millis();
        let record = TokenRecord {
            owner_name: None,
            user_id: user_id.to_string(),
            created_at: now,
            last_used_at: now,
            readonly: new_token.readonly,
            cidr_whitelist: new_token.cidr_whitelist,
            name: new_token.name,
            expires_at: new_token.expires_at,
            kind: new_token.kind,
        };
        self.db.insert_token(&token_hash, &record).await?;
        Ok((raw, record))
    }

    async fn issue(&self, user_id: &str) -> Result<String> {
        let (raw, _) = self.issue_token(user_id, NewToken::default()).await?;
        Ok(raw)
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        with_auth_timeout(self.timeout, self.db.touch_token(key, now)).await
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let record = with_auth_timeout(self.timeout, self.db.find_owned_token(&token_hash)).await?;
        let now = unix_millis();
        match record {
            Some(r) if r.expires_at.is_none_or(|exp| exp > now) => Ok(Some(r.user_id)),
            _ => Ok(None),
        }
    }

    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let record = with_auth_timeout(self.timeout, self.db.find_owned_token(&token_hash)).await?;
        let now = unix_millis();
        match record {
            Some(r) if r.expires_at.is_none_or(|exp| exp > now) => Ok(Some(r)),
            _ => Ok(None),
        }
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        with_auth_timeout(self.timeout, self.db.find_token(key)).await
    }

    async fn list_for_user(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>> {
        with_auth_timeout(self.timeout, self.db.list_tokens(user_id)).await
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = with_auth_timeout(self.timeout, self.db.find_token(key)).await? else {
            return Ok(None);
        };
        self.db.delete_token(key).await?;
        Ok(Some(record))
    }

    async fn delete_expired(&self, now: u64) -> Result<usize> {
        let removed = with_auth_timeout(self.timeout, self.db.delete_expired_tokens(now)).await?;
        Ok(usize::try_from(removed).unwrap_or(usize::MAX))
    }

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
    }
}
