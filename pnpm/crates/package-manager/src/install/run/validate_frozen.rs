//! Pre-clean frozen-lockfile validation and hook reuse for `pnpm ci`.
//!
//! Runs every read-only gate that rejects an invalid, missing, or outdated
//! frozen lockfile before `pnpm ci` removes `node_modules`, and returns the
//! hooked manifests and `preResolution` result so the subsequent install runs
//! each hook once.

mod checks;

use super::{
    super::{
        Arc, DependencyGroup, Install, InstallError, InstallManifestOptions, InstallRunOptions,
        Lockfile, LogEvent, PackageManifest, PathBuf,
    },
    HookedManifests, InstallScope, InstallWorkspace, Loaded, RunExecution, RunMode, Verification,
    lockfile_load::{
        join_current_lockfile_load, load_hooked_manifests, load_wanted_lockfile,
        spawn_current_lockfile_load,
    },
    pre_resolution::run_pre_resolution_hook,
    reject_frozen_with_update_checksums, settle_wanted_lockfile,
    settled::Settled,
    workspace::{report_install_scope_cycles, workspace_projects},
};
use crate::{LockfileVerificationOverride, WorkspaceInstallSelection};
use checks::verify_settled_frozen_path;
use std::marker::PhantomData;

/// Whether `pnpm ci`'s cleanup step removes or preserves the virtual store's
/// current lockfile (`<install_state_dir>/lock.yaml`).
#[derive(Debug, Clone, Copy)]
pub enum FrozenVirtualStoreClean {
    RemovesVirtualStore,
    PreservesVirtualStore,
}

/// Hooked manifests, `preResolution` output, and verification state produced
/// by [`Install::prepare_frozen`], consumed by [`Install::run_prepared`] or
/// [`Install::run_selected_prepared`].
pub struct PreparedFrozenInstall {
    pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    manifests: HookedManifests,
    pre_resolution: PreparedPreResolution,
    lockfile_verified: bool,
}

pub(crate) struct PreparedFrozenRun {
    pub(crate) manifests: Option<HookedManifests>,
    pub(crate) pre_resolution: Option<PreparedPreResolution>,
}

pub(crate) struct PreparedPreResolution {
    pub(crate) hooked_wanted: Option<Lockfile>,
    pub(crate) hooked_current: Option<Lockfile>,
}

struct SilentReporter;

impl pnpm_reporter::Reporter for SilentReporter {
    fn emit(_: &LogEvent) {}
}

struct HookOnlyReporter<Inner>(PhantomData<Inner>);

