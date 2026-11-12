use super::{ScimUser, sync_scim_state};
use crate::server::{
    AppState, RegistryError, managed_state::WRITE_ATTEMPTS, scim::resources::apply_patch,
};
use pnpr_storage::metadata::{NewScimUser, ScimUserRemoval, ScimUserWrite, StoredScimUser};
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
    sync_scim_state(state, &stored.user_id, stored.active).await;
    let final_username = if stored.active {
        users.account_name(&id).await?.ok_or(RegistryError::NotFound)?
    } else {
        sync_after_scim_write(state, &id, stored.generation, false).await?
    };
    Ok((
        id,
        ScimUser { username: final_username, active: stored.active, attributes: stored.attributes },
    ))
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

async fn attempt_user_write(
    state: &AppState,
    user_id: &str,
    mut snapshot: ScimSnapshot,
    requested_rename: Option<&str>,
    candidate: ScimUser,
) -> Result<Option<ScimUser>, RegistryError> {
    if candidate.active && !prepare_inactive_for_reactivation(state, user_id, &mut snapshot).await?
    {
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
    let username = sync_after_scim_write(state, user_id, saved.generation, saved.active).await?;
    Ok(Some(ScimUser { username, active: saved.active, attributes: saved.attributes }))
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
    match state.inner.storage.remove_scim_user(user_id).await? {
        ScimUserRemoval::Removed(status) => {
            sync_scim_state(state, &status.user_id, false).await;
        }
        ScimUserRemoval::NotFound => return Err(RegistryError::NotFound),
    }
    state.inner.identity.auth.users.delete_user(user_id).await?;
    state.inner.identity.auth.revoke_all_tokens(user_id).await?;
    state.inner.identity.oidc.revoke_user_sessions(user_id);
    Ok(username)
}

/// Strip credentials left over from an earlier deactivation before a write can
/// make `user_id` active again. Returns `false` if another writer changed the
/// account or directory first so the caller retries from a fresh snapshot.
async fn prepare_inactive_for_reactivation(
    state: &AppState,
    user_id: &str,
    snapshot: &mut ScimSnapshot,
) -> Result<bool, RegistryError> {
    let auth = &state.inner.identity.auth;
    let deactivated_in_auth =
        auth.users.account_status(user_id).await? == Some(pnpr_auth::AccountStatus::Deactivated);
    if snapshot.active && !deactivated_in_auth {
        return Ok(true);
    }
    let pnpr_auth::AccountStatusUpdate::Updated(next_revision) = auth.users
        .set_account_status_if_current(
            user_id,
            snapshot.login.revision,
            pnpr_auth::AccountStatus::Deactivated,
        )
        .await?
    else {
        return Ok(false);
    };
    snapshot.login.revision = next_revision;
    if !revoke_deactivated_tokens(state, user_id, snapshot.generation, snapshot.login.revision)
        .await?
    {
        return Ok(false);
    }
    state.inner.identity.oidc.revoke_user_sessions(user_id);
    Ok(true)
}

/// Align `users.status` and credentials with the `scim_users` write committed
/// at `written_generation`, stopping without mutating auth state if a newer
/// SCIM generation superseded this write on another replica.
async fn sync_after_scim_write(
    state: &AppState,
    user_id: &str,
    written_generation: u64,
    active: bool,
) -> Result<String, RegistryError> {
    let auth = &state.inner.identity.auth;
    let target = if active {
        pnpr_auth::AccountStatus::Active
    } else {
        pnpr_auth::AccountStatus::Deactivated
    };
    for _ in 0..WRITE_ATTEMPTS {
        let login = auth.users.account_login(user_id).await?.ok_or(RegistryError::NotFound)?;
        let current_scim =
            state.inner.storage.read_scim_user(user_id).await?.ok_or(RegistryError::NotFound)?;
        if current_scim.generation != written_generation {
            return Ok(login.username);
        }
        match auth.users.set_account_status_if_current(user_id, login.revision, target).await? {
            pnpr_auth::AccountStatusUpdate::Updated(revision) => {
                if !active {
                    let _ = revoke_deactivated_tokens(state, user_id, written_generation, revision)
                        .await?;
                    state.inner.identity.oidc.revoke_user_sessions(user_id);
                }
                return Ok(login.username);
            }
            pnpr_auth::AccountStatusUpdate::CurrentChanged => continue,
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM user kept changing; retry the request".to_string(),
    })
}

/// Revoke tokens snapshotted while `user_id` remains deactivated at
/// `expected_generation` and `expected_revision`, so a slow deactivation never
/// revokes tokens minted after a newer reactivation.
async fn revoke_deactivated_tokens(
    state: &AppState,
    user_id: &str,
    expected_generation: u64,
    expected_revision: u64,
) -> Result<bool, RegistryError> {
    let auth = &state.inner.identity.auth;
    let tokens = auth.tokens.list_for_user(user_id).await?;
    let still_current = state.inner.storage
        .read_scim_user(user_id)
        .await?
        .is_some_and(|stored| stored.generation == expected_generation)
        && auth.users
            .account_login(user_id)
            .await?
            .is_some_and(|login| login.revision == expected_revision);
    if !still_current {
        return Ok(false);
    }
    for (key, _) in tokens {
        auth.tokens.revoke_by_key(&key).await?;
    }
    Ok(true)
}
