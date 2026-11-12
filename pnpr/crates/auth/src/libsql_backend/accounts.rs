use super::{
    AccountStatus, LibsqlAuth, RegistryError, Result, first_row, is_unique_violation, params,
    retry_database_conflicts, token_timestamp_to_sql, unix_millis, validate_username,
};
use crate::validation::name_revision_bounds;

impl LibsqlAuth {
    pub(super) async fn list_accounts_impl(&self) -> Result<Vec<crate::UserAccount>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT id, username FROM users WHERE status <> 'deleted' ORDER BY username",
                (),
            )
            .await?;
            collect_accounts(rows).await
        })
        .await
    }

    pub(super) async fn accounts_by_id_impl(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<crate::UserAccount>> {
        let ids = serde_json::to_string(user_ids)?;
        let ids = ids.as_str();
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT id, username FROM users WHERE id IN (SELECT value FROM json_each(?1)) AND status <> 'deleted'",
                params![ids],
            ).await?;
            collect_accounts(rows).await
        }).await
    }

    pub(super) async fn provision_account(&self, username: &str) -> Result<String> {
        validate_username(username)?;
        let id = crate::new_principal_id();
        let id_ref = id.as_str();
        let created_at = token_timestamp_to_sql(unix_millis());
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                let tx = conn.transaction().await?;
                let inserted = tx.execute(
                    "INSERT INTO users (id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at)
                     VALUES (?1, ?2, ?3, 'active', 'none', NULL, ?4)",
                    params![
                        id_ref,
                        username,
                        crate::validation::username_skeleton_hash(username),
                        created_at,
                    ],
                )
                .await;
                match inserted {
                    Ok(_) => {
                        tx.execute(
                            "UPDATE auth_counters SET value = value + 1 WHERE name = 'users'",
                            (),
                        )
                        .await?;
                        tx.commit().await?;
                        Ok(())
                    }
                    Err(err) if is_unique_violation(&err) => {
                        let conflict = classify_username_conflict(&tx, username).await?;
                        tx.rollback().await?;
                        Err(conflict)
                    }
                    Err(err) => Err(err.into()),
                }
            })
        })
        .await?;
        self.sync_replica().await?;
        Ok(id)
    }

    pub(super) async fn account_id_by_name(&self, username: &str) -> Result<Option<String>> {
        Ok(self
            .stored_account(username)
            .await?
            .filter(|account| !account.status.is_deleted())
            .map(|account| account.id))
    }

    pub(super) async fn active_account_id_by_name(&self, username: &str) -> Result<Option<String>> {
        Ok(self
            .stored_account(username)
            .await?
            .filter(|account| account.status.is_active())
            .map(|account| account.id))
    }

    pub(super) async fn account_name_by_id(&self, user_id: &str) -> Result<Option<String>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT username FROM users WHERE id = ?1 AND status <> 'deleted'",
                params![user_id],
            )
            .await?;
            first_row(rows, |row| Ok(row.get::<String>(0)?)).await
        })
        .await
    }

    pub(super) async fn active_account_name_by_id(&self, user_id: &str) -> Result<Option<String>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT username FROM users WHERE id = ?1 AND status = 'active'",
                params![user_id],
            )
            .await?;
            first_row(rows, |row| Ok(row.get::<String>(0)?)).await
        })
        .await
    }

    pub(super) async fn account_status_by_id(
        &self,
        user_id: &str,
    ) -> Result<Option<AccountStatus>> {
        self.pool.read(|conn| async move {
            let rows =
                conn.query("SELECT status FROM users WHERE id = ?1", params![user_id]).await?;
            let Some(raw) = first_row(rows, |row| Ok(row.get::<String>(0)?)).await? else {
                return Ok(None);
            };
            let status = AccountStatus::parse(&raw)
                .ok_or_else(|| RegistryError::Internal {
                    reason: format!("user {user_id} has an unrecognized status {raw:?}"),
                })?;
            Ok(Some(status))
        })
        .await
    }

    pub(super) async fn set_account_status_by_id(
        &self,
        user_id: &str,
        status: AccountStatus,
    ) -> Result<bool> {
        if status.is_deleted() {
            return Err(RegistryError::BadRequest {
                reason: "use delete_user to delete an account".to_string(),
            });
        }
        let status_str = status.as_str();
        let updated = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                Ok::<_, RegistryError>(
                    conn.execute(
                        "UPDATE users
                         SET status = ?2, name_revision = name_revision + 1
                         WHERE id = ?1 AND status <> 'deleted'",
                        params![user_id, status_str],
                    )
                    .await?,
                )
            })
        })
        .await?;
        if updated > 0 {
            self.sync_replica().await?;
        }
        Ok(updated > 0)
    }

    pub(super) async fn set_account_status_by_id_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        status: AccountStatus,
    ) -> Result<crate::AccountStatusUpdate> {
        if status.is_deleted() {
            return Err(RegistryError::BadRequest {
                reason: "use delete_user to delete an account".to_string(),
            });
        }
        let Some((expected, next, next_revision)) = name_revision_bounds(expected_revision)? else {
            self.account_name_by_id(user_id).await?.ok_or(RegistryError::NotFound)?;
            return Ok(crate::AccountStatusUpdate::CurrentChanged);
        };
        let status_str = status.as_str();
        let sql = if matches!(status, AccountStatus::Deactivated) {
            "UPDATE users
             SET status = ?2, credential_kind = 'none', password_hash = NULL, name_revision = ?3
             WHERE id = ?1 AND name_revision = ?4 AND status <> 'deleted'"
        } else {
            "UPDATE users
             SET status = ?2, name_revision = ?3
             WHERE id = ?1 AND name_revision = ?4 AND status <> 'deleted'"
        };
        let updated = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                Ok::<_, RegistryError>(
                    conn.execute(sql, params![user_id, status_str, next, expected]).await?,
                )
            })
        })
        .await?;
        if updated > 0 {
            self.sync_replica().await?;
            return Ok(crate::AccountStatusUpdate::Updated(next_revision));
        }
        self.sync_replica().await?;
        match self.account_name_by_id(user_id).await? {
            Some(_) => Ok(crate::AccountStatusUpdate::CurrentChanged),
            None => Err(RegistryError::NotFound),
        }
    }

    pub(super) async fn account_login_by_id(
        &self,
        user_id: &str,
    ) -> Result<Option<crate::AccountLogin>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT username, name_revision FROM users WHERE id = ?1 AND status <> 'deleted'",
                params![user_id],
            )
            .await?;
            first_row(rows, |row| {
                let username: String = row.get(0)?;
                let revision = u64::try_from(row.get::<i64>(1)?).unwrap_or(0);
                Ok(crate::AccountLogin { username, revision })
            })
            .await
        })
        .await
    }

    pub(super) async fn rename_account(&self, user_id: &str, username: &str) -> Result<()> {
        validate_username(username)?;
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                let tx = conn.transaction().await?;
                match tx.execute(
                    "UPDATE users
                     SET username = ?2, username_skeleton_hash = ?3, name_revision = name_revision + 1
                     WHERE id = ?1 AND status <> 'deleted'",
                    params![user_id, username, crate::validation::username_skeleton_hash(username)],
                )
                .await
                {
                    Ok(0) => {
                        tx.rollback().await?;
                        Err(RegistryError::NotFound)
                    }
                    Ok(_) => {
                        tx.commit().await?;
                        Ok(())
                    }
                    Err(err) if is_unique_violation(&err) => {
                        let conflict = classify_username_conflict(&tx, username).await?;
                        tx.rollback().await?;
                        Err(conflict)
                    }
                    Err(err) => Err(err.into()),
                }
            })
        })
        .await?;
        self.sync_replica().await?;
        Ok(())
    }

    pub(super) async fn rename_account_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename> {
        validate_username(username)?;
        let Some((expected, next, next_revision)) = name_revision_bounds(expected_revision)? else {
            self.account_name_by_id(user_id).await?.ok_or(RegistryError::NotFound)?;
            return Ok(crate::UserRename::CurrentChanged);
        };
        let outcome = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                execute_conditional_rename(&conn, user_id, expected, next, username).await
            })
        })
        .await?;
        match outcome {
            PrimaryRename::CurrentChanged => {
                self.sync_replica().await?;
                Ok(crate::UserRename::CurrentChanged)
            }
            PrimaryRename::Renamed { previous } => {
                if let Err(err) = self.sync_replica().await {
                    self.rollback_unsynced_rename(user_id, next_revision, username, &previous)
                        .await;
                    return Err(err);
                }
                Ok(crate::UserRename::Renamed(next_revision))
            }
        }
    }

    async fn rollback_unsynced_rename(
        &self,
        user_id: &str,
        revision: u64,
        applied: &str,
        previous: &str,
    ) {
        if applied == previous {
            return;
        }
        let Ok(Some((expected, next, _))) = name_revision_bounds(revision) else {
            return;
        };
        let _ = self.pool.write(|conn| async move {
            execute_conditional_rename(&conn, user_id, expected, next, previous).await
        })
        .await;
    }
}

