use super::{
    Change, Registration, StoredAccount, UserStore, check_username_skeleton, registered_user_count,
    stored::{
        apply_stored_lifecycle, checked_next_revision, find_active_login, move_account_login,
    },
};
use crate::{
    AccountCredential, AccountLifecycle, AccountStatus, AccountStatusOutcome, PasswordChange,
    Result, UpsertOutcome, UserBackend, UserCreation, UserRemoval, dummy_verify, hash_bcrypt,
    validation::{status_revision_to_sql, validate_username},
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
        crate::validation::validate_password(password)?;
        self.sync_from_disk_if_path()?;

        let existing = self
            .snapshot()
            .users
            .get(username)
            .cloned();
        if let Some(stored) = existing {
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
                && registered_user_count(users) >= max
            {
                return Err(RegistryError::TooManyUsers { max });
            }
            check_username_skeleton(users, &name, None)?;
            let account = StoredAccount::new_with_hash(hash);
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
            .filter(|(_, account)| !account.status.is_deleted())
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        Ok(names)
    }

    async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
        self.sync_from_disk_if_path()?;
        let mut accounts: Vec<_> = self
            .snapshot()
            .users
            .iter()
            .filter(|(_, account)| !account.status.is_deleted())
            .map(|(username, account)| crate::UserAccount {
                id: account.id.clone(),
                username: username.clone(),
            })
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

    async fn credential(&self, username: &str) -> Result<Option<AccountCredential>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .filter(|account| account.status.is_active())
            .map(StoredAccount::credential))
    }

    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        validate_username(username)?;
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        self.update(move |users| {
            if users.contains_key(&name) {
                return Ok(Change::Keep(UserCreation::NameTaken));
            }
            check_username_skeleton(users, &name, None)?;
            let account = StoredAccount::new_with_hash(hash);
            let id = account.id.clone();
            users.insert(name, account);
            Ok(Change::Write(UserCreation::Created(id)))
        })
        .await
    }

    async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange> {
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        self.replace_password_hash(user_id, Some(hash)).await
    }

    async fn set_password_if_current(
        &self,
        user_id: &str,
        password: &str,
        expected_hash: &str,
    ) -> Result<bool> {
        crate::validation::validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let (id, expected) = (user_id.to_string(), expected_hash.to_string());
        self.update(move |users| {
            let Some(stored) = users
                .values_mut()
                .find(|account| account.id == id)
            else {
                return Ok(Change::Keep(false));
            };
            if !stored.status.is_active()
                || stored.password_hash.as_deref() != Some(expected.as_str())
            {
                return Ok(Change::Keep(false));
            }
            stored.set_hash(hash);
            Ok(Change::Write(true))
        })
        .await
    }

    async fn clear_password(&self, user_id: &str) -> Result<PasswordChange> {
        self.replace_password_hash(user_id, None).await
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
            check_username_skeleton(users, &name, None)?;
            let account = StoredAccount::new_passwordless();
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
            .filter(|account| !account.status.is_deleted())
            .map(|account| account.id.clone()))
    }

    async fn active_account_id(&self, username: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        Ok(self
            .snapshot()
            .users
            .get(username)
            .filter(|account| account.status.is_active())
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

    async fn active_account_name(&self, user_id: &str) -> Result<Option<String>> {
        self.sync_from_disk_if_path()?;
        let state = self.snapshot();
        if !state.active_ids.contains(user_id) {
            return Ok(None);
        }
        Ok(state.names_by_id.get(user_id).cloned())
    }

    async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>> {
        self.sync_from_disk_if_path()?;
        let state = self.snapshot();
        if let Some(username) = state.names_by_id.get(user_id) {
            return Ok(state.users.get(username).map(|account| account.status));
        }
        Ok(state.users
            .get(&format!("deleted:{user_id}"))
            .map(|account| account.status))
    }

    async fn status_revisions(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::HashMap<String, crate::AccountStatusRevision>> {
        self.sync_from_disk_if_path()?;
        let state = self.snapshot();
        let mut revisions = std::collections::HashMap::with_capacity(user_ids.len());
        for id in user_ids {
            let account = state.names_by_id
                .get(id)
                .and_then(|username| state.users.get(username))
                .or_else(|| state.users.get(&format!("deleted:{id}")));
            if let Some(account) = account {
                revisions.insert(
                    id.clone(),
                    crate::AccountStatusRevision {
                        status: account.status,
                        revision: account.status_revision,
                        pending_token_revocation: account.pending_token_revocation,
                    },
                );
            }
        }
        Ok(revisions)
    }

    async fn apply_account_status(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: AccountLifecycle,
    ) -> Result<AccountStatusOutcome> {
        let _ = status_revision_to_sql(revision)?;
        let id = user_id.to_string();
        let revoke_id = id.clone();
        let stores = self.bound_tokens();
        let post_stores = stores.clone();
        self.update_with_post_commit(
            move |users| apply_stored_lifecycle(users, &id, revision, lifecycle, &stores),
            move |outcome, users| {
                super::stored::complete_lifecycle_revocation(
                    users,
                    &revoke_id,
                    *outcome,
                    lifecycle,
                    &post_stores,
                )
            },
        )
        .await
    }

    fn revokes_tokens_in_status_transition(&self) -> bool {
        !self.bound_tokens().is_empty()
    }

    async fn account_login(&self, user_id: &str) -> Result<Option<crate::AccountLogin>> {
        self.sync_from_disk_if_path()?;
        let state = self.snapshot();
        let Some(username) = state.names_by_id.get(user_id) else {
            return Ok(None);
        };
        let revision = state.users
            .get(username)
            .map(|account| account.name_revision)
            .unwrap_or(0);
        Ok(Some(crate::AccountLogin { username: username.clone(), revision }))
    }

    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
        validate_username(username)?;
        let (id, name) = (user_id.to_string(), username.to_string());
        self.update(move |users| {
            let (old, current_revision) = find_active_login(users, &id)?;
            let next_revision = checked_next_revision(current_revision)?;
            move_account_login(users, &id, &old, name, next_revision)?;
            Ok(Change::Write(()))
        })
        .await
    }

    async fn rename_user_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename> {
        validate_username(username)?;
        let next_revision = checked_next_revision(expected_revision)?;
        let (id, name) = (user_id.to_string(), username.to_string());
        self.update(move |users| {
            let (old, current_revision) = find_active_login(users, &id)?;
            if current_revision != expected_revision {
                return Ok(Change::Keep(crate::UserRename::CurrentChanged));
            }
            move_account_login(users, &id, &old, name, next_revision)?;
            Ok(Change::Write(crate::UserRename::Renamed(next_revision)))
        })
        .await
    }

    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval> {
        let id = user_id.to_string();
        self.update(move |users| {
            let name = users
                .iter()
                .find(|(_, account)| account.id == id && !account.status.is_deleted())
                .map(|(name, _)| name.clone());
            let Some(name) = name else {
                return Ok(Change::Keep(UserRemoval::NoSuchUser));
            };
            let mut account = users.remove(&name).ok_or(RegistryError::NotFound)?;
            account.status = AccountStatus::Deleted;
            account.clear_hash();
            account.pending_token_revocation = false;
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
    async fn replace_password_hash(
        &self,
        user_id: &str,
        hash: Option<String>,
    ) -> Result<PasswordChange> {
        let id = user_id.to_string();
        self.update(move |users| {
            match users
                .values_mut()
                .find(|account| account.id == id)
            {
                Some(stored) if !stored.status.is_deleted() => {
                    match hash {
                        Some(hash) => stored.set_hash(hash),
                        None => stored.clear_hash(),
                    }
                    Ok(Change::Write(PasswordChange::Changed))
                }
                _ => Ok(Change::Keep(PasswordChange::NoSuchUser)),
            }
        })
        .await
    }
}
