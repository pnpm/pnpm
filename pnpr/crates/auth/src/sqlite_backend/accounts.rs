use super::SqliteAuth;
use crate::{
    AccountCredential, AccountLifecycle, AccountLogin, AccountStatus, AccountStatusOutcome,
    AccountStatusRevision, CredentialKind, PasswordChange, UpsertOutcome, UserAccount,
    UserCreation, UserRemoval,
    account::{decode_sql_account_state, deprovisioned_login_error, should_apply_lifecycle},
    dummy_verify, hash_bcrypt, is_sha_hash, unix_millis,
    validation::{
        confusable_username, token_timestamp_to_sql, username_conflict, username_skeleton_hash,
        validate_password, validate_username,
    },
    verify_returning_user,
};
use pnpr_config::MaxUsers;
use pnpr_error::{RegistryError, Result};
use pnpr_sql::adapter::{Connection, Param, Row};
use std::collections::HashMap;

pub(super) struct StoredAccount {
    pub(super) id: String,
    pub(super) status: AccountStatus,
    pub(super) credential: AccountCredential,
}

/// Whether a login name may be given to an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameCheck {
    Free,
    /// Another account holds exactly this name.
    Taken,
}

/// A generation-ordered lifecycle transition.
#[derive(Debug, Clone, Copy)]
pub(super) struct Lifecycle {
    pub(super) revision: u64,
    pub(super) sql_revision: i64,
    pub(super) target: AccountLifecycle,
}

enum Registration {
    Created(String),
    Existing,
}

impl SqliteAuth {
    pub(super) async fn stored_account(&self, username: &str) -> Result<Option<StoredAccount>> {
        let username = username.to_string();
        self.read(move |connection| stored_account(connection, &username)).await
    }

    pub(super) async fn add_or_login_account(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;
        validate_password(password)?;
        if let Some(stored) = self.stored_account(username).await? {
            return self.verify_and_maybe_upgrade(username, password, stored).await;
        }
        let max = match self.max_users {
            MaxUsers::Disabled => {
                dummy_verify(password, self.bcrypt_cost).await;
                return Err(RegistryError::Unauthenticated {
                    resource: format!("user {username:?}"),
                });
            }
            MaxUsers::Unlimited => None,
            MaxUsers::Limited(max) => Some(max),
        };
        if let Some(max) = max
            && self.read(registered_count).await? >= max
        {
            return Err(RegistryError::TooManyUsers { max });
        }
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let name = username.to_string();
        let registration = self.write(move |connection| {
            if check_name(connection, &name, None)? == NameCheck::Taken {
                return Ok(Registration::Existing);
            }
            if let Some(max) = max
                && registered_count(connection)? >= max
            {
                return Err(RegistryError::TooManyUsers { max });
            }
            insert_account(connection, &name, Some(&hash)).map(Registration::Created)
        })
        .await?;
        match registration {
            Registration::Created(id) => Ok((UpsertOutcome::Created, id)),
            Registration::Existing => {
                let stored =
                    self.stored_account(username).await?.ok_or_else(confusable_username)?;
                self.verify_and_maybe_upgrade(username, password, stored).await
            }
        }
    }

    async fn verify_and_maybe_upgrade(
        &self,
        username: &str,
        password: &str,
        stored: StoredAccount,
    ) -> Result<(UpsertOutcome, String)> {
        if stored.status == AccountStatus::Deactivated {
            return Err(deprovisioned_login_error(username));
        }
        if stored.status.is_deleted() {
            dummy_verify(password, self.bcrypt_cost).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }
        let (outcome, _) =
            verify_returning_user(username, password, &stored.credential, self.bcrypt_cost).await?;
        if let AccountCredential::PasswordHash(old_hash) = &stored.credential
            && is_sha_hash(old_hash)
        {
            let new_hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
            self.replace_password_hash_if_current(&stored.id, old_hash, new_hash).await?;
        }
        Ok((outcome, stored.id))
    }

