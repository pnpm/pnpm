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
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc, Mutex, PoisonError, RwLock,
        atomic::{AtomicBool, Ordering},
    },
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
    /// How many writes have made the user inactive. A replica that sees it
    /// change ends its own browser sessions for the user, even when a later
    /// write has made the user active again before the replica reloaded.
    #[serde(default)]
    pub(super) deprovisions: u64,
    /// The attributes other than `userName` and `active`, as the client sent
    /// them.
    #[serde(default)]
    pub(super) attributes: Map<String, Value>,
}

impl Directory {
    /// Store `user` as `username`, carrying over its deprovision count and
    /// counting this write if it makes the user inactive.
    pub(super) fn store(&mut self, username: &str, mut user: ScimUser) -> ScimUser {
        let previous = self.users.get(username);
        user.deprovisions = previous.map_or(0, |previous| previous.deprovisions);
        if !user.active && previous.is_none_or(|previous| previous.active) {
            user.deprovisions += 1;
        }
        self.users.insert(username.to_string(), user.clone());
        user
    }
}

/// This replica's copy of the usernames the directory marks inactive.
#[derive(Debug, Default)]
pub(in super::super) struct ScimState {
    inactive: RwLock<Arc<BTreeSet<String>>>,
    /// The deprovision count of each user as this replica last read it.
    seen: Mutex<HashMap<String, u64>>,
    /// Set by a failed read of the directory, cleared by the next successful
    /// read or write.
    unreadable: AtomicBool,
}

impl ScimState {
    fn publish(&self, state: &AppState, directory: &Directory) {
        let inactive = directory.users
            .iter()
            .filter(|(_, user)| !user.active)
            .map(|(name, _)| name.clone())
            .collect();
        *self.inactive.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(inactive);
        self.unreadable.store(false, Ordering::Release);
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        for name in newly_deprovisioned(&mut seen, directory) {
            state.inner.identity.oidc.revoke_user_sessions(&name);
        }
    }
}

/// The users `directory` has deprovisioned since `seen` was last updated,
/// which it then records.
pub(super) fn newly_deprovisioned(
    seen: &mut HashMap<String, u64>,
    directory: &Directory,
) -> Vec<String> {
    directory.users
        .iter()
        .filter(|(name, user)| {
            seen.insert((*name).clone(), user.deprovisions).unwrap_or(0) != user.deprovisions
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Whether a SCIM client deprovisioned `username`. Always `false` without
/// `auth.scim`. Fails closed while the last read of the directory failed,
/// because this replica's copy may miss a deprovisioning.
pub(in super::super) fn is_deprovisioned(
    state: &AppState,
    username: &str,
) -> Result<bool, RegistryError> {
    let Some(scim) = &state.inner.identity.managed.scim else {
        return Ok(false);
    };
    if scim.unreadable.load(Ordering::Acquire) {
        return Err(RegistryError::Internal {
            reason: "the SCIM directory could not be read; sign-ins resume once it can".to_string(),
        });
    }
    Ok(scim.inactive
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(username))
}

/// Read the directory and publish its inactive usernames. After a failed read
/// [`is_deprovisioned`] fails closed until a read succeeds. Reports whether the read
/// succeeded.
pub(in super::super) async fn reload_directory(state: &AppState, scim: &ScimState) -> bool {
    match read_directory(state).await {
        Ok((directory, _)) => {
            scim.publish(state, &directory);
            true
        }
        Err(err) => {
            tracing::error!(error = %err, "could not read the SCIM directory; refusing user credentials");
            scim.unreadable.store(true, Ordering::Release);
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
            scim.publish(state, &directory);
            return Ok(output);
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the SCIM directory kept changing; retry the request".to_string(),
    })
}

/// Run [`deprovision`] again for a user the directory holds as inactive,
/// before a write can make it active. A cleanup that failed when the user was
/// made inactive must not leave its credentials working once it is active.
pub(super) async fn clean_up_before_reactivating(
    state: &AppState,
    username: &str,
) -> Result<(), RegistryError> {
    let (directory, _) = read_directory(state).await?;
    if directory.users.get(username).is_some_and(|user| !user.active) {
        deprovision(state, username).await?;
    }
    Ok(())
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
