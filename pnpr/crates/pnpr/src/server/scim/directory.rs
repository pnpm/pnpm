//! The accounts a SCIM client provisioned, kept as rows in `scim_users` so
//! every replica refuses the same deprovisioned account IDs.

use super::{
    super::{
        AppState, RegistryError,
        managed_state::{WRITE_ATTEMPTS, read_with_deadline},
    },
    resources::apply_patch,
};
use pnpr_storage::metadata::{
    NewScimUser, ScimStatus, ScimUserRemoval, ScimUserWrite, StoredScimUser,
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Mutex, PoisonError, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Default, Clone)]
pub(super) struct ScimUser {
    pub(super) username: String,
    pub(super) active: bool,
    /// The attributes other than `userName` and `active`, as the client sent
    /// them.
    pub(super) attributes: Map<String, Value>,
}

#[derive(Debug, Default)]
pub(super) struct ScimTracker {
    pub(super) since_generation: u64,
    pub(super) deprovisions: HashMap<String, u64>,
}

/// This replica's copy of the directory's account IDs, each with whether it is
/// active.
#[derive(Debug, Default)]
pub(in super::super) struct ScimState {
    active_by_id: RwLock<BTreeMap<String, bool>>,
    pub(super) subjects: RwLock<super::subjects::SubjectDirectory>,
    tracker: Mutex<ScimTracker>,
    /// Set by a failed read of the directory, cleared by the next successful
    /// read.
    unreadable: AtomicBool,
}

impl ScimState {
    fn since_generation(&self) -> u64 {
        self.tracker.lock().unwrap_or_else(PoisonError::into_inner).since_generation
    }

    fn apply_statuses(&self, state: &AppState, statuses: &[ScimStatus]) {
        let mut subjects = self.subjects.write().unwrap_or_else(PoisonError::into_inner);
        for status in statuses {
            if subjects.apply(status) {
                state.inner.identity.oidc.revoke_user_sessions(&status.user_id);
            }
        }
        if !statuses.is_empty() {
            let mut active_by_id =
                self.active_by_id.write().unwrap_or_else(PoisonError::into_inner);
            for status in statuses {
                active_by_id.insert(status.user_id.clone(), status.active);
            }
        }
        self.unreadable.store(false, Ordering::Release);
        let revoked = {
            let mut tracker = self.tracker.lock().unwrap_or_else(PoisonError::into_inner);
            record_status_deltas(&mut tracker, statuses)
        };
        for user_id in revoked {
            state.inner.identity.oidc.revoke_user_sessions(&user_id);
        }
    }
}

/// Record incremental `statuses` in `tracker` and return the user IDs whose
/// deprovision count increased since `tracker` last observed them.
pub(super) fn record_status_deltas(
    tracker: &mut ScimTracker,
    statuses: &[ScimStatus],
) -> Vec<String> {
    let mut revoked = Vec::new();
    for status in statuses {
        tracker.since_generation = tracker.since_generation.max(status.generation);
        let previous =
            tracker.deprovisions.insert(status.user_id.clone(), status.deprovisions).unwrap_or(0);
        if previous != status.deprovisions {
            revoked.push(status.user_id.clone());
        }
    }
    revoked
}

/// Whether a SCIM client deprovisioned the account ID. Always `false` without
/// `auth.scim`. Fails closed while the last read of the directory failed,
/// because this replica's copy may miss a deprovisioning.
pub(in super::super) fn is_deprovisioned(
    state: &AppState,
    user_id: &str,
) -> Result<bool, RegistryError> {
    Ok(directory_entry(state, user_id)? == Some(false))
}

fn directory_entry(state: &AppState, user_id: &str) -> Result<Option<bool>, RegistryError> {
    let Some(scim) = &state.inner.identity.managed.scim else {
        return Ok(None);
    };
    if scim.unreadable.load(Ordering::Acquire) {
        return Err(RegistryError::Internal {
            reason: "the SCIM directory could not be read; sign-ins resume once it can".to_string(),
        });
    }
    Ok(scim.active_by_id
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(user_id)
        .copied())
}

