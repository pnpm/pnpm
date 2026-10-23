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
            let mut rows = conn.query("SELECT username FROM users ORDER BY username", ()).await?;
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
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                let tx = conn.transaction().await?;
                let inserted = tx.execute(
                    "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                    params![username, hash],
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
        username: &str,
        password: &str,
    ) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        let hash = hash.as_str();
        let updated = retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                Ok::<_, RegistryError>(
                    conn.execute(
                        "UPDATE users SET bcrypt_hash = ?2 WHERE username = ?1",
                        params![username, hash],
                    )
                    .await?,
                )
            })
        })
        .await?;
        Ok(if updated > 0 { PasswordChange::Changed } else { PasswordChange::NoSuchUser })
    }

    pub(super) async fn remove_user(&self, username: &str) -> Result<UserRemoval> {
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                let tx = conn.transaction().await?;
                let removed =
                    tx.execute("DELETE FROM users WHERE username = ?1", params![username]).await?;
                if removed > 0 {
                    tx.execute(COUNT_ONE_LESS, ()).await?;
                }
                tx.commit().await?;
                Ok::<_, RegistryError>(if removed > 0 {
                    UserRemoval::Removed
                } else {
                    UserRemoval::NoSuchUser
                })
            })
        })
        .await
    }
}
