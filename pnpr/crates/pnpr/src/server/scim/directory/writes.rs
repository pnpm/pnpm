use super::{
    ScimUser, fail_closed_on_unsynced_deactivation, lifecycle_for_active, sync_scim_state,
};
use crate::server::{
    AppState, RegistryError, managed_state::WRITE_ATTEMPTS, scim::resources::apply_patch,
};
use pnpr_auth::{AccountLifecycle, AccountStatusOutcome};
use pnpr_storage::database::{NewScimUser, ScimUserRemoval, ScimUserWrite, StoredScimUser};
use serde_json::Value;

pub(in super::super) async fn create_user(
    state: &AppState,
    username: &str,
    user: &ScimUser,
) -> Result<(String, ScimUser), RegistryError> {
    let _reload = state.inner.identity.managed.reload.lock().await;
    let users = &state.inner.identity.auth.users;
    let id = users.provision_user(username).await?;
    let new_user = NewScimUser {
        user_id: id.clone(),
        active: user.active,
        attributes: user.attributes.clone(),
    };
    let stored = match state.inner.storage.insert_scim_user(new_user).await {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            users.delete_user(&id).await?;
            return Err(RegistryError::AdminConflict {
                reason: "the SCIM user already exists".to_string(),
            });
        }
        Err(err) => {
            users.delete_user(&id).await?;
            return Err(err);
        }
    };
    let created = finish_scim_write(state, stored).await?;
    Ok((id, created))
}

pub(in super::super) async fn replace_user(
    state: &AppState,
    user_id: &str,
    user: &ScimUser,
) -> Result<ScimUser, RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let (stored, login) = load_scim_account(state, user_id).await?;
        let snapshot = ScimSnapshot { generation: stored.generation, active: stored.active, login };
        let candidate = ScimUser {
            username: String::new(),
            active: user.active,
            attributes: user.attributes.clone(),
        };
        if let Some(saved) =
            attempt_user_write(state, user_id, snapshot, Some(&user.username), candidate).await?
        {
            return Ok(saved);
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM user kept changing; retry the request".to_string(),
    })
}

pub(in super::super) async fn patch_user(
    state: &AppState,
    user_id: &str,
    patch: &Value,
) -> Result<ScimUser, RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let (stored, login) = load_scim_account(state, user_id).await?;
        let snapshot = ScimSnapshot { generation: stored.generation, active: stored.active, login };
        let mut candidate = ScimUser {
            username: String::new(),
            active: stored.active,
            attributes: stored.attributes,
        };
        apply_patch(&mut candidate, patch)?;
        let requested = std::mem::take(&mut candidate.username);
        let rename = (!requested.is_empty()).then_some(requested.as_str());
        if let Some(saved) = attempt_user_write(state, user_id, snapshot, rename, candidate).await?
        {
            return Ok(saved);
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM user kept changing; retry the request".to_string(),
    })
}

struct ScimSnapshot {
    generation: u64,
    active: bool,
    login: pnpr_auth::AccountLogin,
}

struct ClaimedRename {
    previous: String,
    applied: String,
    revision: u64,
}

struct PendingReconcile<'a> {
    state: &'a AppState,
    user_id: &'a str,
    generation: u64,
    lifecycle: AccountLifecycle,
    disarmed: bool,
}

impl<'a> PendingReconcile<'a> {
    const fn new(
        state: &'a AppState,
        user_id: &'a str,
        generation: u64,
        lifecycle: AccountLifecycle,
    ) -> Self {
        Self { state, user_id, generation, lifecycle, disarmed: false }
    }

    const fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for PendingReconcile<'_> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        match self.lifecycle {
            AccountLifecycle::Active => {}
            AccountLifecycle::Deactivated => {
                fail_closed_on_unsynced_deactivation(self.state, self.user_id, self.generation);
            }
        }
        self.state.inner.identity.managed.schedule_retry();
    }
}

