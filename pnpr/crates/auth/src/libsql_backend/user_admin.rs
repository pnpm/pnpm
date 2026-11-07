use super::{
    DEFAULT_BCRYPT_COST, LibsqlAuth, PasswordChange, Result, UserCreation, UserRemoval,
    hash_bcrypt, is_unique_violation, params, retry_database_conflicts, validate_username,
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
                "SELECT username FROM users WHERE bcrypt_hash <> '!' ORDER BY username",
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
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        let hash = hash.as_str();
        let id = crate::new_principal_id();
        let id = id.as_str();
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                let tx = conn.transaction().await?;
                let inserted = tx.execute(
                    "INSERT INTO users (username, bcrypt_hash, id) VALUES (?1, ?2, ?3)",
                    params![username, hash, id],
                )
                .await;
                match inserted {
                    Ok(_) => {
                        tx.execute(COUNT_ONE_MORE, ()).await?;
                        tx.commit().await?;
                        Ok(UserCreation::Created)
                    }
                    Err(err) if is_unique_violation(&err) => {
                        tx.rollback().await?;
                        Ok(UserCreation::NameTaken)
                    }
                    Err(err) => Err(RegistryError::from(err)),
                }
            })
        })
        .await
    }

    pub(super) async fn update_password(
        &self,
        user_id: &str,
        password: &str,
    ) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        self.replace_password_hash(user_id, &hash).await
    }

    pub(super) async fn replace_password_hash(
        &self,
        user_id: &str,
        hash: &str,
    ) -> Result<PasswordChange> {
        let updated = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                Ok::<_, RegistryError>(
                    conn.execute(
                        "UPDATE users SET bcrypt_hash = ?2 WHERE id = ?1 AND bcrypt_hash <> '!'",
                        params![user_id, hash],
                    )
                    .await?,
                )
            })
        })
        .await?;
        if updated > 0 {
            self.sync_replica().await?;
            Ok(PasswordChange::Changed)
        } else {
            Ok(PasswordChange::NoSuchUser)
        }
    }

    pub(super) async fn remove_user_record(&self, user_id: &str) -> Result<UserRemoval> {
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move { remove_user_in_transaction(&conn, user_id).await })
        })
        .await
    }
}

async fn remove_user_in_transaction(
    conn: &libsql::Connection,
    user_id: &str,
) -> Result<UserRemoval> {
    let tx = conn.transaction().await?;
    let statement = "UPDATE users SET username = 'deleted:' || id, bcrypt_hash = '!' WHERE id = ?1 AND bcrypt_hash <> '!'";
    let removed = tx.execute(statement, params![user_id]).await?;
    if removed > 0 {
        tx.execute(COUNT_ONE_LESS, ()).await?;
    }
    tx.commit().await?;
    Ok(if removed > 0 { UserRemoval::Removed } else { UserRemoval::NoSuchUser })
}
