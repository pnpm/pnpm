//! Pre-clean frozen-lockfile validation for `pnpm ci`.
//!
//! Runs every read-only gate that rejects an invalid, missing, or outdated
//! frozen lockfile before `pnpm ci` removes `node_modules`.

use super::{
    super::{
        DependencyGroup, Install, InstallError, InstallRunOptions, LogEvent, PackageManifest,
        PathBuf, verify_lockfile_eagerly,
    },
    InstallScope, InstallWorkspace, Loaded, RunExecution, RunMode, Verification,
    dispatch::{FrozenDispatch, decide_frozen_path},
    frozen_local_tarballs::local_tarballs_keep_frozen_path,
    lockfile_load::{load_hooked_manifests, load_wanted_lockfile},
    pre_resolution::run_pre_resolution_hook,
    reject_frozen_with_update_checksums, settle_wanted_lockfile,
    settled::Settled,
    workspace::{report_install_scope_cycles, workspace_projects},
};
use crate::WorkspaceInstallSelection;

struct SilentReporter;

impl pnpm_reporter::Reporter for SilentReporter {
    fn emit(_: &LogEvent) {}
}

impl<DependencyGroupList> Install<'_, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub(crate) async fn validate_frozen_inner(
        self,
        selection: Option<WorkspaceInstallSelection<'_>>,
    ) -> Result<(), InstallError> {
        let options = InstallRunOptions { selection, ..Default::default() };
        let (install, mut owned) = self.split();
        let mode = RunMode::settle(install, &owned, &options)?;
        owned.projects.dependency_groups = super::super::project_dependency_groups(std::mem::take(
            &mut owned.projects.dependency_groups,
        ));
        let mut workspace =
            InstallWorkspace::discover::<SilentReporter>(install, &mut owned, &options)?;
        let loaded_workspace_projects = workspace.loaded_workspace.projects.take();
        Box::pin(
            RunExecution {
                install,
                owned,
                prefetch_downloads: None,
                store_verification: None,
                mode,
                workspace,
                options,
                loaded_workspace_projects: loaded_workspace_projects.as_deref(),
            }
            .validate_frozen(),
        )
        .await
    }
}

impl<'a> RunExecution<'a> {
    async fn validate_frozen(mut self) -> Result<(), InstallError> {
        let scope = self.select_scope();
        let mut loaded = self.load_for_frozen_validation(&scope).await?;
        let manifests = std::mem::take(&mut loaded.manifests);
        let project_manifests = manifests.view(&scope.project_manifests);
        self.validate_project_manifests(&project_manifests)?;
        self.verify_settled_frozen(&scope, &mut loaded, &project_manifests).await
    }

    async fn load_for_frozen_validation(
        &mut self,
        scope: &InstallScope<'_>,
    ) -> Result<Loaded<'a>, InstallError> {
        reject_frozen_with_update_checksums(
            self.install.lockfile_policy.update_checksums,
            self.install.lockfile_policy.frozen,
        )?;
        report_install_scope_cycles::<SilentReporter>(
            self.install.context.config,
            &self.workspace,
            self.options.selection.as_ref(),
            (
                self.install.execution.mutation,
                workspace_projects(self.loaded_workspace_projects, self.options.selection.as_ref()),
            ),
        )?;
        let wanted = load_wanted_lockfile::<SilentReporter>(
            self.install.context.lockfile,
            self.install.lockfile_policy.frozen,
            (&self.workspace.dirs.workspace_root, &self.workspace.prefix),
        )?;
        let (pnpmfile_hook, manifests) = load_hooked_manifests::<SilentReporter>(
            self.install,
            &mut self.owned,
            &self.workspace,
            scope,
            &self.options.manifests.hooked_paths,
        )
        .await?;
        Ok(Loaded { wanted, current: None, early_host_detection: None, pnpmfile_hook, manifests })
    }

    async fn verify_settled_frozen(
        self,
        scope: &InstallScope<'_>,
        loaded: &mut Loaded<'_>,
        project_manifests: &[(PathBuf, &PackageManifest)],
    ) -> Result<(), InstallError> {
        let mut lockfiles = settle_wanted_lockfile::<SilentReporter>(
            self.install,
            &self.mode,
            &self.workspace,
            scope,
            loaded,
            project_manifests,
            self.options.selection.as_ref(),
        )
        .await?;
        let verification = Verification::set_up(&self, &lockfiles, (scope, project_manifests))?;
        run_pre_resolution_hook::<SilentReporter>(
            self.install,
            &self.workspace,
            loaded,
            &mut lockfiles.wanted,
        )
        .await?;
        let settled = Settled::new(
            (self.install, &self.owned, &self.mode, &self.workspace),
            (loaded, &lockfiles, &verification),
            (scope, project_manifests),
        );
        verify_settled_frozen_path(settled).await
    }
}

async fn verify_settled_frozen_path(settled: Settled<'_, '_>) -> Result<(), InstallError> {
    let take_frozen_path =
        decide_frozen_path::<SilentReporter>(&FrozenDispatch::new(settled, None)).await?;
    if take_frozen_path && !settled.mode.lockfile_only {
        local_tarballs_keep_frozen_path(settled).await?;
    }
    if take_frozen_path && let Some(lockfile) = settled.lockfiles.wanted.get() {
        verify_lockfile_eagerly::<SilentReporter>(
            lockfile,
            &settled.verification.resolution_verifiers,
            settled.verification.derived_lockfile_path.as_deref(),
            &settled.install.context.config.cache_dir,
        )
        .await?;
    }
    Ok(())
}
