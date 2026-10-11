//! Who may read and who may publish what an organization owns. Signed
//! artifacts, compiler caches, and pipeline run records all answer to the
//! organizations `artifacts.orgs` declares.

use indexmap::IndexMap;
use pnpr_config::StorageAccess;
use pnpr_error::RegistryError;
use pnpr_policy::Identity;
use pnpr_shared_artifacts::OrgAccess;

use super::{AppState, require_caller};

/// An authenticated caller, and what it may do with each declared
/// organization.
pub(super) struct CallerOrgs<'a> {
    username: String,
    identity: &'a Identity,
    orgs: &'a IndexMap<String, StorageAccess>,
}

impl<'a> CallerOrgs<'a> {
    /// `Err` for an anonymous caller. `resource` names what it asked for.
    pub(super) fn new(
        state: &'a AppState,
        identity: &'a Identity,
        resource: &str,
    ) -> Result<Self, RegistryError> {
        Ok(CallerOrgs {
            username: require_caller(identity, resource)?,
            identity,
            orgs: &state.inner.config.features.artifacts.orgs,
        })
    }

    /// `Ok` when the caller may read what `org` owns and, with `publish`,
    /// publish to it. An organization the caller cannot read answers as
    /// not found, so its existence is not disclosed.
    pub(super) fn authorize(
        &self,
        org: &str,
        publish: bool,
        action: &'static str,
    ) -> Result<(), RegistryError> {
        if !self.may_read(org) {
            return Err(RegistryError::NotFound);
        }
        if publish && !self.may_publish(org) {
            return Err(RegistryError::Forbidden {
                user: self.username.clone(),
                action,
                resource: org.to_string(),
            });
        }
        Ok(())
    }

    /// Every organization the caller may read.
    pub(super) fn readable(&self) -> impl Iterator<Item = &str> {
        self.orgs
            .iter()
            .filter(|(_, policy)| policy.access.allows(self.identity))
            .map(|(org, _)| org.as_str())
    }
}

impl OrgAccess for CallerOrgs<'_> {
    fn username(&self) -> &str {
        &self.username
    }

    fn may_read(&self, org: &str) -> bool {
        self.orgs
            .get(org)
            .is_some_and(|policy| policy.access.allows(self.identity))
    }

    /// Publishing implies reading: a publisher that cannot see what it
    /// publishes could not tell its own artifact from another's.
    fn may_publish(&self, org: &str) -> bool {
        self.may_read(org)
            && self.orgs
                .get(org)
                .is_some_and(|policy| policy.publish.allows(self.identity))
    }
}
