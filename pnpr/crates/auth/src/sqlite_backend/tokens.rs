use super::SqliteAuth;
use crate::{
    NewToken, TokenBackend, TokenKind, TokenRecord,
    token_store::{mint_token, sha256_hex, unix_millis},
    validation::{token_timestamp_from_sql, token_timestamp_to_sql},
};
use async_trait::async_trait;
use pnpr_error::{RegistryError, Result};
use pnpr_sql::adapter::{Connection, Param, Row};

/// The `tokens` columns every token read selects, in the order
/// [`decode_token`] reads them.
const TOKEN_COLUMNS: &str = "t.token_hash, t.user_id, t.created_at, t.last_used_at, t.readonly, t.cidr_whitelist, t.name, t.expires_at, t.kind";

#[async_trait]
impl TokenBackend for SqliteAuth {
    async fn check_readiness(&self) -> Result<()> {
        crate::UserBackend::check_readiness(self).await
    }

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
        let stored = record.clone();
        self.write(move |connection| insert_token(connection, &token_hash, &stored)).await?;
        Ok((raw, record))
    }

    /// Resolve a token whose owner is an active account.
    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        Ok(self.lookup_record(raw).await?.map(|record| record.user_id))
    }

    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_millis();
        self.read(move |connection| {
            let Some(mut row) = connection.query_optional(
                &format!(
                    "SELECT {TOKEN_COLUMNS}, u.username FROM tokens t
                     JOIN users u ON u.id = t.user_id AND u.status = 'active'
                     WHERE t.token_hash = ?1",
                ),
                &[Param::Text(&token_hash)],
            )?
            else {
                return Ok(None);
            };
            let owner_name: String = row.get(9)?;
            let (_, record) = decode_token(row)?;
            let expired = record.expires_at.is_some_and(|expires_at| expires_at <= now);
            Ok((!expired).then(|| record.with_owner_name(owner_name)))
        })
        .await
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        let key = key.to_string();
        self.write(move |connection| {
            connection.execute(
                "UPDATE tokens SET last_used_at = ?1 WHERE token_hash = ?2",
                &[Param::Integer(token_timestamp_to_sql(now)), Param::Text(&key)],
            )
        })
        .await
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let key = key.to_string();
        self.read(move |connection| find_token(connection, &key)).await
    }

    async fn list_for_user(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>> {
        let user_id = user_id.to_string();
        self.read(move |connection| {
            connection
                .query(
                    &format!("SELECT {TOKEN_COLUMNS} FROM tokens t WHERE t.user_id = ?1"),
                    &[Param::Text(&user_id)],
                )?
                .into_iter()
                .map(decode_token)
                .collect()
        })
        .await
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let key = key.to_string();
        self.write(move |connection| {
            let record = find_token(connection, &key)?;
            if record.is_some() {
                connection.execute(
                    "DELETE FROM tokens WHERE token_hash = ?1",
                    &[Param::Text(&key)],
                )?;
            }
            Ok(record)
        })
        .await
    }

    async fn revoke_all_for_user(&self, user_id: &str) -> Result<usize> {
        let user_id = user_id.to_string();
        self.write(move |connection| {
            Ok(connection
                .query(
                    "DELETE FROM tokens WHERE user_id = ?1 RETURNING token_hash",
                    &[Param::Text(&user_id)],
                )?
                .len())
        })
        .await
    }

    async fn delete_expired(&self, now: u64) -> Result<usize> {
        self.write(move |connection| {
            Ok(connection
                .query(
                    "DELETE FROM tokens WHERE expires_at IS NOT NULL AND expires_at <= ?1
                     RETURNING token_hash",
                    &[Param::Integer(token_timestamp_to_sql(now))],
                )?
                .len())
        })
        .await
    }
}

fn find_token(connection: &dyn Connection, key: &str) -> Result<Option<TokenRecord>> {
    connection
        .query_optional(
            &format!("SELECT {TOKEN_COLUMNS} FROM tokens t WHERE t.token_hash = ?1"),
            &[Param::Text(key)],
        )?
        .map(|row| decode_token(row).map(|(_, record)| record))
        .transpose()
}

fn insert_token(connection: &dyn Connection, token_hash: &str, record: &TokenRecord) -> Result<()> {
    let cidr_whitelist = serde_json::to_string(&record.cidr_whitelist)?;
    connection.execute(
        "INSERT INTO tokens
             (token_hash, user_id, created_at, last_used_at, readonly, cidr_whitelist, name, expires_at, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        &[
            Param::Text(token_hash),
            Param::Text(&record.user_id),
            Param::Integer(token_timestamp_to_sql(record.created_at)),
            Param::Integer(token_timestamp_to_sql(record.last_used_at)),
            Param::Integer(i64::from(record.readonly)),
            Param::Text(&cidr_whitelist),
            record.name.as_deref().map_or(Param::Null, Param::Text),
            record.expires_at.map_or(Param::NullInteger, |expires_at| {
                Param::Integer(token_timestamp_to_sql(expires_at))
            }),
            record.kind.map_or(Param::Null, |kind| Param::Text(kind.as_str())),
        ],
    )
}

/// Decode a row selecting [`TOKEN_COLUMNS`] into its `(token_hash, record)`
/// pair.
fn decode_token(mut row: Row) -> Result<(String, TokenRecord)> {
    let token_hash: String = row.get(0)?;
    let cidr_whitelist: Vec<String> = serde_json::from_str(&row.get::<String>(5)?)
        .map_err(|err| RegistryError::Internal {
            reason: format!("token {token_hash} has an unreadable cidr_whitelist: {err}"),
        })?;
    let expires_at: Option<i64> = row.get(7)?;
    let kind = TokenKind::from_sql(row.get::<Option<String>>(8)?.as_deref(), &token_hash)?;
    let record = TokenRecord {
        owner_name: None,
        user_id: row.get(1)?,
        created_at: token_timestamp_from_sql(row.get(2)?),
        last_used_at: token_timestamp_from_sql(row.get(3)?),
        readonly: row.get::<i64>(4)? != 0,
        cidr_whitelist,
        name: row.get(6)?,
        expires_at: expires_at.map(token_timestamp_from_sql),
        kind,
    };
    Ok((token_hash, record))
}
