//! Who may read and who may publish what an artifact owner holds. Signed
//! artifacts, compiler caches, and pipeline run records all answer to the
//! owners `artifacts.owners` declares.

use indexmap::IndexMap;
use pnpr_config::StorageAccess;
use pnpr_error::RegistryError;
use pnpr_policy::Identity;
use pnpr_shared_artifacts::{OwnerAccess, OwnerGrant};

use super::{AppState, require_caller};

/// An authenticated caller, and what it may do with each declared
/// artifact owner.
pub(super) struct CallerOwners<'a> {
    username: String,
    identity: &'a Identity,
    owners: &'a IndexMap<String, StorageAccess>,
}

impl<'a> CallerOwners<'a> {
    /// `Err` for an anonymous caller. `resource` names what it asked for.
    pub(super) fn new(
        state: &'a AppState,
        identity: &'a Identity,
        resource: &str,
    ) -> Result<Self, RegistryError> {
        Ok(CallerOwners {
            username: require_caller(identity, resource)?,
            identity,
            owners: &state.inner.config.features.artifacts.owners,
        })
    }

    /// `Ok` when the caller holds `grant` on `owner`. An owner the
    /// caller cannot read answers as not found, so its existence is not
    /// disclosed.
    pub(super) fn authorize(
        &self,
        owner: &str,
        grant: OwnerGrant,
        action: &'static str,
    ) -> Result<(), RegistryError> {
        if !self.may_read(owner) {
            return Err(RegistryError::NotFound);
        }
        match grant {
            OwnerGrant::Publish if !self.may_publish(owner) => Err(RegistryError::Forbidden {
                user: self.username.clone(),
                action,
                resource: owner.to_string(),
            }),
            OwnerGrant::Publish | OwnerGrant::Read => Ok(()),
        }
    }

    /// Every artifact owner the caller may read.
    pub(super) fn readable(&self) -> impl Iterator<Item = &str> {
        self.owners
            .iter()
            .filter(|(_, policy)| policy.access.allows(self.identity))
            .map(|(owner, _)| owner.as_str())
    }
}

impl OwnerAccess for CallerOwners<'_> {
    fn username(&self) -> &str {
        &self.username
    }

    fn may_read(&self, owner: &str) -> bool {
        self.owners
            .get(owner)
            .is_some_and(|policy| policy.access.allows(self.identity))
    }

    /// Publishing implies reading: a publisher that cannot see what it
    /// publishes could not tell its own artifact from another's.
    fn may_publish(&self, owner: &str) -> bool {
        self.may_read(owner)
            && self.owners
                .get(owner)
                .is_some_and(|policy| policy.publish.allows(self.identity))
    }
}
