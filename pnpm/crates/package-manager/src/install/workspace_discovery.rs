use std::sync::{Arc, Mutex};

use pnpm_resolving_resolver_base::WorkspacePackages;
use pnpm_workspace::Project;

use super::{InstallError, build_workspace_packages_map};

/// Workspace inputs shared by dedicated installs in one command. Callers must
/// keep discovery settings and source manifests unchanged for its lifetime.
#[derive(Default)]
pub struct WorkspaceDiscoveryCache {
    snapshot: Mutex<Option<WorkspaceSnapshot>>,
}

#[derive(Clone)]
pub(super) struct WorkspaceSnapshot {
    pub(super) projects: Option<Arc<[Project]>>,
    pub(super) packages: Option<Arc<WorkspacePackages>>,
}

impl WorkspaceDiscoveryCache {
    pub(super) fn load(
        &self,
        discover: impl FnOnce() -> Result<Option<Vec<Project>>, InstallError>,
    ) -> Result<WorkspaceSnapshot, InstallError> {
        let mut snapshot = self.snapshot.lock().expect("workspace discovery lock is not poisoned");
        if let Some(snapshot) = snapshot.as_ref() {
            return Ok(snapshot.clone());
        }
        let projects = discover()?.map(Arc::<[Project]>::from);
        let loaded = WorkspaceSnapshot {
            packages: build_workspace_packages_map(projects.as_deref()).map(Arc::new),
            projects,
        };
        *snapshot = Some(loaded.clone());
        Ok(loaded)
    }
}

/// Workspace resolver inputs, built only when a fresh install needs them.
pub(super) enum WorkspacePackageSource<'a> {
    Shared(Arc<WorkspacePackages>),
    Projects(Option<&'a [Project]>),
}

impl WorkspacePackageSource<'_> {
    pub(super) fn into_packages(self) -> Option<Arc<WorkspacePackages>> {
        match self {
            Self::Shared(packages) => Some(packages),
            Self::Projects(projects) => build_workspace_packages_map(projects).map(Arc::new),
        }
    }
}
