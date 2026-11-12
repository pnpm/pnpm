//! The accounts a SCIM client provisioned, kept as rows in `scim_users` so
//! every replica refuses the same deprovisioned account IDs.

pub(super) use writes::{create_user, delete_user, patch_user, replace_user};

#[cfg(test)]
mod tests;
mod writes;

use super::super::{AppState, RegistryError, managed_state::read_with_deadline};
use pnpr_storage::metadata::ScimStatus;
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

/// Read SCIM status changes since the last generation, reconcile account
/// statuses in auth, and publish them. After a failed read or reconciliation
/// [`is_deprovisioned`] fails closed until a reload succeeds.
pub(in super::super) async fn reload_directory(state: &AppState, scim: &ScimState) -> bool {
    let since = scim.since_generation();
    let statuses = match read_with_deadline(state.inner.storage.read_scim_statuses_since(since))
        .await
    {
        Ok(statuses) => statuses,
        Err(err) => {
            tracing::error!(error = %err, "could not read the SCIM directory; refusing user credentials");
            scim.unreadable.store(true, Ordering::Release);
            return false;
        }
    };
    if let Err(err) = reconcile_auth_statuses(state, &statuses).await {
        tracing::error!(error = %err, "could not reconcile SCIM account statuses; refusing user credentials");
        scim.unreadable.store(true, Ordering::Release);
        return false;
    }
    scim.apply_statuses(state, &statuses);
    true
}

async fn reconcile_auth_statuses(
    state: &AppState,
    statuses: &[ScimStatus],
) -> Result<(), RegistryError> {
    for status in statuses {
        let lifecycle = lifecycle_for_active(status.active);
        let outcome = state.inner.identity.auth.apply_account_status(
            &status.user_id,
            status.generation,
            lifecycle,
        )
        .await?;
        if outcome == pnpr_auth::AccountStatusOutcome::Applied
            && matches!(lifecycle, pnpr_auth::AccountLifecycle::Deactivated)
        {
            state.inner.identity.oidc.revoke_user_sessions(&status.user_id);
        }
    }
    Ok(())
}

pub(super) const fn lifecycle_for_active(active: bool) -> pnpr_auth::AccountLifecycle {
    if active {
        pnpr_auth::AccountLifecycle::Active
    } else {
        pnpr_auth::AccountLifecycle::Deactivated
    }
}

pub(super) fn fail_closed_on_unsynced_deactivation(
    state: &AppState,
    user_id: &str,
    generation: u64,
) {
    state.inner.identity.oidc.revoke_user_sessions(user_id);
    if let Some(scim) = &state.inner.identity.managed.scim
        && scim.since_generation() <= generation
    {
        scim.unreadable.store(true, Ordering::Release);
    }
}

async fn sync_scim_state(
    state: &AppState,
    user_id: &str,
    generation: u64,
    applied: Option<pnpr_auth::AccountLifecycle>,
) {
    let managed = &state.inner.identity.managed;
    let Some(scim) = &managed.scim else {
        return;
    };
    if let Some(lifecycle) = applied {
        if scim.since_generation() < generation {
            let active = matches!(lifecycle, pnpr_auth::AccountLifecycle::Active);
            scim.active_by_id
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(user_id.to_string(), active);
        }
        if matches!(lifecycle, pnpr_auth::AccountLifecycle::Deactivated) {
            state.inner.identity.oidc.revoke_user_sessions(user_id);
        }
    }
    if !reload_directory(state, scim).await {
        managed.schedule_retry();
    }
}
