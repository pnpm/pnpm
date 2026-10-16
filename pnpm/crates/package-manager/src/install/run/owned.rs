//! The install's owned inputs, each consumed by one phase.

use super::{
    super::{InstallError, InstallRunOptions, workspace_discovery::WorkspaceSnapshot},
    InstallView,
    workspace::{WorkspaceDirs, discovered_workspace_projects},
};
use pnpm_package_manifest::DependencyGroup;
use std::sync::Arc;

impl InstallOwned {
    pub(super) fn shared_caches(&self) -> Option<&super::super::SharedInstallCaches> {
        self.projects.dedicated.as_ref().map(|dedicated| &dedicated.caches)
    }

    pub(super) fn discover_workspace_projects(
        &mut self,
        install: InstallView<'_>,
        options: &InstallRunOptions<'_, '_>,
        dirs: &WorkspaceDirs<'_>,
        workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    ) -> Result<WorkspaceSnapshot, InstallError> {
        let cache = self
            .shared_caches()
            .and_then(|caches| caches.workspace.as_ref())
            .filter(|_| {
                workspace_manifest.is_some()
                    && options.selection.is_none()
                    && self.projects.workspace_projects_override.is_none()
            })
            .map(Arc::clone);
        let mut discover = || {
            discovered_workspace_projects(
                options.selection.is_some(),
                self.projects.workspace_projects_override.take(),
                dirs.workspace_dir.as_deref().unwrap_or(&dirs.workspace_root),
                workspace_manifest,
                &install.context.config.managed_directories(),
            )
        };
        match cache {
            Some(cache) => cache.load(discover),
            None => Ok(WorkspaceSnapshot {
                projects: discover()?.map(Arc::<[pnpm_workspace::Project]>::from),
                packages: None,
            }),
        }
    }
}

/// The install's owned inputs, each consumed by one phase.
pub(super) struct InstallOwned {
    pub(super) tarball_mem_cache: Arc<super::super::MemCache>,
    pub(super) http_client_arc: Arc<super::super::ThrottledClient>,
    pub(super) projects: super::super::InstallProjects<Vec<DependencyGroup>>,
    pub(super) resolution: super::ResolutionInputs,
}