impl<Inner: pnpm_reporter::Reporter> pnpm_reporter::Reporter for HookOnlyReporter<Inner> {
    fn emit(event: &LogEvent) {
        if matches!(event, LogEvent::Hook(_)) {
            Inner::emit(event);
        }
    }
}

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub(crate) async fn validate_frozen_silent(
        self,
        selection: Option<WorkspaceInstallSelection<'_>>,
    ) -> Result<(), InstallError> {
        self.prepare_frozen::<SilentReporter>(
            selection,
            FrozenVirtualStoreClean::RemovesVirtualStore,
        )
        .await
        .map(|_| ())
    }

    /// Validate non-frozen preflight invariants (mode compatibility, workspace
    /// cycles, and raw project manifests) for `pnpm ci --fix-lockfile` before
    /// `node_modules` is cleaned.
    pub async fn validate_preflight(
        mut self,
        selection: Option<WorkspaceInstallSelection<'_>>,
    ) -> Result<(), InstallError> {
        self.execution.dry_run = false;
        let options = InstallRunOptions { selection, ..Default::default() };
        let (install, mut owned) = self.split();
        let mode = RunMode::settle(install, &owned, &options)?;
        owned.projects.dependency_groups = super::super::project_dependency_groups(std::mem::take(
            &mut owned.projects.dependency_groups,
        ));
        let mut workspace =
            InstallWorkspace::discover::<SilentReporter>(install, &mut owned, &options)?;
        let loaded_workspace_projects = workspace.loaded_workspace.projects.take();
        let execution = RunExecution {
            install,
            owned,
            prefetch_downloads: None,
            store_verification: None,
            mode,
            workspace,
            options,
            loaded_workspace_projects: loaded_workspace_projects.as_deref(),
        };
        execution.validate_preflight_only()
    }

    /// Validate every frozen-lockfile precondition before `pnpm ci` cleans
    /// `node_modules` and return [`PreparedFrozenInstall`] for reuse.
    pub async fn prepare_frozen<Reporter: pnpm_reporter::Reporter + 'static>(
        mut self,
        selection: Option<WorkspaceInstallSelection<'_>>,
        virtual_store_clean: FrozenVirtualStoreClean,
    ) -> Result<PreparedFrozenInstall, InstallError> {
        self.lockfile_policy.frozen = true;
        self.execution.dry_run = false;
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
            .prepare_frozen::<Reporter>(virtual_store_clean),
        )
        .await
    }

    /// Execute the install reusing artifacts from [`Self::prepare_frozen`].
    pub async fn run_prepared<Reporter: pnpm_reporter::Reporter + 'static>(
        mut self,
        prepared: Option<PreparedFrozenInstall>,
    ) -> Result<(), InstallError> {
        let options = self.prepare_run_options(prepared, None);
        Box::pin(self.run_inner::<Reporter>(options)).await
    }

    /// Execute a selected workspace install reusing [`Self::prepare_frozen`].
    pub async fn run_selected_prepared<Reporter: pnpm_reporter::Reporter + 'static>(
        mut self,
        selection: WorkspaceInstallSelection<'_>,
        prepared: Option<PreparedFrozenInstall>,
    ) -> Result<(), InstallError> {
        let options = self.prepare_run_options(prepared, Some(selection));
        Box::pin(self.run_inner::<Reporter>(options)).await
    }

    fn prepare_run_options<'selection>(
        &mut self,
        prepared: Option<PreparedFrozenInstall>,
        selection: Option<WorkspaceInstallSelection<'selection>>,
    ) -> InstallRunOptions<'a, 'selection> {
        let Some(prepared) = prepared else {
            return InstallRunOptions { selection, ..Default::default() };
        };
        if let Some(hook) = prepared.pnpmfile_hook {
            self.projects.pnpmfile_hook_override = Some(hook);
        }
        let lockfile_verification_override = prepared.lockfile_verified.then(|| {
            Box::pin(async { Ok(()) }) as LockfileVerificationOverride<'a>
        });
        InstallRunOptions {
            selection,
            lockfile_verification_override,
            manifests: InstallManifestOptions {
                prepared_frozen: Some(PreparedFrozenRun {
                    manifests: Some(prepared.manifests),
                    pre_resolution: Some(prepared.pre_resolution),
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

impl<'a> RunExecution<'a> {
    fn validate_preflight_only(&self) -> Result<(), InstallError> {
        let scope = self.select_scope();
        report_install_scope_cycles::<SilentReporter>(
            self.install.context.config,
            &self.workspace,
            self.options.selection.as_ref(),
            (
                self.install.execution.mutation,
                workspace_projects(self.loaded_workspace_projects, self.options.selection.as_ref()),
            ),
        )?;
        self.validate_project_manifests(&scope.project_manifests)
    }

    async fn prepare_frozen<Reporter: pnpm_reporter::Reporter + 'static>(
        mut self,
        virtual_store_clean: FrozenVirtualStoreClean,
    ) -> Result<PreparedFrozenInstall, InstallError> {
        let scope = self.select_scope();
        let mut loaded =
            self.load_for_frozen_validation::<Reporter>(&scope, virtual_store_clean).await?;
        let manifests = std::mem::take(&mut loaded.manifests);
        let (pre_resolution, lockfile_verified) = {
            let project_manifests = manifests.view(&scope.project_manifests);
            self.validate_project_manifests(&project_manifests)?;
            self.verify_settled_frozen::<Reporter>(&scope, &mut loaded, &project_manifests).await?
        };
        Ok(PreparedFrozenInstall {
            pnpmfile_hook: loaded.pnpmfile_hook,
            manifests,
            pre_resolution,
            lockfile_verified,
        })
    }

    async fn load_for_frozen_validation<Reporter: pnpm_reporter::Reporter + 'static>(
        &mut self,
        scope: &InstallScope<'_>,
        virtual_store_clean: FrozenVirtualStoreClean,
    ) -> Result<Loaded<'a>, InstallError> {
        reject_frozen_with_update_checksums(
            self.install.lockfile_policy.update_checksums,
            self.install.lockfile_policy.frozen,
        )?;
        self.validate_preflight_only()?;
        let current_task = match virtual_store_clean {
            FrozenVirtualStoreClean::PreservesVirtualStore => {
                Some(spawn_current_lockfile_load(self.install.context.config))
            }
            FrozenVirtualStoreClean::RemovesVirtualStore => None,
        };
        let wanted = load_wanted_lockfile::<SilentReporter>(
            self.install.context.lockfile,
            self.install.lockfile_policy.frozen,
            (&self.workspace.dirs.workspace_root, &self.workspace.prefix),
        )?;
        self.validate_raw_wanted_before_hooks(scope, wanted.lockfile).await?;
        let (pnpmfile_hook, manifests) = load_hooked_manifests::<HookOnlyReporter<Reporter>>(
            self.install,
            &mut self.owned,
            &self.workspace,
            scope,
            &self.options.manifests.hooked_paths,
        )
        .await?;
        let current = finish_current_lockfile_load::<Reporter>(
            current_task,
            self.install.context.config,
            &self.workspace.prefix,
        )
        .await;
        Ok(Loaded { wanted, current, early_host_detection: None, pnpmfile_hook, manifests })
    }

    async fn verify_settled_frozen<Reporter: pnpm_reporter::Reporter + 'static>(
        self,
        scope: &InstallScope<'_>,
        loaded: &mut Loaded<'_>,
        project_manifests: &[(PathBuf, &PackageManifest)],
    ) -> Result<(PreparedPreResolution, bool), InstallError> {
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
        let hooked_current = run_pre_resolution_hook::<HookOnlyReporter<Reporter>>(
            self.install,
            &self.workspace,
            loaded,
            &mut lockfiles.wanted,
        )
        .await?;
        let lockfile_verified = {
            let settled = Settled::new(
                (self.install, &self.owned, &self.mode, &self.workspace),
                (loaded, &lockfiles, &verification),
                (scope, project_manifests),
            );
            verify_settled_frozen_path(settled).await?
        };
        let pre_resolution =
            PreparedPreResolution { hooked_wanted: lockfiles.wanted.take_hooked(), hooked_current };
        Ok((pre_resolution, lockfile_verified))
    }
}

async fn finish_current_lockfile_load<Reporter: pnpm_reporter::Reporter + 'static>(
    current_task: Option<
        tokio::task::JoinHandle<Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError>>,
    >,
    config: &pnpm_config::Config,
    prefix: &str,
) -> Option<Lockfile> {
    let task = current_task?;
    join_current_lockfile_load::<HookOnlyReporter<Reporter>>(task, config, prefix).await
}
