use super::{
    CredentialKind, DEFAULT_BCRYPT_COST, LibsqlAuth, PasswordChange, Result, UserCreation,
    UserRemoval, hash_bcrypt, is_unique_violation, params, retry_database_conflicts,
    token_timestamp_to_sql, unix_millis, validate_username,
};
use pnpr_error::RegistryError;

/// Keeps the `users` counter, which the registration cap reads, in step with
/// an admin's insert or delete in the same transaction.
const COUNT_ONE_MORE: &str = "UPDATE auth_counters SET value = value + 1 WHERE name = 'users'";
const COUNT_ONE_LESS: &str =
    "UPDATE auth_counters SET value = value - 1 WHERE name = 'users' AND value > 0";

impl LibsqlAuth {
    pub(super) async fn list_usernames(&self) -> Result<Vec<String>> {
        self.pool.read(|conn| async move {
            let mut rows = conn.query(
                "SELECT username FROM users WHERE status = 'active' ORDER BY username",
                (),
            )
            .await?;
            let mut names = Vec::new();
            while let Some(row) = rows.next().await? {
                names.push(row.get::<String>(0)?);
            }
            Ok(names)
        })
        .await
    }

    pub(super) async fn insert_user_for_admin(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserCreation> {
        validate_username(username)?;
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        let hash = hash.as_str();
        let kind = CredentialKind::for_optional_hash(Some(hash)).as_str();
        let id = crate::new_principal_id();
        let id = id.as_str();
        let created_at = token_timestamp_to_sql(unix_millis());
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                insert_admin_user_in_tx(&conn, id, username, kind, hash, created_at).await
            })
        })
        .await
    }

    pub(super) async fn update_password(
        &self,
        user_id: &str,
        password: &str,
    ) -> Result<PasswordChange> {
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        self.replace_password_hash(user_id, Some(&hash)).await
    }

    pub(super) async fn replace_password_hash(
        &self,
        user_id: &str,
        hash: Option<&str>,
    ) -> Result<PasswordChange> {
        let kind = match hash {
            Some(hash) => CredentialKind::from_hash(hash)
                .expect("replace_password_hash requires a supported hash")
                .as_str(),
            None => CredentialKind::None.as_str(),
        };
        let updated = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                Ok::<_, RegistryError>(
                    conn.execute(
                        "UPDATE users SET credential_kind = ?2, password_hash = ?3
                         WHERE id = ?1 AND status <> 'deleted'",
                        params![user_id, kind, hash],
                    )
                    .await?,
                )
            })
        })
        .await?;
        if updated > 0 {
            self.sync_after_password_change(user_id).await?;
            Ok(PasswordChange::Changed)
        } else {
            Ok(PasswordChange::NoSuchUser)
        }
    }

    pub(super) async fn replace_password_hash_if_current(
        &self,
        user_id: &str,
        expected_hash: &str,
        hash: &str,
    ) -> Result<bool> {
        let kind = CredentialKind::from_hash(hash)
            .expect("replace_password_hash_if_current requires a supported hash")
            .as_str();
        let updated = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                Ok::<_, RegistryError>(
                    conn.execute(
                        "UPDATE users SET credential_kind = ?1, password_hash = ?2
                         WHERE id = ?3 AND password_hash = ?4 AND status = 'active'",
                        params![kind, hash, user_id, expected_hash],
                    )
                    .await?,
                )
            })
        })
        .await?;
        if updated > 0 {
            self.sync_after_password_change(user_id).await?;
        }
        Ok(updated > 0)
    }

    async fn sync_after_password_change(&self, user_id: &str) -> Result<()> {
        self.sync_replica().await
            .map_err(|source| RegistryError::PasswordChangeCommitted {
                user_id: user_id.to_string(),
                source: Box::new(source),
            })
    }

    pub(super) async fn remove_user_record(&self, user_id: &str) -> Result<UserRemoval> {
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move { remove_user_in_transaction(&conn, user_id).await })
        })
        .await
    }
}

async fn insert_admin_user_in_tx(
    conn: &libsql::Connection,
    id: &str,
    username: &str,
    kind: &str,
    hash: &str,
    created_at: i64,
) -> Result<UserCreation> {
    let tx = conn.transaction().await?;
    let inserted = tx.execute(
        "INSERT INTO users (id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at)
         VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
        params![
            id,
            username,
            crate::validation::username_skeleton_hash(username),
            kind,
            hash,
            created_at,
        ],
    )
    .await;
    match inserted {
        Ok(_) => {
            tx.execute(COUNT_ONE_MORE, ()).await?;
            tx.commit().await?;
            Ok(UserCreation::Created(id.to_string()))
        }
        Err(err) if is_unique_violation(&err) => {
            tx.rollback().await?;
            Ok(UserCreation::NameTaken)
        }
        Err(err) => Err(RegistryError::from(err)),
    }
}

async fn remove_user_in_transaction(
    conn: &libsql::Connection,
    user_id: &str,
) -> Result<UserRemoval> {
    let tx = conn.transaction().await?;
    let statement = "UPDATE users
                     SET username = 'deleted:' || id,
                         username_skeleton_hash = NULL,
                         status = 'deleted',
                         credential_kind = 'none',
                         password_hash = NULL
                     WHERE id = ?1 AND status <> 'deleted'";
    let removed = tx.execute(statement, params![user_id]).await?;
    if removed > 0 {
        tx.execute(COUNT_ONE_LESS, ()).await?;
    }
    tx.commit().await?;
    Ok(if removed > 0 { UserRemoval::Removed } else { UserRemoval::NoSuchUser })
}
