use super::{
    AccountStatus, AuthSqlBackend, MaxUsers, PostgresDatabase, RegistryError, Result, Row,
    StoredUser, claim_user_slot, decode_sql_account_state, is_unique_violation,
};
use crate::{
    AccountLifecycle, AccountStatusOutcome,
    account::should_apply_lifecycle,
    validation::{name_revision_bounds, status_revision_to_sql},
};

impl PostgresDatabase {
    pub(super) async fn stored_user_impl(&self, username: &str) -> Result<Option<StoredUser>> {
        let row = sqlx::query(
            "SELECT id, username, status, credential_kind, password_hash FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&mut *self.pool.acquire().await?)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id: String = row.try_get(0)?;
        let username: String = row.try_get(1)?;
        let raw_status: String = row.try_get(2)?;
        let raw_kind: String = row.try_get(3)?;
        let password_hash: Option<String> = row.try_get(4)?;
        let (status, credential) =
            decode_sql_account_state(&id, &raw_status, &raw_kind, password_hash)?;
        Ok(Some(StoredUser { id, username, status, credential }))
    }

    pub(super) async fn account_status_impl(&self, user_id: &str) -> Result<Option<AccountStatus>> {
        let raw: Option<String> = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&mut *self.pool.acquire().await?)
            .await?;
        match raw {
            Some(raw) => AccountStatus::parse(&raw)
                .map(Some)
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("user {user_id} has an unrecognized status {raw:?}"),
                }),
            None => Ok(None),
        }
    }

    pub(super) async fn status_revisions_impl(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::HashMap<String, crate::AccountStatusRevision>> {
        if user_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows = sqlx::query("SELECT id, status, status_revision FROM users WHERE id = ANY($1)")
            .bind(user_ids)
            .fetch_all(&mut *self.pool.acquire().await?)
            .await?;
        let mut revisions = std::collections::HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get(0)?;
            let raw_status: String = row.try_get(1)?;
            let status = AccountStatus::parse(&raw_status)
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("user {id} has an unrecognized status {raw_status:?}"),
                })?;
            let revision = u64::try_from(row.try_get::<i64, _>(2)?).unwrap_or(0);
            revisions.insert(id, crate::AccountStatusRevision::reconciled(status, revision));
        }
        Ok(revisions)
    }

    pub(super) async fn apply_account_status_impl(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: AccountLifecycle,
    ) -> Result<AccountStatusOutcome> {
        let sql_revision = status_revision_to_sql(revision)?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT status, status_revision FROM users
             WHERE id = $1 AND status <> 'deleted' FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(AccountStatusOutcome::Missing);
        };
        let raw_status: String = row.try_get(0)?;
        let actual_status = AccountStatus::parse(&raw_status)
            .ok_or_else(|| RegistryError::Internal {
                reason: format!("user {user_id} has an unrecognized status {raw_status:?}"),
            })?;
        let actual_rev = u64::try_from(row.try_get::<i64, _>(1)?).unwrap_or(0);
        if !should_apply_lifecycle(actual_status, actual_rev, revision, lifecycle) {
            tx.rollback().await?;
            return Ok(AccountStatusOutcome::Superseded);
        }
        apply_postgres_lifecycle(&mut tx, user_id, sql_revision, lifecycle).await?;
        tx.commit().await?;
        Ok(AccountStatusOutcome::Applied)
    }

    pub(super) async fn claim_tx_for_insert(
        &self,
        max_users: MaxUsers,
    ) -> Result<Option<sqlx::Transaction<'_, sqlx::Postgres>>> {
        let mut can_retry_after_reconcile = matches!(max_users, MaxUsers::Limited(_));
        loop {
            let mut tx = self.pool.begin().await?;
            if claim_user_slot(&mut tx, max_users).await? {
                return Ok(Some(tx));
            }
            tx.rollback().await?;
            if !std::mem::take(&mut can_retry_after_reconcile)
                || !self.reconcile_user_counter_overcount_impl().await?
            {
                return Ok(None);
            }
        }
    }

    pub(super) async fn account_login_impl(
        &self,
        user_id: &str,
    ) -> Result<Option<crate::AccountLogin>> {
        let row = sqlx::query(
            "SELECT username, name_revision FROM users WHERE id = $1 AND status <> 'deleted'",
        )
        .bind(user_id)
        .fetch_optional(&mut *self.pool.acquire().await?)
        .await?;
        row.map(|row| -> std::result::Result<crate::AccountLogin, sqlx::Error> {
            let revision: i64 = row.try_get(1)?;
            Ok(crate::AccountLogin {
                username: row.try_get(0)?,
                revision: u64::try_from(revision).unwrap_or(0),
            })
        })
        .transpose()
        .map_err(RegistryError::from)
    }

    pub(super) async fn accounts_by_id_impl(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<crate::UserAccount>> {
        let rows = sqlx::query(
            "SELECT id, username FROM users WHERE id = ANY($1) AND status <> 'deleted'",
        )
        .bind(user_ids)
        .fetch_all(&mut *self.pool.acquire().await?)
        .await?;
        rows.into_iter()
            .map(|row| Ok(crate::UserAccount { id: row.try_get(0)?, username: row.try_get(1)? }))
            .collect()
    }

    pub(super) async fn rename_account(&self, user_id: &str, username: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE users
             SET username = $2, username_skeleton_hash = $3, name_revision = name_revision + 1
             WHERE id = $1 AND status <> 'deleted'",
        )
        .bind(user_id)
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .execute(&mut *self.pool.acquire().await?)
        .await;
        match result {
            Ok(result) if result.rows_affected() > 0 => Ok(()),
            Ok(_) => Err(RegistryError::NotFound),
            Err(err) if is_unique_violation(&err) => Err(crate::validation::username_conflict(
                username,
                self.stored_user(username).await?.is_some(),
            )),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn rename_account_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename> {
        let Some((expected, next, next_revision)) = name_revision_bounds(expected_revision)? else {
            self.account_name(user_id).await?.ok_or(RegistryError::NotFound)?;
            return Ok(crate::UserRename::CurrentChanged);
        };
        let result = sqlx::query(
            "UPDATE users
             SET username = $2, username_skeleton_hash = $3, name_revision = $5
             WHERE id = $1 AND name_revision = $4 AND status <> 'deleted'",
        )
        .bind(user_id)
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .bind(expected)
        .bind(next)
        .execute(&mut *self.pool.acquire().await?)
        .await;
        match result {
            Ok(result) if result.rows_affected() > 0 => {
                Ok(crate::UserRename::Renamed(next_revision))
            }
            Ok(_) => match self.account_name(user_id).await? {
                Some(_) => Ok(crate::UserRename::CurrentChanged),
                None => Err(RegistryError::NotFound),
            },
            Err(err) if is_unique_violation(&err) => Err(crate::validation::username_conflict(
                username,
                self.stored_user(username).await?.is_some(),
            )),
            Err(err) => Err(err.into()),
        }
    }
}

async fn apply_postgres_lifecycle(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: &str,
    sql_revision: i64,
    lifecycle: AccountLifecycle,
) -> Result<()> {
    match lifecycle {
        AccountLifecycle::Active => {
            sqlx::query("UPDATE users SET status = 'active', status_revision = $2 WHERE id = $1")
                .bind(user_id)
                .bind(sql_revision)
                .execute(&mut **tx)
                .await?;
        }
        AccountLifecycle::Deactivated => {
            sqlx::query(
                "UPDATE users
                 SET status = 'deactivated',
                     credential_kind = 'none',
                     password_hash = NULL,
                     status_revision = $2
                 WHERE id = $1",
            )
            .bind(user_id)
            .bind(sql_revision)
            .execute(&mut **tx)
            .await?;
            sqlx::query("DELETE FROM tokens WHERE user_id = $1")
                .bind(user_id)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}
