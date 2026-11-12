use crate::{
    RegistryError, Result, dummy_verify, htpasswd::verify_sha1, is_bcrypt_hash, is_sha_hash,
    verify_bcrypt,
};
use serde::{Deserialize, Serialize};

/// Lifecycle state of an account record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Active,
    Deactivated,
    Deleted,
}

impl AccountStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deactivated => "deactivated",
            Self::Deleted => "deleted",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "active" => Some(Self::Active),
            "deactivated" => Some(Self::Deactivated),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    #[must_use]
    pub const fn is_deleted(self) -> bool {
        matches!(self, Self::Deleted)
    }
}

/// Password hash algorithm stored on an account, or [`Self::None`] for
/// passwordless and deleted accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialKind {
    None,
    Bcrypt,
    Sha1,
}

impl CredentialKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bcrypt => "bcrypt",
            Self::Sha1 => "sha1",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "bcrypt" => Some(Self::Bcrypt),
            "sha1" => Some(Self::Sha1),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_hash(hash: &str) -> Option<Self> {
        if is_bcrypt_hash(hash) {
            Some(Self::Bcrypt)
        } else if is_sha_hash(hash) {
            Some(Self::Sha1)
        } else {
            None
        }
    }

    #[cfg(any(
        feature = "backend-libsql",
        feature = "backend-postgres",
        feature = "backend-mysql"
    ))]
    pub(crate) fn for_optional_hash(password_hash: Option<&str>) -> Self {
        match password_hash {
            Some(hash) => Self::from_hash(hash).expect("requires a supported password hash"),
            None => Self::None,
        }
    }
}

/// Password credential of an active account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountCredential {
    /// Account has no password credential (for example, SSO or SCIM provisioned).
    None,
    /// Account authenticates with a bcrypt or `{SHA}` password hash.
    PasswordHash(String),
}

impl AccountCredential {
    #[must_use]
    pub fn password_hash(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::PasswordHash(hash) => Some(hash.as_str()),
        }
    }

    #[must_use]
    pub fn into_password_hash(self) -> Option<String> {
        match self {
            Self::None => None,
            Self::PasswordHash(hash) => Some(hash),
        }
    }
}

pub(crate) fn deprovisioned_login_error(username: &str) -> RegistryError {
    RegistryError::Forbidden {
        user: username.to_string(),
        action: "sign in to",
        resource: "this account, which an identity provider deprovisioned".to_string(),
    }
}

pub(crate) fn validate_credential_pair(
    kind: CredentialKind,
    password_hash: Option<&str>,
) -> std::result::Result<(), &'static str> {
    match (kind, password_hash) {
        (CredentialKind::None, None) => Ok(()),
        (CredentialKind::None, Some(_)) => Err("passwordless account must not have password_hash"),
        (CredentialKind::Bcrypt, Some(hash)) if is_bcrypt_hash(hash) => Ok(()),
        (CredentialKind::Sha1, Some(hash)) if is_sha_hash(hash) => Ok(()),
        (CredentialKind::Bcrypt | CredentialKind::Sha1, None) => {
            Err("password account is missing password_hash")
        }
        (CredentialKind::Bcrypt | CredentialKind::Sha1, Some(_)) => {
            Err("password_hash does not match credential_kind")
        }
    }
}

#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) fn decode_sql_account_state(
    user_id: &str,
    raw_status: &str,
    raw_kind: &str,
    password_hash: Option<String>,
) -> Result<(AccountStatus, AccountCredential)> {
    let status = AccountStatus::parse(raw_status)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("user {user_id} has an unrecognized status {raw_status:?}"),
        })?;
    let kind = CredentialKind::parse(raw_kind)
        .ok_or_else(|| RegistryError::Internal {
            reason: format!("user {user_id} has an unrecognized credential_kind {raw_kind:?}"),
        })?;
    if status.is_deleted() && (kind != CredentialKind::None || password_hash.is_some()) {
        return Err(RegistryError::Internal {
            reason: format!("user {user_id}: deleted accounts must not retain credentials"),
        });
    }
    if let Err(reason) = validate_credential_pair(kind, password_hash.as_deref()) {
        return Err(RegistryError::Internal { reason: format!("user {user_id}: {reason}") });
    }
    let credential = match password_hash {
        Some(hash) => AccountCredential::PasswordHash(hash),
        None => AccountCredential::None,
    };
    Ok((status, credential))
}

