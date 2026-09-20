use super::{
    super::{
        ApplyMaterializationInputs, Arc, AtomicU8, IncludedDependencies, InstallError, LogEvent,
        LogLevel, MaterializationInputs, PackageManifest, PathBuf, RebuildOptions, Reporter,
        SummaryLog, apply_materialization_result, materialize, prior_hoisted_dependencies,
        prior_hoisted_locations,
    },
    Dispatched, InstallRunOutcome, InstallScope, Loaded, Lockfiles, RunExecution, Settled,
    Verification, dispatch, load_lockfiles, settle_wanted_lockfile, workspace_projects,
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
    ) -> Result<InstallRunOutcome, InstallError> {
        let scope = self.select_scope();
        if scope.is_already_up_to_date::<Reporter>(
            self.install,
            &self.owned,
            &self.mode,
            &self.workspace,
        )? {
            Reporter::emit(&LogEvent::Summary(SummaryLog {
                level: LogLevel::Debug,
                prefix: self.workspace.prefix,
            }));
            return Ok(InstallRunOutcome::AlreadyUpToDate);
        }
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
        self.install_settled::<Reporter>(&scope, &mut loaded, &project_manifests, &lockfiles).await
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
        let verification = Verification::set_up(self, lockfiles.wanted.get().is_some())?;
        let Some(mut dispatched) = dispatch::<Reporter>(
            Settled {
                install: self.install,
                owned: &self.owned,
                mode: &self.mode,
                loaded,
                lockfiles,
                verification: &verification,
                projects: crate::install::run::dispatch::SettledProjects {
                    workspace: &self.workspace,
                    scope,
                    project_manifests,
                },
            },
            &mut self.options,
        )
        .await?
        else {
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
        self.finish_materialization::<Reporter>(
            (scope, project_manifests),
            loaded,
            lockfiles,
            dispatched,
            materialized,
        )
        .await
    }

    async fn finish_materialization<Reporter: self::Reporter + 'static>(
        &mut self,
        projects: (&InstallScope<'_>, &[(PathBuf, &PackageManifest)]),
        loaded: &mut Loaded<'_>,
        lockfiles: &Lockfiles<'_>,
        dispatched: Dispatched<'a>,
        materialized: super::super::materialize::MaterializationOutput,
    ) -> Result<InstallRunOutcome, InstallError> {
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
                &self.owned,
                &self.options,
                dispatched.take_frozen_path,
                early_host_detection,
                &self.workspace.prefix,
            ),
            downloads: (&self.owned).into(),
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
        }
    }

    fn take_project_scripts(
        &mut self,
    ) -> crate::install::state_options::PendingProjectScripts<'a, 'a> {
        crate::install::state_options::PendingProjectScripts {
            mutation: self.install.execution.mutation,
            manifest_dir: self.workspace.dirs.manifest_dir,
            selection: self.options.selection.take(),
            rebuild: self.options.rebuild.take(),
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
        let workspace_packages = self.workspace.workspace_packages.take();
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
                },
                workspace_packages,
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
            scripts: self.take_project_scripts(),
            write: lockfiles.write_policy(self.options.save_lockfile),
            materialized,
        }
    }
}
/// The install's borrowed and `Copy` inputs, as one value every phase reads.
pub(super) fn materialization_lockfiles<'r, 'install>(
    loaded: &'r mut Loaded<'_>,
    lockfiles: &'r Lockfiles<'_>,
    dispatched: &mut Dispatched<'install>,
    verification: Verification,
) -> super::super::materialize::MaterializationLockfiles<'r, 'install> {
    super::super::materialize::MaterializationLockfiles {
        wanted: lockfiles.wanted.get(),
        wanted_shared: lockfiles.wanted.loader_handle(loaded.wanted.shared.take()),
        merge_wanted: loaded.wanted.merge,
        current: loaded.current.as_ref(),
        verification,
        verification_override: dispatched.modules.lockfile_verification_override.take(),
    }
}

impl super::InstallWorkspace<'_> {
    fn materialization_workspace<'r>(
        &'r self,
        (dependency_groups, lockfile_specifier_project_manifests): (
            Vec<pnpm_package_manifest::DependencyGroup>,
            Option<Vec<(PathBuf, PackageManifest)>>,
        ),
        (project_manifests, workspace_projects): (
            &'r [(PathBuf, &'r PackageManifest)],
            Option<&'r [pnpm_workspace::Project]>,
        ),
        scope: &'r InstallScope<'_>,
    ) -> crate::install::materialize::MaterializationWorkspace<'r> {
        crate::install::materialize::MaterializationWorkspace {
            dependency_groups,
            project_manifests,
            lockfile_specifier_project_manifests,
            workspace_projects,
            requested_importer_ids: scope.importers.requested_importer_ids.as_ref(),
            real_importer_ids: &scope.importers.real_importer_ids,
            workspace_root: &self.dirs.workspace_root,
            catalogs: &self.catalogs,
        }
    }
}

impl Dispatched<'_> {
    fn materialization_modules<'r>(
        &'r self,
        (included, rebuild): (IncludedDependencies, Option<&'r RebuildOptions>),
        prune_orphans: bool,
        logged_methods: &'r AtomicU8,
    ) -> crate::install::materialize::MaterializationModules<'r> {
        let prior_modules = self.modules.previous_modules_metadata.as_ref();
        crate::install::materialize::MaterializationModules {
            included,
            rebuild,
            modules_manifest: self.modules.old_modules.as_ref(),
            prior_hoisted_dependencies: prior_hoisted_dependencies(prior_modules),
            prior_hoisted_locations: prior_hoisted_locations(prior_modules),
            prune_orphans,
            relink_every_slot_bin: self.modules.tree_moved,
            logged_methods,
        }
    }
}

impl super::RunMode {
    fn materialization_execution<'r>(
        &mut self,
        owned: &'r super::InstallOwned,
        options: &super::InstallRunOptions<'_, '_>,
        take_frozen_path: bool,
        early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
        prefix: &'r str,
    ) -> crate::install::materialize::MaterializationExecution<'r> {
        crate::install::materialize::MaterializationExecution {
            effective_node_version: self.effective_node_version.take(),
            take_frozen_path,
            supported_architectures: owned.projects.supported_architectures.as_ref(),
            early_host_detection,
            resolve_only: self.resolve_only,
            can_prompt: self.can_prompt,
            save_lockfile: options.save_lockfile,
            prefix,
        }
    }
}

impl From<&super::InstallOwned> for crate::install::materialize::MaterializationDownloads {
    fn from(owned: &super::InstallOwned) -> Self {
        Self {
            tarball_mem_cache: Arc::clone(&owned.tarball_mem_cache),
            http_client_arc: Arc::clone(&owned.http_client_arc),
        }
    }
}

impl Lockfiles<'_> {
    pub(super) fn write_policy(
        &self,
        save: bool,
    ) -> crate::install::state_options::LockfileWritePolicy {
        crate::install::state_options::LockfileWritePolicy {
            synthesized_from_current: self.wanted.synthesized_from_current(),
            fast_updated: self.wanted.was_fast_updated(),
            save,
        }
    }
}
