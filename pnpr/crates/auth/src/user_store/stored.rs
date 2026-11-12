use super::{
    AccountCredential, AccountStatus, Change, CredentialKind, HashMap, HashSet, unix_millis,
};
use crate::{
    AccountLifecycle, AccountStatusOutcome,
    account::{should_apply_lifecycle, validate_credential_pair},
};

/// Account records indexed by their current login name.
pub(super) type Users = HashMap<String, StoredAccount>;

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if is called as f(&field)"
)]
const fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredAccount {
    pub(super) id: String,
    pub(super) status: AccountStatus,
    pub(super) credential_kind: CredentialKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) password_hash: Option<String>,
    pub(super) created_at: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(super) name_revision: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(super) status_revision: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) pending_token_revocation: bool,
}

impl StoredAccount {
    pub(super) fn new_with_hash(password_hash: String) -> Self {
        let credential_kind = CredentialKind::from_hash(&password_hash)
            .expect("new_with_hash requires a supported password hash");
        Self {
            id: crate::new_principal_id(),
            status: AccountStatus::Active,
            credential_kind,
            password_hash: Some(password_hash),
            created_at: unix_millis(),
            name_revision: 0,
            status_revision: 0,
            pending_token_revocation: false,
        }
    }

    pub(super) fn new_passwordless() -> Self {
        Self {
            id: crate::new_principal_id(),
            status: AccountStatus::Active,
            credential_kind: CredentialKind::None,
            password_hash: None,
            created_at: unix_millis(),
            name_revision: 0,
            status_revision: 0,
            pending_token_revocation: false,
        }
    }

    pub(super) fn credential(&self) -> AccountCredential {
        match &self.password_hash {
            Some(hash) => AccountCredential::PasswordHash(hash.clone()),
            None => AccountCredential::None,
        }
    }

    pub(super) fn set_hash(&mut self, password_hash: String) {
        self.credential_kind = CredentialKind::from_hash(&password_hash)
            .expect("set_hash requires a supported password hash");
        self.password_hash = Some(password_hash);
    }

    pub(super) fn clear_hash(&mut self) {
        self.credential_kind = CredentialKind::None;
        self.password_hash = None;
    }
}

pub(super) fn parse_accounts(raw: &str) -> std::result::Result<Users, String> {
    let accounts: Users = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let mut ids = HashSet::new();
    let mut skeletons = HashMap::new();
    for (name, account) in &accounts {
        validate_stored_account(name, account, &mut skeletons)?;
        if account.id.is_empty() || !ids.insert(account.id.as_str()) {
            return Err("account IDs must be nonempty and unique".to_string());
        }
    }
    Ok(accounts)
}

fn validate_stored_account<'a>(
    name: &'a str,
    account: &StoredAccount,
    skeletons: &mut HashMap<String, &'a str>,
) -> std::result::Result<(), String> {
    if account.status.is_deleted() {
        if name != format!("deleted:{}", account.id) {
            return Err("deleted accounts must use their generated names".to_string());
        }
        if account.credential_kind != CredentialKind::None
            || account.password_hash.is_some()
        {
            return Err("deleted accounts must not retain credentials".to_string());
        }
        return Ok(());
    }
    crate::validation::validate_username(name)
        .map_err(|err| format!("invalid username {name:?}: {err}"))?;
    let skeleton = crate::validation::username_skeleton_hash(name);
    if let Some(existing) = skeletons.insert(skeleton, name) {
        return Err(format!(
            "username {:?} is too similar to username {:?}",
            existing.max(name),
            existing.min(name),
        ));
    }
    if account.status.is_active() && account.pending_token_revocation {
        return Err(format!("active account {name:?} must not have pending_token_revocation"));
    }
    validate_credential_pair(account.credential_kind, account.password_hash.as_deref())
        .map_err(|reason| format!("{reason} for {name:?}"))
}

pub(super) fn checked_next_revision(revision: u64) -> crate::Result<u64> {
    revision
        .checked_add(1)
        .ok_or_else(|| pnpr_error::RegistryError::Internal {
            reason: "account name revision overflowed".to_string(),
        })
}

pub(super) fn find_active_login(users: &Users, user_id: &str) -> crate::Result<(String, u64)> {
    users
        .iter()
        .find(|(_, account)| account.id == user_id && !account.status.is_deleted())
        .map(|(login, account)| (login.clone(), account.name_revision))
        .ok_or(pnpr_error::RegistryError::NotFound)
}

pub(super) fn move_account_login(
    users: &mut Users,
    user_id: &str,
    old: &str,
    name: String,
    next_revision: u64,
) -> crate::Result<()> {
    if old == name {
        let account = users.get_mut(old).ok_or(pnpr_error::RegistryError::NotFound)?;
        account.name_revision = next_revision;
        return Ok(());
    }
    if users.contains_key(&name) {
        return Err(pnpr_error::RegistryError::AdminConflict {
            reason: format!("username {name:?} is taken"),
        });
    }
    super::check_username_skeleton(users, &name, Some(user_id))?;
    let mut account = users.remove(old).ok_or(pnpr_error::RegistryError::NotFound)?;
    account.name_revision = next_revision;
    users.insert(name, account);
    Ok(())
}

