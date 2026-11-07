use super::{
    LibsqlAuth, RegistryError, Result, first_row, is_unique_violation, params,
    retry_database_conflicts, validate_username,
};

impl LibsqlAuth {
    pub(super) async fn list_accounts_impl(&self) -> Result<Vec<crate::UserAccount>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT id, username FROM users WHERE bcrypt_hash <> '!' ORDER BY username",
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
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT id, username FROM users WHERE id IN (SELECT value FROM json_each(?1)) AND bcrypt_hash <> '!'",
                params![ids],
            ).await?;
            collect_accounts(rows).await
        }).await
    }

    pub(super) async fn provision_account(&self, username: &str) -> Result<String> {
        validate_username(username)?;
        let id = crate::new_principal_id();
        let id_ref = id.as_str();
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                let tx = conn.transaction().await?;
                let inserted = tx.execute(
                    "INSERT INTO users (id, username, bcrypt_hash) VALUES (?1, ?2, '*')",
                    params![id_ref, username],
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
                    Err(err) if is_unique_violation(&err) => Err(RegistryError::AdminConflict {
                        reason: format!("username {username:?} is taken"),
                    }),
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
            .stored_hash(username)
            .await?
            .filter(|(hash, _)| hash != crate::DELETED_USER_HASH)
            .map(|(_, id)| id))
    }

    pub(super) async fn account_name_by_id(&self, user_id: &str) -> Result<Option<String>> {
        self.pool.read(|conn| async move {
            let rows = conn.query(
                "SELECT username FROM users WHERE id = ?1 AND bcrypt_hash <> '!'",
                params![user_id],
            )
            .await?;
            first_row(rows, |row| Ok(row.get::<String>(0)?)).await
        })
        .await
    }

    pub(super) async fn rename_account(&self, user_id: &str, username: &str) -> Result<()> {
        validate_username(username)?;
        let current = self.account_name_by_id(user_id).await?.ok_or(RegistryError::NotFound)?;
        if current == username {
            return Ok(());
        }
        retry_database_conflicts(|| {
            self.pool.write(|conn| async move {
                match conn.execute(
                    "UPDATE users SET username = ?2 WHERE id = ?1 AND bcrypt_hash <> '!'",
                    params![user_id, username],
                )
                .await
                {
                    Ok(0) => Err(RegistryError::NotFound),
                    Ok(_) => Ok(()),
                    Err(err) if is_unique_violation(&err) => Err(RegistryError::AdminConflict {
                        reason: format!("username {username:?} is taken"),
                    }),
                    Err(err) => Err(err.into()),
                }
            })
        })
        .await?;
        self.sync_replica().await?;
        Ok(())
    }
}

async fn collect_accounts(mut rows: libsql::Rows) -> Result<Vec<crate::UserAccount>> {
    let mut accounts = Vec::new();
    while let Some(row) = rows.next().await? {
        accounts.push(crate::UserAccount { id: row.get(0)?, username: row.get(1)? });
    }
    Ok(accounts)
}
