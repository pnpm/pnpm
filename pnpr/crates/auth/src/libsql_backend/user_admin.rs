use super::{
    DEFAULT_BCRYPT_COST, LibsqlAuth, PasswordChange, Result, UserCreation, UserRemoval,
    hash_bcrypt, is_unique_violation, params, retry_database_conflicts, validate_username,
    with_auth_timeout,
};
use pnpr_error::RegistryError;

/// Keeps the `users` counter, which the registration cap reads, in step with
/// an admin's insert or delete in the same transaction.
const COUNT_ONE_MORE: &str = "UPDATE auth_counters SET value = value + 1 WHERE name = 'users'";
const COUNT_ONE_LESS: &str =
    "UPDATE auth_counters SET value = value - 1 WHERE name = 'users' AND value > 0";

impl LibsqlAuth {
    pub(super) async fn list_usernames(&self) -> Result<Vec<String>> {
        with_auth_timeout::<_, RegistryError>(self.timeout, async {
            let mut rows =
                self.conn.query("SELECT username FROM users ORDER BY username", ()).await?;
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
        retry_database_conflicts(|| async {
            let tx = self.conn.transaction().await?;
            let inserted = tx.execute(
                "INSERT INTO users (username, bcrypt_hash) VALUES (?1, ?2)",
                params![username, hash.as_str()],
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
                Err(err) => Err(err.into()),
            }
        })
        .await
    }

    pub(super) async fn update_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        let updated = retry_database_conflicts(|| async {
            Ok(self.conn.execute(
                "UPDATE users SET bcrypt_hash = ?2 WHERE username = ?1",
                params![username, hash.as_str()],
            )
            .await?)
        })
        .await?;
        if updated > 0 { Ok(PasswordChange::Changed) } else { Ok(PasswordChange::NoSuchUser) }
    }

    pub(super) async fn remove_user(&self, username: &str) -> Result<UserRemoval> {
        retry_database_conflicts(|| async {
            let tx = self.conn.transaction().await?;
            let removed =
                tx.execute("DELETE FROM users WHERE username = ?1", params![username]).await?;
            if removed > 0 {
                tx.execute(COUNT_ONE_LESS, ()).await?;
            }
            tx.commit().await?;
            if removed > 0 { Ok(UserRemoval::Removed) } else { Ok(UserRemoval::NoSuchUser) }
        })
        .await
    }
}
