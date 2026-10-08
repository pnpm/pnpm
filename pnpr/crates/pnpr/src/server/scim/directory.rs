//! The accounts a SCIM client provisioned, kept as one record in the hosted
//! store so every replica refuses the same deprovisioned usernames.

use super::super::{
    AppState, RegistryError,
    managed_state::{WRITE_ATTEMPTS, store_registry_record},
    user_admin::revoke_all_tokens,
};
use pnpr_storage::{RegistryRecord, SCIM_DIRECTORY};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, PoisonError, RwLock},
};

/// The stored record: every username a SCIM client has provisioned, keyed
/// by username, which is also its SCIM `id`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Directory {
    #[serde(default)]
    pub(super) users: BTreeMap<String, ScimUser>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(super) struct ScimUser {
    pub(super) active: bool,
    /// Set by `DELETE`. The user stays in the record, inactive, so the
    /// username keeps being refused, but the SCIM API no longer lists it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) removed: bool,
    /// The attributes other than `userName` and `active`, as the client sent
    /// them.
    #[serde(default)]
    pub(super) attributes: Map<String, Value>,
}

/// This replica's copy of the usernames the directory marks inactive.
#[derive(Debug, Default)]
pub(in super::super) struct ScimState {
    inactive: RwLock<Arc<BTreeSet<String>>>,
}

impl ScimState {
    fn publish(&self, directory: &Directory) {
        let inactive = directory.users
            .iter()
            .filter(|(_, user)| !user.active)
            .map(|(name, _)| name.clone())
            .collect();
        *self.inactive.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(inactive);
    }
}

/// Whether a SCIM client deprovisioned `username`. Always `false` without
/// `auth.scim`.
pub(in super::super) fn is_deprovisioned(state: &AppState, username: &str) -> bool {
    state.inner.identity.managed.scim
        .as_ref()
        .is_some_and(|scim| {
            scim.inactive
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(username)
        })
}

/// Read the directory and publish its inactive usernames. A failed read keeps
/// the usernames this replica already refuses. Reports whether the read
/// succeeded.
pub(in super::super) async fn reload_directory(state: &AppState, scim: &ScimState) -> bool {
    match read_directory(state).await {
        Ok((directory, _)) => {
            scim.publish(&directory);
            true
        }
        Err(err) => {
            tracing::error!(error = %err, "could not read the SCIM directory");
            false
        }
    }
}

pub(super) async fn read_directory(
    state: &AppState,
) -> Result<(Directory, Option<Vec<u8>>), RegistryError> {
    let kind = RegistryRecord::ScimUsers;
    let stored = state.inner.storage.read_registry_record(kind, SCIM_DIRECTORY).await?;
    let directory = match &stored {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|err| RegistryError::Internal {
                reason: format!("the stored SCIM directory is unreadable: {err}"),
            })?,
        None => Directory::default(),
    };
    Ok((directory, stored))
}

/// Apply `edit` to the stored directory and store the result, rereading and
/// reapplying it when another writer replaced it first. The result applies
/// on this replica at once.
pub(super) async fn update_directory<Edit, Output>(
    state: &AppState,
    edit: Edit,
) -> Result<Output, RegistryError>
where
    Edit: Fn(&mut Directory) -> Result<Output, RegistryError>,
{
    let managed = &state.inner.identity.managed;
    let scim = managed.scim.as_ref().ok_or(RegistryError::NotFound)?;
    let _reload = managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let (mut directory, stored) = read_directory(state).await?;
        let output = edit(&mut directory)?;
        let bytes = serde_json::to_vec(&directory)?;
        let kind = RegistryRecord::ScimUsers;
        if store_registry_record(state, kind, SCIM_DIRECTORY, stored.as_deref(), &bytes).await? {
            scim.publish(&directory);
            return Ok(output);
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM directory kept changing; retry the request".to_string(),
    })
}

/// Take away everything `username` could sign in or authenticate with: its
/// stored tokens, its password account, and its browser sessions on this
/// replica. Other replicas refuse its sessions once they reload the
/// directory. Safe to repeat.
pub(super) async fn deprovision(state: &AppState, username: &str) -> Result<(), RegistryError> {
    revoke_all_tokens(state, username).await?;
    state.inner.identity.auth.users.delete_user(username).await?;
    revoke_all_tokens(state, username).await?;
    state.inner.identity.oidc.revoke_user_sessions(username);
    Ok(())
}