enum PrimaryRename {
    Renamed { previous: String },
    CurrentChanged,
}

async fn execute_conditional_rename(
    conn: &libsql::Connection,
    user_id: &str,
    expected: i64,
    next: i64,
    username: &str,
) -> Result<PrimaryRename> {
    let tx = conn.transaction().await?;
    let rows = tx.query(
        "SELECT username, name_revision FROM users WHERE id = ?1 AND status <> 'deleted'",
        params![user_id],
    )
    .await?;
    let Some((previous, actual)) =
        first_row(rows, |row| Ok((row.get::<String>(0)?, row.get::<i64>(1)?))).await?
    else {
        tx.rollback().await?;
        return Err(RegistryError::NotFound);
    };
    if actual != expected {
        tx.rollback().await?;
        return Ok(PrimaryRename::CurrentChanged);
    }
    apply_primary_rename(tx, user_id, expected, next, username, previous).await
}

async fn apply_primary_rename(
    tx: libsql::Transaction,
    user_id: &str,
    expected: i64,
    next: i64,
    username: &str,
    previous: String,
) -> Result<PrimaryRename> {
    let skeleton = crate::validation::username_skeleton_hash(username);
    match tx.execute(
        "UPDATE users SET username = ?2, username_skeleton_hash = ?3, name_revision = ?4
         WHERE id = ?1 AND name_revision = ?5 AND status <> 'deleted'",
        params![user_id, username, skeleton, next, expected],
    )
    .await
    {
        Ok(0) => {
            tx.rollback().await?;
            Ok(PrimaryRename::CurrentChanged)
        }
        Ok(_) => {
            tx.commit().await?;
            Ok(PrimaryRename::Renamed { previous })
        }
        Err(err) if is_unique_violation(&err) => {
            let conflict = classify_username_conflict(&tx, username).await?;
            tx.rollback().await?;
            Err(conflict)
        }
        Err(err) => Err(err.into()),
    }
}

async fn collect_accounts(mut rows: libsql::Rows) -> Result<Vec<crate::UserAccount>> {
    let mut accounts = Vec::new();
    while let Some(row) = rows.next().await? {
        accounts.push(crate::UserAccount { id: row.get(0)?, username: row.get(1)? });
    }
    Ok(accounts)
}

/// The write transaction keeps this lookup on the primary.
async fn classify_username_conflict(
    transaction: &libsql::Transaction,
    username: &str,
) -> Result<RegistryError> {
    let rows =
        transaction.query("SELECT id FROM users WHERE username = ?1", params![username]).await?;
    let exact_match = first_row(rows, |_| Ok(())).await?.is_some();
    Ok(crate::validation::username_conflict(username, exact_match))
}