async fn attempt_user_write(
    state: &AppState,
    user_id: &str,
    snapshot: ScimSnapshot,
    requested_rename: Option<&str>,
    candidate: ScimUser,
) -> Result<Option<ScimUser>, RegistryError> {
    if candidate.active && !prepare_inactive_for_reactivation(state, user_id, &snapshot).await? {
        return Ok(None);
    }
    let Some(claimed) =
        apply_conditional_rename(state, user_id, &snapshot.login, requested_rename).await?
    else {
        return Ok(None);
    };
    let write = NewScimUser {
        user_id: user_id.to_string(),
        active: candidate.active,
        attributes: candidate.attributes,
    };
    let Some(saved) =
        commit_user_write(state, user_id, snapshot.generation, write, claimed.as_ref()).await?
    else {
        return Ok(None);
    };
    finish_scim_write(state, saved).await.map(Some)
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
        Ok(ScimUserWrite::Written(saved)) => Ok(Some(saved)),
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

pub(in super::super) async fn delete_user(
    state: &AppState,
    user_id: &str,
) -> Result<String, RegistryError> {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    let username = state.inner.identity.auth.users
        .account_name(user_id)
        .await?
        .ok_or(RegistryError::NotFound)?;
    let status = match state.inner.storage.remove_scim_user(user_id).await? {
        ScimUserRemoval::Removed(status) => status,
        ScimUserRemoval::NotFound => return Err(RegistryError::NotFound),
    };
    let mut pending = PendingReconcile::new(
        state,
        &status.user_id,
        status.generation,
        AccountLifecycle::Deactivated,
    );
    state.inner.identity.auth.users.delete_user(user_id).await?;
    state.inner.identity.auth.revoke_all_tokens(user_id).await?;
    sync_scim_state(state, &status.user_id, status.generation, Some(AccountLifecycle::Deactivated))
        .await;
    state.inner.identity.oidc.revoke_user_sessions(user_id);
    pending.disarm();
    Ok(username)
}

/// Strip credentials left over from an earlier deactivation before a write can
/// make `user_id` active again. Returns `false` if a newer status revision
/// superseded `snapshot` so the caller retries from a fresh snapshot.
async fn prepare_inactive_for_reactivation(
    state: &AppState,
    user_id: &str,
    snapshot: &ScimSnapshot,
) -> Result<bool, RegistryError> {
    let auth = &state.inner.identity.auth;
    if snapshot.active
        && auth.users.account_status(user_id).await? != Some(pnpr_auth::AccountStatus::Deactivated)
    {
        return Ok(true);
    }
    let revision =
        if snapshot.active { snapshot.generation.saturating_sub(1) } else { snapshot.generation };
    match auth.apply_account_status(user_id, revision, AccountLifecycle::Deactivated).await? {
        AccountStatusOutcome::Applied => {
            state.inner.identity.oidc.revoke_user_sessions(user_id);
            Ok(true)
        }
        AccountStatusOutcome::Superseded => Ok(false),
        AccountStatusOutcome::Missing => Err(RegistryError::NotFound),
    }
}

/// Align `users.status`, credentials, and sessions with the `scim_users` write
/// committed in `saved`, skipping session/token cleanup when a newer SCIM
/// generation has already superseded `saved.generation`.
pub(super) async fn finish_scim_write(
    state: &AppState,
    saved: StoredScimUser,
) -> Result<ScimUser, RegistryError> {
    let lifecycle = lifecycle_for_active(saved.active);
    let mut pending = PendingReconcile::new(state, &saved.user_id, saved.generation, lifecycle);
    let outcome =
        state.inner.identity.auth.apply_account_status(&saved.user_id, saved.generation, lifecycle)
            .await?;
    match outcome {
        AccountStatusOutcome::Applied => {
            sync_scim_state(state, &saved.user_id, saved.generation, Some(lifecycle)).await;
        }
        AccountStatusOutcome::Superseded => {
            sync_scim_state(state, &saved.user_id, saved.generation, None).await;
        }
        AccountStatusOutcome::Missing => {
            sync_scim_state(state, &saved.user_id, saved.generation, None).await;
            pending.disarm();
            return Err(RegistryError::NotFound);
        }
    }
    pending.disarm();
    let username = state.inner.identity.auth.users
        .account_name(&saved.user_id)
        .await?
        .ok_or(RegistryError::NotFound)?;
    Ok(ScimUser { username, active: saved.active, attributes: saved.attributes })
}
