use super::{Change, Registration, UserStore, active_user_count};
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
            users.insert(name, hash);
            Ok(Change::Write(Registration::Created))
        });
        match registration.await? {
            Registration::Created => Ok((UpsertOutcome::Created, username.to_string())),
            Registration::Existing(stored) => self.login_existing(username, password, stored).await,
        }
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        self.sync_from_disk_if_path()?;
        let mut names: Vec<String> = self
            .snapshot()
            .users
            .iter()
            .filter(|(_, hash)| hash.as_str() != crate::DELETED_USER_HASH)
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        Ok(names)
    }

    async fn password_hash(&self, username: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .filter(|hash| hash.as_str() != crate::DELETED_USER_HASH)
            .cloned())
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        validate_username(username)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        self.update(move |users| {
            if users.contains_key(&name) {
                return Ok(Change::Keep(UserCreation::NameTaken));
            }
            users.insert(name, hash);
            Ok(Change::Write(UserCreation::Created))
        })
        .await
    }

    async fn set_password(&self, username: &str, password: &str) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        self.update(move |users| match users.get_mut(&name) {
            Some(stored) if stored.as_str() != crate::DELETED_USER_HASH => {
                *stored = hash;
                Ok(Change::Write(PasswordChange::Changed))
            }
            _ => Ok(Change::Keep(PasswordChange::NoSuchUser)),
        })
        .await
    }

    async fn delete_user(&self, username: &str) -> Result<UserRemoval> {
        let name = username.to_string();
        self.update(move |users| {
            if users
                .get(&name)
                .is_none_or(|hash| hash == crate::DELETED_USER_HASH)
            {
                return Ok(Change::Keep(UserRemoval::NoSuchUser));
            }
            users.remove(&name);
            Ok(Change::Write(UserRemoval::Removed))
        })
        .await
    }

    async fn is_retired(&self, username: &str) -> Result<bool> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .is_some_and(|hash| hash == crate::DELETED_USER_HASH))
    }

    async fn retire_user(&self, username: &str) -> Result<UserRemoval> {
        validate_username(username)?;
        let name = username.to_string();
        self.update(move |users| {
            let previous = users.insert(name, crate::DELETED_USER_HASH.to_string());
            match previous {
                Some(hash) if hash == crate::DELETED_USER_HASH => {
                    Ok(Change::Keep(UserRemoval::NoSuchUser))
                }
                Some(_) => Ok(Change::Write(UserRemoval::Removed)),
                None => Ok(Change::Write(UserRemoval::NoSuchUser)),
            }
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