pub(super) fn apply_stored_lifecycle(
    users: &mut Users,
    user_id: &str,
    revision: u64,
    lifecycle: AccountLifecycle,
    stores: &[std::sync::Arc<crate::TokenStore>],
) -> super::Result<Change<AccountStatusOutcome>> {
    let Some(account) = users
        .values_mut()
        .find(|account| account.id == user_id && !account.status.is_deleted())
    else {
        return Ok(Change::Keep(AccountStatusOutcome::Missing));
    };
    let cleared_pending = clear_pending_before_activation(account, user_id, lifecycle, stores)?;
    let (outcome, staged) =
        if should_apply_lifecycle(account.status, account.status_revision, revision, lifecycle) {
            let changed = account.stage_lifecycle(revision, lifecycle, !stores.is_empty());
            (AccountStatusOutcome::Applied, changed)
        } else {
            (AccountStatusOutcome::Superseded, false)
        };
    Ok(if cleared_pending || staged { Change::Write(outcome) } else { Change::Keep(outcome) })
}

impl StoredAccount {
    fn stage_lifecycle(
        &mut self,
        revision: u64,
        lifecycle: AccountLifecycle,
        has_bound_stores: bool,
    ) -> bool {
        let target_pending = match lifecycle {
            AccountLifecycle::Active => false,
            AccountLifecycle::Deactivated => self.pending_token_revocation || has_bound_stores,
        };
        if is_lifecycle_already_staged(self, revision, lifecycle, target_pending) {
            return false;
        }
        self.status = lifecycle.as_status();
        match lifecycle {
            AccountLifecycle::Active => {}
            AccountLifecycle::Deactivated => self.clear_hash(),
        }
        self.status_revision = revision;
        self.pending_token_revocation = target_pending;
        true
    }
}

fn clear_pending_before_activation(
    account: &mut StoredAccount,
    user_id: &str,
    lifecycle: AccountLifecycle,
    stores: &[std::sync::Arc<crate::TokenStore>],
) -> super::Result<bool> {
    match lifecycle {
        AccountLifecycle::Active if account.pending_token_revocation => {
            revoke_all_bound_tokens(stores, user_id)?;
            account.pending_token_revocation = false;
            Ok(true)
        }
        AccountLifecycle::Active | AccountLifecycle::Deactivated => Ok(false),
    }
}

fn is_lifecycle_already_staged(
    account: &StoredAccount,
    revision: u64,
    lifecycle: AccountLifecycle,
    target_pending: bool,
) -> bool {
    account.status == lifecycle.as_status()
        && account.status_revision == revision
        && account.pending_token_revocation == target_pending
        && match lifecycle {
            AccountLifecycle::Active => true,
            AccountLifecycle::Deactivated => {
                account.credential_kind == CredentialKind::None
                    && account.password_hash.is_none()
            }
        }
}

pub(super) fn complete_lifecycle_revocation(
    users: &mut Users,
    user_id: &str,
    outcome: AccountStatusOutcome,
    lifecycle: AccountLifecycle,
    stores: &[std::sync::Arc<crate::TokenStore>],
) -> super::Result<bool> {
    let has_pending = users
        .values()
        .any(|account| account.id == user_id && account.pending_token_revocation);
    let should_revoke = match (outcome, lifecycle) {
        (AccountStatusOutcome::Applied, AccountLifecycle::Deactivated) => {
            has_pending || !stores.is_empty()
        }
        (AccountStatusOutcome::Superseded, AccountLifecycle::Deactivated) => has_pending,
        (
            AccountStatusOutcome::Applied | AccountStatusOutcome::Superseded,
            AccountLifecycle::Active,
        )
        | (
            AccountStatusOutcome::Missing,
            AccountLifecycle::Active | AccountLifecycle::Deactivated,
        ) => false,
    };
    if !should_revoke {
        return Ok(false);
    }
    revoke_all_bound_tokens(stores, user_id)?;
    let Some(account) = users
        .values_mut()
        .find(|account| account.id == user_id && account.pending_token_revocation)
    else {
        return Ok(false);
    };
    account.pending_token_revocation = false;
    Ok(true)
}

fn revoke_all_bound_tokens(
    stores: &[std::sync::Arc<crate::TokenStore>],
    user_id: &str,
) -> super::Result<()> {
    if stores.is_empty() {
        return Err(pnpr_error::RegistryError::StorageUnavailable {
            reason: "cannot complete pending token revocation without a bound token store"
                .to_string(),
        });
    }
    for tokens in stores {
        tokens.revoke_all_for_user_blocking(user_id)?;
    }
    Ok(())
}
