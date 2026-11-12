use super::super::AppState;
use pnpr_config::Config;
use pnpr_error::{RegistryError, Result};
use pnpr_storage::metadata::ScimStatus;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{PoisonError, RwLockReadGuard},
};

#[derive(Debug, Default)]
pub(super) struct SubjectDirectory {
    by_user: HashMap<String, String>,
    by_subject: HashMap<String, BTreeSet<String>>,
}

impl SubjectDirectory {
    /// Update a binding, returning whether an existing session must end.
    pub(super) fn apply(&mut self, status: &ScimStatus) -> bool {
        let previous = self.by_user.remove(&status.user_id);
        if let Some(subject) = &previous {
            let users = self.by_subject.get_mut(subject).expect("indexed subject");
            users.remove(&status.user_id);
            if users.is_empty() {
                self.by_subject.remove(subject);
            }
        }
        if let Some(subject) = &status.external_id {
            self.by_user.insert(status.user_id.clone(), subject.clone());
            self.by_subject
                .entry(subject.clone())
                .or_default()
                .insert(status.user_id.clone());
        }
        previous.is_some() && previous != status.external_id
    }

    pub(super) fn resolve(&self, subject: &str) -> Result<String> {
        let users = self.by_subject.get(subject).ok_or_else(rejected)?;
        if users.len() != 1 {
            return Err(rejected());
        }
        users
            .first()
            .cloned()
            .ok_or_else(rejected)
    }
}

/// Holds the directory read lock through session creation. Binding updates take
/// its write lock before revoking sessions.
pub(in super::super) struct SubjectBinding<'state> {
    _directory: RwLockReadGuard<'state, SubjectDirectory>,
    user_id: String,
}

impl AsRef<str> for SubjectBinding<'_> {
    fn as_ref(&self) -> &str {
        &self.user_id
    }
}

pub(in super::super) fn resolve_subject<'state>(
    state: &'state AppState,
    subject: &str,
) -> Result<SubjectBinding<'state>> {
    let scim = state.inner.identity.managed.scim.as_ref().ok_or_else(rejected)?;
    let directory = scim.subjects.read().unwrap_or_else(PoisonError::into_inner);
    let user_id = directory.resolve(subject)?;
    if super::is_deprovisioned(state, &user_id)? {
        return Err(rejected());
    }
    Ok(SubjectBinding { _directory: directory, user_id })
}

pub(in super::super) fn validate_oidc(config: &Config) -> Result<()> {
    let count = config.identity.auth.oidc
        .iter()
        .filter(|provider| provider.login.as_ref().is_some_and(|login| login.scim))
        .count();
    if count > 1 || (count == 1 && config.identity.auth.scim.is_none()) {
        return Err(RegistryError::InvalidConfig {
            reason: "OIDC login.scim requires auth.scim and may be enabled for only one provider"
                .to_string(),
        });
    }
    Ok(())
}

fn rejected() -> RegistryError {
    RegistryError::Unauthenticated { resource: "SCIM subject binding".to_string() }
}