    pub(super) async fn create_account(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserCreation> {
        validate_username(username)?;
        validate_password(password)?;
        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        let username = username.to_string();
        self.write(move |connection| match check_name(connection, &username, None)? {
            NameCheck::Taken => Ok(UserCreation::NameTaken),
            NameCheck::Free => {
                insert_account(connection, &username, Some(&hash)).map(UserCreation::Created)
            }
        })
        .await
    }

    pub(super) async fn provision_account(&self, username: &str) -> Result<String> {
        validate_username(username)?;
        let username = username.to_string();
        self.write(move |connection| match check_name(connection, &username, None)? {
            NameCheck::Taken => Err(username_conflict(&username, true)),
            NameCheck::Free => insert_account(connection, &username, None),
        })
        .await
    }

    pub(super) async fn replace_password_hash(
        &self,
        user_id: &str,
        hash: Option<String>,
    ) -> Result<PasswordChange> {
        let user_id = user_id.to_string();
        let changed =
            self.write(move |connection| set_password_hash(connection, &user_id, hash.as_deref()))
                .await?;
        Ok(if changed { PasswordChange::Changed } else { PasswordChange::NoSuchUser })
    }

    pub(super) async fn replace_password_hash_if_current(
        &self,
        user_id: &str,
        expected_hash: &str,
        hash: String,
    ) -> Result<bool> {
        let (user_id, expected_hash) = (user_id.to_string(), expected_hash.to_string());
        self.write(move |connection| {
            let kind = CredentialKind::for_optional_hash(Some(&hash)).as_str();
            let changed = connection.query(
                "UPDATE users SET credential_kind = ?1, password_hash = ?2
                 WHERE id = ?3 AND password_hash = ?4 AND status = 'active'
                 RETURNING id",
                &[
                    Param::Text(kind),
                    Param::Text(&hash),
                    Param::Text(&user_id),
                    Param::Text(&expected_hash),
                ],
            )?;
            Ok(!changed.is_empty())
        })
        .await
    }
}

pub(super) fn stored_account(
    connection: &dyn Connection,
    username: &str,
) -> Result<Option<StoredAccount>> {
    let Some(mut row) = connection.query_optional(
        "SELECT id, status, credential_kind, password_hash FROM users WHERE username = ?1",
        &[Param::Text(username)],
    )?
    else {
        return Ok(None);
    };
    let id: String = row.get(0)?;
    let (status, credential) =
        decode_sql_account_state(&id, &row.get::<String>(1)?, &row.get::<String>(2)?, row.get(3)?)?;
    Ok(Some(StoredAccount { id, status, credential }))
}

/// Accounts that hold a registration slot: every account that is not deleted.
pub(super) fn registered_count(connection: &dyn Connection) -> Result<u64> {
    let count: i64 = connection
        .query_one("SELECT COUNT(*) FROM users WHERE status <> 'deleted'", &[])?
        .get(0)?;
    Ok(count.max(0) as u64)
}

/// Whether `username` is free for the account `owner` (any account when
/// `None`). A name that only looks like another account's is refused.
pub(super) fn check_name(
    connection: &dyn Connection,
    username: &str,
    owner: Option<&str>,
) -> Result<NameCheck> {
    let skeleton = username_skeleton_hash(username);
    let rows = connection.query(
        "SELECT id, username FROM users WHERE username = ?1 OR username_skeleton_hash = ?2",
        &[Param::Text(username), Param::Text(&skeleton)],
    )?;
    let mut check = NameCheck::Free;
    for mut row in rows {
        if owner.is_some_and(|owner| {
            row.get::<String>(0)
                .is_ok_and(|id| id == owner)
        }) {
            continue;
        }
        if row.get::<String>(1)? != username {
            return Err(confusable_username());
        }
        check = NameCheck::Taken;
    }
    Ok(check)
}

/// Insert an active account under a fresh ID, which it returns. The caller
/// checked the name with [`check_name`] in the same transaction.
pub(super) fn insert_account(
    connection: &dyn Connection,
    username: &str,
    password_hash: Option<&str>,
) -> Result<String> {
    let id = crate::new_principal_id();
    let kind = CredentialKind::for_optional_hash(password_hash).as_str();
    connection.execute(
        "INSERT INTO users (id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at)
         VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
        &[
            Param::Text(&id),
            Param::Text(username),
            Param::Text(&username_skeleton_hash(username)),
            Param::Text(kind),
            password_hash.map_or(Param::Null, Param::Text),
            Param::Integer(token_timestamp_to_sql(unix_millis())),
        ],
    )?;
    Ok(id)
}

/// Replace the credential of a non-deleted account. Returns whether one was
/// changed.
pub(super) fn set_password_hash(
    connection: &dyn Connection,
    user_id: &str,
    password_hash: Option<&str>,
) -> Result<bool> {
    let kind = CredentialKind::for_optional_hash(password_hash).as_str();
    let changed = connection.query(
        "UPDATE users SET credential_kind = ?2, password_hash = ?3
         WHERE id = ?1 AND status <> 'deleted'
         RETURNING id",
        &[Param::Text(user_id), Param::Text(kind), password_hash.map_or(Param::Null, Param::Text)],
    )?;
    Ok(!changed.is_empty())
}

pub(super) fn list_accounts(connection: &dyn Connection) -> Result<Vec<UserAccount>> {
    collect_accounts(connection.query(
        "SELECT id, username FROM users WHERE status <> 'deleted' ORDER BY username",
        &[],
    )?)
}

pub(super) fn accounts_by_id(connection: &dyn Connection, ids: &str) -> Result<Vec<UserAccount>> {
    collect_accounts(connection.query(
        "SELECT id, username FROM users
         WHERE id IN (SELECT value FROM json_each(?1)) AND status <> 'deleted'",
        &[Param::Text(ids)],
    )?)
}

fn collect_accounts(rows: Vec<Row>) -> Result<Vec<UserAccount>> {
    rows.into_iter()
        .map(|mut row| Ok(UserAccount { id: row.get(0)?, username: row.get(1)? }))
        .collect()
}

pub(super) fn active_account_name(
    connection: &dyn Connection,
    user_id: &str,
) -> Result<Option<String>> {
    connection
        .query_optional(
            "SELECT username FROM users WHERE id = ?1 AND status = 'active'",
            &[Param::Text(user_id)],
        )?
        .map(|mut row| row.get(0))
        .transpose()
}

pub(super) fn account_status(
    connection: &dyn Connection,
    user_id: &str,
) -> Result<Option<AccountStatus>> {
    connection
        .query_optional("SELECT status FROM users WHERE id = ?1", &[Param::Text(user_id)])?
        .map(|mut row| parse_status(user_id, &row.get::<String>(0)?))
        .transpose()
}

pub(super) fn status_revisions(
    connection: &dyn Connection,
    ids: &str,
) -> Result<HashMap<String, AccountStatusRevision>> {
    let rows = connection.query(
        "SELECT id, status, status_revision FROM users
         WHERE id IN (SELECT value FROM json_each(?1))",
        &[Param::Text(ids)],
    )?;
    rows.into_iter()
        .map(|mut row| {
            let id: String = row.get(0)?;
            let status = parse_status(&id, &row.get::<String>(1)?)?;
            let revision = u64::try_from(row.get::<i64>(2)?).unwrap_or(0);
            Ok((id, AccountStatusRevision::reconciled(status, revision)))
        })
        .collect()
}

pub(super) fn account_login(
    connection: &dyn Connection,
    user_id: &str,
) -> Result<Option<AccountLogin>> {
    connection
        .query_optional(
            "SELECT username, name_revision FROM users WHERE id = ?1 AND status <> 'deleted'",
            &[Param::Text(user_id)],
        )?
        .map(|mut row| {
            let username = row.get(0)?;
            let revision = u64::try_from(row.get::<i64>(1)?).unwrap_or(0);
            Ok(AccountLogin { username, revision })
        })
        .transpose()
}

/// Give the non-deleted account `user_id` the login name `username` at name
/// revision `revision`.
pub(super) fn rename_account(
    connection: &dyn Connection,
    user_id: &str,
    username: &str,
    revision: i64,
) -> Result<()> {
    if check_name(connection, username, Some(user_id))? == NameCheck::Taken {
        return Err(username_conflict(username, true));
    }
    let renamed = connection.query(
        "UPDATE users SET username = ?2, username_skeleton_hash = ?3, name_revision = ?4
         WHERE id = ?1 AND status <> 'deleted'
         RETURNING id",
        &[
            Param::Text(user_id),
            Param::Text(username),
            Param::Text(&username_skeleton_hash(username)),
            Param::Integer(revision),
        ],
    )?;
    if renamed.is_empty() {
        return Err(RegistryError::NotFound);
    }
    Ok(())
}

/// Retire `user_id` under a generated deleted name, freeing its login name
/// and dropping its credential. The ID is never reassigned.
pub(super) fn delete_account(connection: &dyn Connection, user_id: &str) -> Result<UserRemoval> {
    let removed = connection.query(
        "UPDATE users
         SET username = 'deleted:' || id,
             username_skeleton_hash = NULL,
             status = 'deleted',
             credential_kind = 'none',
             password_hash = NULL
         WHERE id = ?1 AND status <> 'deleted'
         RETURNING id",
        &[Param::Text(user_id)],
    )?;
    Ok(if removed.is_empty() { UserRemoval::NoSuchUser } else { UserRemoval::Removed })
}

/// Apply `transition` to `user_id`, revoking its tokens in the same
/// transaction when deactivating.
pub(super) fn apply_lifecycle(
    connection: &dyn Connection,
    user_id: &str,
    transition: Lifecycle,
) -> Result<AccountStatusOutcome> {
    let Some(mut row) = connection.query_optional(
        "SELECT status, status_revision FROM users WHERE id = ?1 AND status <> 'deleted'",
        &[Param::Text(user_id)],
    )?
    else {
        return Ok(AccountStatusOutcome::Missing);
    };
    let actual_status = parse_status(user_id, &row.get::<String>(0)?)?;
    let actual_revision = u64::try_from(row.get::<i64>(1)?).unwrap_or(0);
    if !should_apply_lifecycle(
        actual_status,
        actual_revision,
        transition.revision,
        transition.target,
    ) {
        return Ok(AccountStatusOutcome::Superseded);
    }
    let params = [Param::Text(user_id), Param::Integer(transition.sql_revision)];
    match transition.target {
        AccountLifecycle::Active => connection.execute(
            "UPDATE users SET status = 'active', status_revision = ?2 WHERE id = ?1",
            &params,
        )?,
        AccountLifecycle::Deactivated => {
            connection.execute(
                "UPDATE users
                 SET status = 'deactivated',
                     credential_kind = 'none',
                     password_hash = NULL,
                     status_revision = ?2
                 WHERE id = ?1",
                &params,
            )?;
            connection.execute("DELETE FROM tokens WHERE user_id = ?1", &params[..1])?;
        }
    }
    Ok(AccountStatusOutcome::Applied)
}

fn parse_status(user_id: &str, raw: &str) -> Result<AccountStatus> {
    AccountStatus::parse(raw)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("user {user_id} has an unrecognized status {raw:?}"),
        })
}