/// Verify `password` against an existing user's `stored` credential,
/// mapping the result to the login outcome a returning user expects:
/// `LoggedIn` on a match, `Unauthenticated` otherwise.
pub async fn verify_returning_user(
    username: &str,
    password: &str,
    stored: &AccountCredential,
    bcrypt_cost: u32,
) -> Result<(UpsertOutcome, String)> {
    let valid = match stored {
        AccountCredential::None => {
            dummy_verify(password, bcrypt_cost).await;
            false
        }
        AccountCredential::PasswordHash(hash) if is_sha_hash(hash) => {
            let ok = verify_sha1(password, hash);
            if !ok {
                dummy_verify(password, bcrypt_cost).await;
            }
            ok
        }
        AccountCredential::PasswordHash(hash) => {
            verify_bcrypt(password.to_string(), hash.clone()).await?
        }
    };
    if valid {
        Ok((UpsertOutcome::LoggedIn, username.to_string()))
    } else {
        Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// The user didn't exist; we created the account.
    Created,
    /// The user existed and the password matched.
    LoggedIn,
}

/// What [`crate::UserBackend::create_user`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserCreation {
    /// The name was free; the account is stored under this immutable ID.
    Created(String),
    /// The name was taken, so the account it names is untouched.
    NameTaken,
}

/// What [`crate::UserBackend::set_password`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordChange {
    /// The account's password is the new one.
    Changed,
    /// There is no such account, so no password was stored.
    NoSuchUser,
}

/// The outcome of removing an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRemoval {
    /// The account is gone.
    Removed,
    /// There was no such account.
    NoSuchUser,
}

/// A non-deleted account's login name and monotonic rename revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLogin {
    pub username: String,
    pub revision: u64,
}

/// What [`crate::UserBackend::rename_user_if_current`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRename {
    /// The account's login name is now at the returned revision.
    Renamed(u64),
    /// Another writer changed the account's revision first.
    CurrentChanged,
}

/// Stored lifecycle status and monotonic status revision for an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountStatusRevision {
    pub status: AccountStatus,
    pub revision: u64,
}

/// Target status of a generation-ordered account lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountLifecycle {
    Active,
    Deactivated,
}

impl AccountLifecycle {
    #[must_use]
    pub const fn as_status(self) -> AccountStatus {
        match self {
            Self::Active => AccountStatus::Active,
            Self::Deactivated => AccountStatus::Deactivated,
        }
    }
}

/// What [`crate::UserBackend::apply_account_status`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatusOutcome {
    /// The transition was applied at the supplied status revision; deactivating
    /// also cleared any stored password.
    Applied,
    /// A newer status revision (or an already applied activation at the same
    /// revision) superseded this transition.
    Superseded,
    /// No non-deleted account has the requested ID.
    Missing,
}

#[must_use]
pub(crate) const fn should_apply_lifecycle(
    actual_status: AccountStatus,
    actual_revision: u64,
    revision: u64,
    lifecycle: AccountLifecycle,
) -> bool {
    match lifecycle {
        AccountLifecycle::Active => actual_revision < revision,
        AccountLifecycle::Deactivated => {
            actual_revision < revision
                || (actual_revision == revision
                    && match actual_status {
                        AccountStatus::Deactivated => true,
                        AccountStatus::Active | AccountStatus::Deleted => false,
                    })
        }
    }
}
