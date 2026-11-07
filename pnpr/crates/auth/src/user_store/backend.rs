use super::{Change, Registration, StoredAccount, UserStore, active_user_count};
use crate::{
    PasswordChange, Result, UpsertOutcome, UserBackend, UserCreation, UserRemoval, dummy_verify,
    hash_bcrypt, validation::validate_username,
};
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::RegistryError;

#[async_trait]
impl UserBackend for UserStore {
    /// * Unknown username, registration allowed → bcrypt the password,
    ///   insert, persist, return `Created`.
    /// * Known username, password matches → return `LoggedIn`.
    /// * Known username, password wrong → `Unauthenticated`.
    /// * Unknown username, registration disabled or capped →
    ///   `RegistrationDisabled` / `TooManyUsers`.
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;
        self.sync_from_disk_if_path()?;

        let existing_hash = self
            .snapshot()
            .users
            .get(username)
            .cloned();
        if let Some(stored) = existing_hash {
            return self.login_existing(username, password, stored).await;
        }

        if matches!(self.max_users, MaxUsers::Disabled) {
            dummy_verify(password, self.bcrypt_cost).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }

        self.check_registration_capacity()?;

        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        let max_users = self.max_users;
        let registration = self.update(move |users| {
            if let Some(stored) = users.get(&name) {
                return Ok(Change::Keep(Registration::Existing(stored.clone())));
            }
            // Re-check under the lock because another registration may
            // have filled the store while we were hashing.
            if let MaxUsers::Limited(max) = max_users
                && active_user_count(users) >= max
            {
                return Err(RegistryError::TooManyUsers { max });
            }
            let account = StoredAccount::new(hash);
            let id = account.id.clone();
            users.insert(name, account);
            Ok(Change::Write(Registration::Created(id)))
        });
        match registration.await? {
            Registration::Created(id) => Ok((UpsertOutcome::Created, id)),
            Registration::Existing(stored) => self.login_existing(username, password, stored).await,
        }
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        self.sync_from_disk_if_path()?;
        let mut names: Vec<String> = self
            .snapshot()
            .users
            .iter()
            .filter(|(_, hash)| hash.password_hash.as_str() != crate::DELETED_USER_HASH)
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        Ok(names)
    }

    async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
        self.sync_from_disk_if_path()?;
        let mut accounts: Vec<_> = self
            .snapshot()
            .names_by_id
            .iter()
            .map(|(id, username)| crate::UserAccount { id: id.clone(), username: username.clone() })
            .collect();
        accounts.sort_by(|left, right| left.username.cmp(&right.username));
        Ok(accounts)
    }

    async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<crate::UserAccount>> {
        self.sync_from_disk_if_path()?;
        let state = self.snapshot();
        Ok(user_ids
            .iter()
            .filter_map(|id| {
                state.names_by_id
                    .get(id)
                    .map(|username| crate::UserAccount {
                        id: id.clone(),
                        username: username.clone(),
                    })
            })
            .collect())
    }

    async fn password_hash(&self, username: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .filter(|account| account.password_hash != crate::DELETED_USER_HASH)
            .map(|account| account.password_hash.clone()))
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        validate_username(username)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        self.update(move |users| {
            if users.contains_key(&name) {
                return Ok(Change::Keep(UserCreation::NameTaken));
            }
            users.insert(name, StoredAccount::new(hash));
            Ok(Change::Write(UserCreation::Created))
        })
        .await
    }

    async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        self.replace_password_hash(user_id, hash).await
    }

    async fn clear_password(&self, user_id: &str) -> Result<PasswordChange> {
        self.replace_password_hash(user_id, crate::NO_PASSWORD_HASH.to_string()).await
    }

    async fn provision_user(&self, username: &str) -> Result<String> {
        validate_username(username)?;
        let name = username.to_string();
        self.update(move |users| {
            if users.contains_key(&name) {
                return Err(RegistryError::AdminConflict {
                    reason: format!("username {name:?} is taken"),
                });
            }
            let account = StoredAccount::new(crate::NO_PASSWORD_HASH.to_string());
            let id = account.id.clone();
            users.insert(name, account);
            Ok(Change::Write(id))
        })
        .await
    }

    async fn account_id(&self, username: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .filter(|account| account.password_hash != crate::DELETED_USER_HASH)
            .map(|account| account.id.clone()))
    }

    async fn account_name(&self, user_id: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .names_by_id
            .get(user_id)
            .cloned())
    }

    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
        validate_username(username)?;
        let id = user_id.to_string();
        let name = username.to_string();
        self.update(move |users| {
            let is_current = users
                .get(&name)
                .is_some_and(|account| {
                    account.id == id && account.password_hash != crate::DELETED_USER_HASH
                });
            if is_current {
                return Ok(Change::Keep(()));
            }
            if users.contains_key(&name) {
                return Err(RegistryError::AdminConflict {
                    reason: format!("username {name:?} is taken"),
                });
            }
            let old = users
                .iter()
                .find(|(_, account)| {
                    account.id == id && account.password_hash != crate::DELETED_USER_HASH
                })
                .map(|(name, _)| name.clone())
                .ok_or(RegistryError::NotFound)?;
            let account = users.remove(&old).ok_or(RegistryError::NotFound)?;
            users.insert(name, account);
            Ok(Change::Write(()))
        })
        .await
    }

    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval> {
        let id = user_id.to_string();
        self.update(move |users| {
            let name = users
                .iter()
                .find(|(_, account)| {
                    account.id == id && account.password_hash != crate::DELETED_USER_HASH
                })
                .map(|(name, _)| name.clone());
            let Some(name) = name else {
                return Ok(Change::Keep(UserRemoval::NoSuchUser));
            };
            let mut account = users.remove(&name).ok_or(RegistryError::NotFound)?;
            account.password_hash = crate::DELETED_USER_HASH.to_string();
            users.insert(format!("deleted:{}", account.id), account);
            Ok(Change::Write(UserRemoval::Removed))
        })
        .await
    }

    async fn check_readiness(&self) -> Result<()> {
        if let Some(path) = &self.path {
            if path.exists() {
                tokio::fs::metadata(path).await.map_err(RegistryError::Io)?;
            } else if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                tokio::fs::create_dir_all(parent).await.map_err(RegistryError::Io)?;
            }
        }
        Ok(())
    }
}

impl UserStore {
    async fn replace_password_hash(&self, user_id: &str, hash: String) -> Result<PasswordChange> {
        let id = user_id.to_string();
        self.update(move |users| {
            match users
                .values_mut()
                .find(|account| account.id == id)
            {
                Some(stored) if stored.password_hash.as_str() != crate::DELETED_USER_HASH => {
                    stored.password_hash = hash;
                    Ok(Change::Write(PasswordChange::Changed))
                }
                _ => Ok(Change::Keep(PasswordChange::NoSuchUser)),
            }
        })
        .await
    }
}
