use super::{
    super::{
        ApplyMaterializationInputs, AtomicU8, InstallError, MaterializationInputs, PackageManifest,
        PathBuf, Reporter, apply_materialization_result, materialize,
    },
    Dispatched, InstallRunOutcome, InstallScope, Loaded, Lockfiles, RepeatInstallVerdict,
    RunExecution, Verification, dispatch, load_lockfiles,
    materialization_inputs::materialization_lockfiles,
    report_already_up_to_date, settle_wanted_lockfile,
    settled::Settled,
    time_machine_capture::capture_time_machine_exclusions,
    workspace_projects,
};

impl<'a> RunExecution<'a> {
    fn select_scope(&self) -> InstallScope<'a> {
        InstallScope::select(
            self.install,
            &self.workspace.dirs.workspace_root,
            workspace_projects(self.loaded_workspace_projects, self.options.selection.as_ref()),
            self.workspace.workspace_projects_are_overridden,
            &self.options,
        )
    }

    pub(super) async fn run<Reporter: self::Reporter + 'static>(
        mut self,
        time_machine_exclusions: &mut super::super::TimeMachineExclusions,
    ) -> Result<InstallRunOutcome, InstallError> {
        let mut scope = self.select_scope();
        capture_time_machine_exclusions(&self, &scope, time_machine_exclusions);
        scope.project_scripts_current = match self.repeat_install_verdict(&scope).await? {
            RepeatInstallVerdict::Unchanged => {
                return Ok(report_already_up_to_date::<Reporter>(self.workspace.prefix));
            }
            RepeatInstallVerdict::UnchangedFrozen => true,
            RepeatInstallVerdict::Changed => false,
        };
        let mut loaded = load_lockfiles::<Reporter>(
            self.install,
            &mut self.owned,
            &self.mode,
            &self.workspace,
            &scope,
            self.options.selection.as_ref(),
            (&self.options.manifests.hooked_paths, self.loaded_workspace_projects),
        )
        .await?;
        let manifests = std::mem::take(&mut loaded.manifests);
        let project_manifests = manifests.view(&scope.project_manifests);
        self.validate_project_manifests(&project_manifests)?;
        let lockfiles = settle_wanted_lockfile::<Reporter>(
            self.install,
            &self.mode,
            &self.workspace,
            &scope,
            &loaded,
            &project_manifests,
            self.options.selection.as_ref(),
        )
        .await?;
        Box::pin(self.install_settled::<Reporter>(
            &scope,
            &mut loaded,
            &project_manifests,
            &lockfiles,
        ))
        .await
    }

    fn take_settled_outcome(&mut self) -> InstallRunOutcome {
        InstallRunOutcome::LockfileSettled {
            workspace_manifest_dir: std::mem::take(&mut self.workspace.dirs.workspace_manifest_dir),
        }
    }

    async fn install_settled<Reporter: self::Reporter + 'static>(
        &mut self,
        scope: &InstallScope<'_>,
        loaded: &mut Loaded<'_>,
        project_manifests: &[(PathBuf, &PackageManifest)],
        lockfiles: &Lockfiles<'_>,
    ) -> Result<InstallRunOutcome, InstallError> {
        let verification = Verification::set_up(self, lockfiles, (scope, project_manifests))?;
        if let Some(message) = self.install.context.config.bypassed_home_store_warning() {
            pnpm_reporter::emit_global_warning::<Reporter>(&message);
        }
        let settled = Settled::new(
            (self.install, &self.owned, &self.mode, &self.workspace),
            (loaded, lockfiles, &verification),
            (scope, project_manifests),
        );
        let Some(mut dispatched) = dispatch::<Reporter>(settled, &mut self.options).await? else {
            super::store_fetch::fetch_wanted_lockfile::<Reporter>(self.store_fetch(), lockfiles)
                .await?;
            return Ok(self.take_settled_outcome());
        };
        let materialized = materialize::<Reporter>(self.materialization_inputs(
            (scope, project_manifests),
            loaded,
            lockfiles,
            &mut dispatched,
            (verification, &AtomicU8::new(0)),
        ))
        .await?;
        super::store_fetch::finish_fresh_fetch::<Reporter>(
            self.store_fetch(),
            materialized.materialized.fresh_lockfile.as_ref(),
        )
        .await?;
        self.finish_materialization::<Reporter>(
            (scope, project_manifests),
            loaded,
            lockfiles,
            dispatched,
            materialized,
        )
        .await
    }

    fn store_fetch(&self) -> super::store_fetch::StoreFetchRun<'_> {
        super::store_fetch::StoreFetchRun {
            mode: &self.mode,
            install: self.install,
            owned: &self.owned,
            requester: &self.workspace.prefix,
            prefetch_downloads: self.prefetch_downloads.as_ref(),
        }
    }

    async fn finish_materialization<Reporter: self::Reporter + 'static>(
        &mut self,
        projects: (&InstallScope<'_>, &[(PathBuf, &PackageManifest)]),
        loaded: &mut Loaded<'_>,
        lockfiles: &Lockfiles<'_>,
        dispatched: Dispatched<'a>,
        materialized: super::super::materialize::MaterializationOutput,
    ) -> Result<InstallRunOutcome, InstallError> {
        let dependencies_installed = self.owned.projects.dedicated
            .as_ref()
            .and_then(|dedicated| dedicated.dependencies_installed.clone());
        if let Err(error) = wait_for_workspace_dependencies(dependencies_installed).await {
            pnpm_store_dir::StoreIndexWriter::drain(
                materialized.store_index_teardown,
                "; some rows may not be persisted",
            )
            .await;
            return Err(error);
        }
        let workspace_manifest_dir = self.workspace.dirs.workspace_manifest_dir.clone();
        apply_materialization_result::<Reporter>(self.apply_inputs(
            projects,
            loaded,
            lockfiles,
            dispatched,
            materialized.materialized,
        ))
        .await?;
        pnpm_store_dir::StoreIndexWriter::drain(
            materialized.store_index_teardown,
            "; some rows may not be persisted",
        )
        .await;
        Ok(InstallRunOutcome::LockfileSettled { workspace_manifest_dir })
    }

    fn materialization_inputs<'r>(
        &'r mut self,
        (scope, project_manifests): (&'r InstallScope<'_>, &'r [(PathBuf, &PackageManifest)]),
        loaded: &'r mut Loaded<'_>,
        lockfiles: &'r Lockfiles<'_>,
        dispatched: &'r mut Dispatched<'a>,
        (verification, logged_methods): (Verification, &'r AtomicU8),
    ) -> MaterializationInputs<'r, 'a> {
        let resolution = self.materialization_resolution(loaded);
        let early_host_detection = loaded.early_host_detection.take();
        let lockfiles = materialization_lockfiles(loaded, lockfiles, dispatched, verification);
        MaterializationInputs {
            install: self.install,
            resolution,
            lockfiles,
            workspace: self.workspace.materialization_workspace(
                (
                    std::mem::take(&mut self.owned.projects.dependency_groups),
                    self.options.manifests.specifier_manifests.take(),
                ),
                (
                    project_manifests,
                    workspace_projects(
                        self.loaded_workspace_projects,
                        self.options.selection.as_ref(),
                    ),
                ),
                scope,
            ),
            modules: dispatched.materialization_modules(
                (self.mode.included, self.options.rebuild.as_ref()),
                !scope.importers.filtered_install,
                logged_methods,
            ),
            execution: self.mode.materialization_execution(
                (&self.owned, self.install.execution),
                &self.options,
                dispatched.take_frozen_path,
                early_host_detection,
                &self.workspace.prefix,
            ),
            downloads: crate::install::materialize::MaterializationDownloads {
                prefetch_downloads: self.prefetch_downloads.clone(),
                ..(&self.owned).into()
            },
        }
    }

    fn materialization_resolution<'r>(
        &mut self,
        loaded: &mut Loaded<'r>,
    ) -> super::super::materialize::MaterializationResolution<'a> {
        super::super::materialize::MaterializationResolution {
            pnpmfile_hook: loaded.pnpmfile_hook.take(),
            deploy_manifest_hook: self.options.manifests.deploy_hook,
            manifest_spec_bumps: self.options.manifests.spec_bumps,
            inputs: std::mem::take(&mut self.owned.resolution),
        }
    }

    fn take_completion_context(&mut self) -> crate::install::state_options::ApplyCompletionContext {
        crate::install::state_options::ApplyCompletionContext {
            prefix: std::mem::take(&mut self.workspace.prefix),
            workspace_manifest_dir: std::mem::take(&mut self.workspace.dirs.workspace_manifest_dir),
            catalogs: std::mem::take(&mut self.workspace.catalogs),
            catalog_context_present: self.workspace.catalog_context_present,
            verified_file_integrity_baseline: self.mode.verified_file_integrity_baseline,
            config: self.install.context.config,
            save_workspace_state: self.options.save.workspace_state,
            can_prompt: self.mode.can_prompt,
        }
    }

    fn take_project_scripts(
        &mut self,
        root_preinstall_ran: bool,
    ) -> crate::install::state_options::PendingProjectScripts<'a, 'a> {
        crate::install::state_options::PendingProjectScripts {
            mutation: self.install.execution.mutation,
            manifest_dir: self.workspace.dirs.manifest_dir,
            selection: self.options.selection.take(),
            rebuild: self.options.rebuild.take(),
            root_preinstall_ran,
        }
    }

    fn apply_inputs<'r>(
        &mut self,
        projects: (&'r InstallScope<'_>, &'r [(PathBuf, &'r PackageManifest)]),
        loaded: &mut Loaded<'_>,
        lockfiles: &'r Lockfiles<'_>,
        dispatched: Dispatched<'a>,
        materialized: super::super::materialize::Materialized,
    ) -> ApplyMaterializationInputs<'r, 'a>
    where
        'a: 'r,
    {
        let (scope, project_manifests) = projects;
        ApplyMaterializationInputs {
            completion: self.take_completion_context(),
            mode: crate::install::state_options::CompletionMode {
                resolve_only: self.mode.resolve_only,
                dry_run: self.install.execution.dry_run,
                peer_issues_sink_is_none: self.mode.peer_issues_sink_is_none,
            },
            prior: crate::install::state_options::ApplyPriorState {
                lockfile: loaded.current.take(),
                layout: dispatched.modules.old_modules,
                metadata: dispatched.modules.previous_modules_metadata,
                is_inconsistent: dispatched.modules.is_inconsistent,
                tree_moved: dispatched.modules.tree_moved,
            },
            projects: crate::install::state_options::ApplyProjectSelection {
                importers: crate::install::state_options::SelectedImporters {
                    requested_ids: scope.importers.requested_importer_ids.as_ref(),
                    real_ids: &scope.importers.real_importer_ids,
                    manifests: project_manifests,
                    ignore_manifest_check: self.install.lockfile_policy.ignore_manifest_check,
                },
                workspace_packages: self.workspace.workspace_packages.take(),
                workspace_root: std::mem::take(&mut self.workspace.dirs.workspace_root),
                included: self.mode.included,
                node_linker: self.install.execution.node_linker,
                filtered_install: scope.importers.filtered_install,
                supported_architectures: self.owned.projects
                    .supported_architectures
                    .take(),
            },
            resolution: crate::install::state_options::ApplyResolutionState {
                existing_wanted: lockfiles.wanted.loaded,
                loaded: lockfiles.wanted.get(),
                frozen: dispatched.take_frozen_path,
            },
            scripts: self.take_project_scripts(dispatched.root_preinstall_ran),
            write: lockfiles.write_policy(self.options.save.lockfile),
            materialized,
        }
    }
}
pub(super) async fn wait_for_workspace_dependencies(
    dependencies_installed: Option<crate::WorkspaceDependenciesInstalled>,
) -> Result<(), InstallError> {
    let Some(dependencies_installed) = dependencies_installed else { return Ok(()) };
    if dependencies_installed.await { Ok(()) } else { Err(InstallError::WorkspaceDependencyFailed) }
}
