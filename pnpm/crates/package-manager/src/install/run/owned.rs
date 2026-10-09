//! The install's owned inputs, each consumed by one phase.

use pnpm_package_manifest::DependencyGroup;
use std::sync::Arc;

impl InstallOwned {
    pub(super) fn shared_caches(&self) -> Option<&super::super::SharedInstallCaches> {
        self.projects.dedicated.as_ref().map(|dedicated| &dedicated.caches)
    }
}

/// The install's owned inputs, each consumed by one phase.
pub(super) struct InstallOwned {
    pub(super) tarball_mem_cache: Arc<super::super::MemCache>,
    pub(super) http_client_arc: Arc<super::super::ThrottledClient>,
    pub(super) projects: super::super::InstallProjects<Vec<DependencyGroup>>,
    pub(super) resolution: super::ResolutionInputs,
}