/// Read SCIM status changes since the last generation and publish them. After
/// a failed read [`is_deprovisioned`] fails closed until a read succeeds.
pub(in super::super) async fn reload_directory(state: &AppState, scim: &ScimState) -> bool {
    let since = scim.since_generation();
    match read_with_deadline(state.inner.storage.read_scim_statuses_since(since)).await {
        Ok(statuses) => {
            scim.apply_statuses(state, &statuses);
            true
        }
        Err(err) => {
            tracing::error!(error = %err, "could not read the SCIM directory; refusing user credentials");
            scim.unreadable.store(true, Ordering::Release);
            false
        }
    }
}

async fn sync_scim_state(state: &AppState, user_id: &str, active: bool) {
    let managed = &state.inner.identity.managed;
    let Some(scim) = &managed.scim else {
        return;
    };
    scim.active_by_id
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(user_id.to_string(), active);
    if !active {
        state.inner.identity.oidc.revoke_user_sessions(user_id);
    }
    if !reload_directory(state, scim).await {
        managed.schedule_retry();
    }
}

pub(super) async fn insert_user(
    state: &AppState,
    user_id: &str,
    user: &ScimUser,
) -> Result<StoredScimUser, RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    let new_user = NewScimUser {
        user_id: user_id.to_string(),
        active: user.active,
        attributes: user.attributes.clone(),
    };
    let stored = state.inner.storage
        .insert_scim_user(new_user)
        .await?
        .ok_or_else(|| RegistryError::AdminConflict {
            reason: "the SCIM user already exists".to_string(),
        })?;
    sync_scim_state(state, &stored.user_id, stored.active).await;
    Ok(stored)
}

pub(super) async fn replace_user(
    state: &AppState,
    user_id: &str,
    user: &ScimUser,
) -> Result<(), RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let (stored, login) = load_scim_account(state, user_id).await?;
        let Some(claimed) =
            apply_conditional_rename(state, user_id, &login, Some(&user.username)).await?
        else {
            continue;
        };
        let new_user = NewScimUser {
            user_id: user_id.to_string(),
            active: user.active,
            attributes: user.attributes.clone(),
        };
        if commit_user_write(state, user_id, stored.generation, new_user, claimed.as_ref())
            .await?
            .is_some()
        {
            return Ok(());
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM user kept changing; retry the request".to_string(),
    })
}

pub(super) async fn patch_user(
    state: &AppState,
    user_id: &str,
    patch: &Value,
) -> Result<ScimUser, RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let (stored, login) = load_scim_account(state, user_id).await?;
        let mut candidate = ScimUser {
            username: String::new(),
            active: stored.active,
            attributes: stored.attributes,
        };
        apply_patch(&mut candidate, patch)?;
        let requested_rename =
            (!candidate.username.is_empty()).then_some(candidate.username.as_str());
        let Some(claimed) =
            apply_conditional_rename(state, user_id, &login, requested_rename).await?
        else {
            continue;
        };
        let write = NewScimUser {
            user_id: user_id.to_string(),
            active: candidate.active,
            attributes: candidate.attributes.clone(),
        };
        if commit_user_write(state, user_id, stored.generation, write, claimed.as_ref())
            .await?
            .is_some()
        {
            if candidate.username.is_empty() {
                candidate.username = state.inner.identity.auth.users
                    .account_name(user_id)
                    .await?
                    .unwrap_or(login.username);
            }
            return Ok(candidate);
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM user kept changing; retry the request".to_string(),
    })
}

struct ClaimedRename {
    previous: String,
    applied: String,
    revision: u64,
}

