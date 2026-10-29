//! What the run has settled before it dispatches.

use super::{
    InstallOwned, InstallView, RunMode, Verification,
    lockfile_load::Loaded,
    wanted::Lockfiles,
    workspace::{InstallScope, InstallWorkspace},
};
use pnpm_package_manifest::PackageManifest;
use std::path::PathBuf;

/// Everything the run has settled before it dispatches.
#[derive(Clone, Copy)]
pub(super) struct Settled<'r, 'a> {
    pub(super) install: InstallView<'a>,
    pub(super) owned: &'r InstallOwned,
    pub(super) mode: &'r RunMode,
    pub(super) loaded: &'r Loaded<'a>,
    pub(super) lockfiles: &'r Lockfiles<'a>,
    pub(super) verification: &'r Verification,
    pub(super) projects: SettledProjects<'r, 'a>,
}

impl<'r, 'a> Settled<'r, 'a> {
    /// Borrowed field by field, so the run's options stay free for
    /// `dispatch` to consume.
    pub(super) fn new(
        (install, owned, mode, workspace): (
            InstallView<'a>,
            &'r InstallOwned,
            &'r RunMode,
            &'r InstallWorkspace<'a>,
        ),
        (loaded, lockfiles, verification): (&'r Loaded<'a>, &'r Lockfiles<'a>, &'r Verification),
        (scope, project_manifests): (&'r InstallScope<'a>, &'r [(PathBuf, &'a PackageManifest)]),
    ) -> Self {
        Settled {
            install,
            owned,
            mode,
            loaded,
            lockfiles,
            verification,
            projects: SettledProjects { workspace, scope, project_manifests },
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct SettledProjects<'r, 'a> {
    pub(super) workspace: &'r InstallWorkspace<'a>,
    pub(super) scope: &'r InstallScope<'a>,
    pub(super) project_manifests: &'r [(PathBuf, &'a PackageManifest)],
}

impl<'r> SettledProjects<'r, '_> {
    pub(super) fn metadata(self) -> crate::install::state_options::InstallProjectMetadata<'r> {
        crate::install::state_options::InstallProjectMetadata {
            catalogs: &self.workspace.catalogs,
            manifests: self.project_manifests,
            declared_manifests: self.scope.declared_manifests(),
            workspace_projects: self.scope.workspace_projects.as_deref(),
            prefix: &self.workspace.prefix,
            workspace_packages: self.workspace.workspace_packages.as_deref(),
        }
    }
}
