use super::{
    AccountCredential,
    AccountStatus,
    AuthSqlBackend,
    DEFAULT_BCRYPT_COST,
    InsertUser,
    MaxUsers,
    PasswordChange,
    RegistryError,
    Result,
    SqlAuth,
    UpsertOutcome,
    UserCreation,
    UserRemoval,
    dummy_verify,
    hash_bcrypt,
    validate_username,
    with_auth_timeout,
};
use std::collections::HashMap;

impl<Db> SqlAuth<Db> {
    pub(super) async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)>
    where
        Db: AuthSqlBackend,
    {
        validate_username(username)?;
        crate::validation::validate_password(password)?;

        if let Some(stored) = with_auth_timeout(self.timeout, self.db.stored_user(username)).await?
        {
            return self.verify_and_maybe_upgrade(stored, password).await;
        }

        if matches!(self.max_users, MaxUsers::Disabled) {
            dummy_verify(password, DEFAULT_BCRYPT_COST).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }

        self.check_registration_capacity().await?;

        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        match self.db.insert_user(username, Some(&hash), self.max_users).await? {
            InsertUser::Created(id) => Ok((UpsertOutcome::Created, id)),
            InsertUser::Existing(stored) => self.verify_and_maybe_upgrade(stored, password).await,
            InsertUser::Confusable => Err(crate::validation::confusable_username()),
            InsertUser::CapReached => match self.max_users {
                MaxUsers::Limited(max) => Err(RegistryError::TooManyUsers { max }),
                MaxUsers::Disabled | MaxUsers::Unlimited => {
                    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
                }
            },
        }
    }

    pub(super) async fn list_users(&self) -> Result<Vec<String>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.list_usernames()).await
    }

    pub(super) async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.list_accounts()).await
    }

    pub(super) async fn accounts_by_id(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<crate::UserAccount>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.accounts_by_id(user_ids)).await
    }

    pub(super) async fn credential(&self, username: &str) -> Result<Option<AccountCredential>>
    where
        Db: AuthSqlBackend,
    {
        let stored = with_auth_timeout(self.timeout, self.db.stored_user(username)).await?;
        Ok(stored.filter(|stored| stored.status.is_active()).map(|stored| stored.credential))
    }

    /// Inserts as an uncapped store would, so the `users` counter still
    /// counts the new user.
    pub(super) async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation>
    where
        Db: AuthSqlBackend,
    {
        validate_username(username)?;
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        match self.db.insert_user(username, Some(&hash), MaxUsers::Unlimited).await? {
            InsertUser::Created(id) => Ok(UserCreation::Created(id)),
            // An uncapped insert cannot reach the cap.
            InsertUser::Existing(_) | InsertUser::CapReached => Ok(UserCreation::NameTaken),
            InsertUser::Confusable => Err(crate::validation::confusable_username()),
        }
    }

    pub(super) async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange>
    where
        Db: AuthSqlBackend,
    {
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        self.db.update_password_hash(user_id, Some(&hash)).await
    }

    pub(super) async fn set_password_if_current(
        &self,
        user_id: &str,
        password: &str,
        expected_hash: &str,
    ) -> Result<bool>
    where
        Db: AuthSqlBackend,
    {
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        self.db.update_user_hash_if_current(user_id, expected_hash, &hash).await
    }

    pub(super) async fn clear_password(&self, user_id: &str) -> Result<PasswordChange>
    where
        Db: AuthSqlBackend,
    {
        self.db.update_password_hash(user_id, None).await
    }

    pub(super) async fn delete_user(&self, user_id: &str) -> Result<UserRemoval>
    where
        Db: AuthSqlBackend,
    {
        self.db.delete_user(user_id).await
    }

    pub(super) async fn provision_user(&self, username: &str) -> Result<String>
    where
        Db: AuthSqlBackend,
    {
        validate_username(username)?;
        match self.db.insert_user(username, None, MaxUsers::Unlimited).await? {
            InsertUser::Created(id) => Ok(id),
            InsertUser::Confusable => Err(crate::validation::confusable_username()),
            _ => Err(RegistryError::AdminConflict {
                reason: format!("username {username:?} is taken"),
            }),
        }
    }

    pub(super) async fn account_id(&self, username: &str) -> Result<Option<String>>
    where
        Db: AuthSqlBackend,
    {
        Ok(with_auth_timeout(self.timeout, self.db.stored_user(username)).await?
            .filter(|account| !account.status.is_deleted())
            .map(|account| account.id))
    }

    pub(super) async fn active_account_id(&self, username: &str) -> Result<Option<String>>
    where
        Db: AuthSqlBackend,
    {
        Ok(with_auth_timeout(self.timeout, self.db.stored_user(username)).await?
            .filter(|account| account.status.is_active())
            .map(|account| account.id))
    }

    pub(super) async fn account_name(&self, user_id: &str) -> Result<Option<String>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.account_name(user_id)).await
    }

    pub(super) async fn active_account_name(&self, user_id: &str) -> Result<Option<String>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.active_account_name(user_id)).await
    }

    pub(super) async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.account_status(user_id)).await
    }

    pub(super) async fn status_revisions(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, crate::AccountStatusRevision>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.status_revisions(user_ids)).await
    }

    pub(super) async fn apply_account_status(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: crate::AccountLifecycle,
    ) -> Result<crate::AccountStatusOutcome>
    where
        Db: AuthSqlBackend,
    {
        self.db.apply_account_status(user_id, revision, lifecycle).await
    }

    pub(super) async fn account_login(&self, user_id: &str) -> Result<Option<crate::AccountLogin>>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.account_login(user_id)).await
    }

    pub(super) async fn rename_user(&self, user_id: &str, username: &str) -> Result<()>
    where
        Db: AuthSqlBackend,
    {
        validate_username(username)?;
        self.db.rename_user(user_id, username).await
    }

    pub(super) async fn rename_user_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename>
    where
        Db: AuthSqlBackend,
    {
        validate_username(username)?;
        self.db.rename_user_if_current(user_id, expected_revision, username).await
    }
}