async fn load_scim_account(
    state: &AppState,
    user_id: &str,
) -> Result<(StoredScimUser, pnpr_auth::AccountLogin), RegistryError> {
    let stored =
        state.inner.storage.read_scim_user(user_id).await?.ok_or(RegistryError::NotFound)?;
    let login = state.inner.identity.auth.users
        .account_login(user_id)
        .await?
        .ok_or(RegistryError::NotFound)?;
    Ok((stored, login))
}

async fn apply_conditional_rename(
    state: &AppState,
    user_id: &str,
    login: &pnpr_auth::AccountLogin,
    requested: Option<&str>,
) -> Result<Option<Option<ClaimedRename>>, RegistryError> {
    let Some(target) = requested else {
        return Ok(Some(None));
    };
    match state.inner.identity.auth.users.rename_user_if_current(user_id, login.revision, target)
        .await?
    {
        pnpr_auth::UserRename::Renamed(revision) => Ok(Some(Some(ClaimedRename {
            previous: login.username.clone(),
            applied: target.to_string(),
            revision,
        }))),
        pnpr_auth::UserRename::CurrentChanged => Ok(None),
    }
}

async fn commit_user_write(
    state: &AppState,
    user_id: &str,
    expected_generation: u64,
    write: NewScimUser,
    claimed: Option<&ClaimedRename>,
) -> Result<Option<StoredScimUser>, RegistryError> {
    match state.inner.storage.replace_scim_user(Some(expected_generation), write).await {
        Ok(ScimUserWrite::Written(saved)) => {
            sync_scim_state(state, &saved.user_id, saved.active).await;
            Ok(Some(saved))
        }
        Ok(ScimUserWrite::Conflict) => {
            rollback_rename(state, user_id, claimed).await;
            Ok(None)
        }
        Ok(ScimUserWrite::NotFound) => {
            rollback_rename(state, user_id, claimed).await;
            Err(RegistryError::NotFound)
        }
        Err(err) => {
            rollback_rename(state, user_id, claimed).await;
            Err(err)
        }
    }
}

async fn rollback_rename(state: &AppState, user_id: &str, claimed: Option<&ClaimedRename>) {
    if let Some(claimed) = claimed
        && claimed.previous != claimed.applied
        && let Err(err) = state.inner.identity.auth.users.rename_user_if_current(
            user_id,
            claimed.revision,
            &claimed.previous,
        )
        .await
    {
        tracing::warn!(error = %err, user_id, "could not roll back SCIM username after aborted write");
    }
}

pub(super) async fn remove_user(state: &AppState, user_id: &str) -> Result<(), RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    match state.inner.storage.remove_scim_user(user_id).await? {
        ScimUserRemoval::Removed(status) => {
            sync_scim_state(state, &status.user_id, false).await;
            Ok(())
        }
        ScimUserRemoval::NotFound => Err(RegistryError::NotFound),
    }
}

/// Run [`deprovision`] again for a user the directory holds as inactive,
/// before a write can make it active. A cleanup that failed when the user was
/// made inactive must not leave its credentials working once it is active.
pub(super) async fn clean_up_before_reactivating(
    state: &AppState,
    user_id: &str,
) -> Result<(), RegistryError> {
    if state.inner.storage.read_scim_user(user_id).await?.is_some_and(|user| !user.active) {
        deprovision(state, user_id).await?;
    }
    Ok(())
}

/// Take away everything the account ID could sign in or authenticate with: its
/// stored tokens, its password, and its browser sessions on this
/// replica. Other replicas refuse its sessions once they reload the
/// directory. Safe to repeat.
pub(super) async fn deprovision(state: &AppState, user_id: &str) -> Result<(), RegistryError> {
    let auth = &state.inner.identity.auth;
    auth.revoke_all_tokens(user_id).await?;
    auth.users.clear_password(user_id).await?;
    auth.revoke_all_tokens(user_id).await?;
    state.inner.identity.oidc.revoke_user_sessions(user_id);
    Ok(())
}

#[cfg(test)]
mod tests;
