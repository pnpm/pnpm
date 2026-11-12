use super::{
    AccountCredential, AccountStatus, CredentialKind, DEFAULT_BCRYPT_COST, LibsqlAuth, MaxUsers,
    RegistryError, Result, StoredLibsqlUser, TransactionBehavior, UpsertOutcome,
    claim_user_counter_slot, deprovisioned_login_error, dummy_verify, hash_bcrypt, is_sha_hash,
    is_unique_violation, params, reconcile_user_counter_overcount, token_timestamp_to_sql,
    unix_millis, validate_username, verify_returning_user,
};

impl LibsqlAuth {
    pub(super) async fn add_or_login_attempt(
        &self,
        username: &str,
        password: &str,
        hash: &tokio::sync::OnceCell<String>,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;
        crate::validation::validate_password(password)?;

        if let Some(stored) = self.stored_account(username).await? {
            return self.verify_and_maybe_upgrade(username, password, stored).await;
        }

        if matches!(self.max_users, MaxUsers::Disabled) {
            dummy_verify(password, DEFAULT_BCRYPT_COST).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }

        self.check_registration_allowed().await?;

        let hash =
            hash.get_or_try_init(|| hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST)).await?;
        if matches!(self.max_users, MaxUsers::Unlimited) {
            return self.insert_uncapped_user(username, password, hash).await;
        }

        let _registration_guard = self.registration_lock.lock().await;
        self.insert_capped_user(username, password, hash).await
    }

    async fn check_registration_allowed(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) if self.user_count().await? >= max => {
                Err(RegistryError::TooManyUsers { max })
            }
            _ => Ok(()),
        }
    }

    async fn insert_uncapped_user(
        &self,
        username: &str,
        password: &str,
        hash: &str,
    ) -> Result<(UpsertOutcome, String)> {
        let id = crate::new_principal_id();
        let id_ref = id.as_str();
        let kind = CredentialKind::for_optional_hash(Some(hash)).as_str();
        let created_at = token_timestamp_to_sql(unix_millis());
        let inserted = self.pool.write(|conn| async move {
            let inserted = conn.execute(
                "INSERT INTO users (id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at)
                 VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
                params![
                    id_ref,
                    username,
                    crate::validation::username_skeleton_hash(username),
                    kind,
                    hash,
                    created_at,
                ],
            )
            .await;
            match inserted {
                Ok(_) => Ok(Insert::Created(id_ref.to_string())),
                Err(err) if is_unique_violation(&err) => Ok(Insert::NameTaken),
                Err(err) => Err(RegistryError::from(err)),
            }
        })
        .await?;
        match inserted {
            Insert::Created(id) => Ok((UpsertOutcome::Created, id)),
            Insert::NameTaken => self.login_after_lost_insert(username, password).await,
            Insert::CapFull => unreachable!("an uncapped insert has no cap"),
        }
    }

    async fn insert_capped_user(
        &self,
        username: &str,
        password: &str,
        hash: &str,
    ) -> Result<(UpsertOutcome, String)> {
        let mut can_retry_after_reconcile = true;
        loop {
            match self.try_capped_insert(username, hash).await? {
                Insert::Created(id) => return Ok((UpsertOutcome::Created, id)),
                Insert::NameTaken => return self.login_after_lost_insert(username, password).await,
                Insert::CapFull => {
                    if !std::mem::take(&mut can_retry_after_reconcile)
                        || !self.reconcile_user_counter().await?
                    {
                        return self.reject_over_cap(username, password).await;
                    }
                }
            }
        }
    }

    async fn try_capped_insert(&self, username: &str, hash: &str) -> Result<Insert> {
        let id = crate::new_principal_id();
        let id_ref = id.as_str();
        let kind = CredentialKind::for_optional_hash(Some(hash)).as_str();
        let created_at = token_timestamp_to_sql(unix_millis());
        self.pool.write(|conn| async move {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).await?;
            let Some(tx) = self.claim_cap_slot(tx).await? else {
                return Ok(Insert::CapFull);
            };
            let inserted = tx.execute(
                "INSERT INTO users (id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at)
                 VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
                params![
                    id_ref,
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
                    tx.commit().await?;
                    Ok(Insert::Created(id_ref.to_string()))
                }
                Err(err) if is_unique_violation(&err) => {
                    tx.rollback().await?;
                    Ok(Insert::NameTaken)
                }
                Err(err) => Err(RegistryError::from(err)),
            }
        })
        .await
    }

    async fn reconcile_user_counter(&self) -> Result<bool> {
        self.pool.write(|conn| async move { reconcile_user_counter_overcount(&conn).await }).await
    }

    async fn claim_cap_slot(&self, tx: libsql::Transaction) -> Result<Option<libsql::Transaction>> {
        let MaxUsers::Limited(max) = self.max_users else {
            return Ok(Some(tx));
        };
        if claim_user_counter_slot(&tx, max).await? {
            return Ok(Some(tx));
        }
        tx.rollback().await?;
        Ok(None)
    }

    async fn reject_over_cap(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        if let Some(stored) = self.stored_account(username).await? {
            return self.verify_and_maybe_upgrade(username, password, stored).await;
        }
        let MaxUsers::Limited(max) = self.max_users else {
            return Err(RegistryError::RegistrationDisabled);
        };
        Err(RegistryError::TooManyUsers { max })
    }

    async fn login_after_lost_insert(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        self.sync_replica().await?;
        let Some(stored) = self.stored_account(username).await? else {
            return Err(crate::validation::confusable_username());
        };
        self.verify_and_maybe_upgrade(username, password, stored).await
    }

    async fn verify_and_maybe_upgrade(
        &self,
        username: &str,
        password: &str,
        stored: StoredLibsqlUser,
    ) -> Result<(UpsertOutcome, String)> {
        if stored.status == AccountStatus::Deactivated {
            return Err(deprovisioned_login_error(username));
        }
        if stored.status.is_deleted() {
            dummy_verify(password, DEFAULT_BCRYPT_COST).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }
        let (outcome, _) =
            verify_returning_user(username, password, &stored.credential, DEFAULT_BCRYPT_COST)
                .await?;
        if let AccountCredential::PasswordHash(ref old_hash) = stored.credential
            && is_sha_hash(old_hash)
        {
            let new_hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
            self.replace_password_hash_if_current(&stored.id, old_hash, &new_hash).await?;
        }
        Ok((outcome, stored.id))
    }
}

enum Insert {
    Created(String),
    NameTaken,
    CapFull,
}
