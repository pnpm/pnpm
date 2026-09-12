use super::{
    super::{
        ApplyMaterializationInputs, Arc, AtomicU8, InstallError, LogEvent, LogLevel,
        MaterializationInputs, PackageManifest, PathBuf, Reporter, SummaryLog, UpdateSeedPolicy,
        apply_materialization_result, materialize, prior_hoisted_dependencies,
        prior_hoisted_locations,
    },
    Dispatched, InstallRunOutcome, InstallScope, Loaded, Lockfiles, RunExecution, Settled,
    Verification, dispatch, load_lockfiles, settle_wanted_lockfile, workspace_projects,
};

impl<'a> RunExecution<'a> {
    fn select_scope(&self) -> InstallScope<'a> {
        InstallScope::select(
            self.install,
            &self.workspace.workspace_root,
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
            (&self.options.read_package_hooked_manifest_paths, self.loaded_workspace_projects),
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
                workspace: &self.workspace,
                scope,
                loaded,
                project_manifests,
                lockfiles,
                verification: &verification,
            },
            &mut self.options,
        )
        .await?
        else {
            return Ok(InstallRunOutcome::LockfileSettled {
                workspace_manifest_dir: std::mem::take(&mut self.workspace.workspace_manifest_dir),
            });
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
        let workspace_manifest_dir = self.workspace.workspace_manifest_dir.clone();
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
        let prior_modules = dispatched.modules.previous_modules_metadata.as_ref();
        MaterializationInputs {
            install: self.install,
            effective_node_version: self.mode.effective_node_version.take(),
            tarball_mem_cache: Arc::clone(&self.owned.tarball_mem_cache),
            http_client_arc: Arc::clone(&self.owned.http_client_arc),
            take_frozen_path: dispatched.take_frozen_path,
            dependency_groups: std::mem::take(&mut self.owned.dependency_groups),
            project_manifests,
            lockfile_specifier_project_manifests: self
                .options
                .lockfile_specifier_project_manifests
                .take(),
            workspace_projects: workspace_projects(
                self.loaded_workspace_projects,
                self.options.selection.as_ref(),
            ),
            requested_importer_ids: scope.importers.requested_importer_ids.as_ref(),
            real_importer_ids: &scope.importers.real_importer_ids,
            workspace_root: &self.workspace.workspace_root,
            included: self.mode.included,
            rebuild: self.options.rebuild.as_ref(),
            supported_architectures: self.owned.supported_architectures.as_ref(),
            early_host_detection,
            modules_manifest: dispatched.modules.old_modules.as_ref(),
            prior_hoisted_dependencies: prior_hoisted_dependencies(prior_modules),
            prior_hoisted_locations: prior_hoisted_locations(prior_modules),
            prune_orphans: !scope.importers.filtered_install,
            logged_methods,
            resolve_only: self.mode.resolve_only,
            can_prompt: self.mode.can_prompt,
            save_lockfile: self.options.save_lockfile,
            catalogs: &self.workspace.catalogs,
            prefix: &self.workspace.prefix,
            resolution,
            lockfiles,
        }
    }

    fn materialization_resolution<'r>(
        &mut self,
        loaded: &mut Loaded<'r>,
    ) -> super::super::materialize::MaterializationResolution<'a> {
        super::super::materialize::MaterializationResolution {
            update_seed_policy: std::mem::replace(
                &mut self.owned.update_seed_policy,
                UpdateSeedPolicy::KeepAll,
            ),
            preferred_versions_override: self.owned.preferred_versions_override.take(),
            auth_override: self.owned.auth_override.take(),
            resolution_observer: self.owned.resolution_observer.take(),
            peer_issues_sink: self.owned.peer_issues_sink.take(),
            deps_requiring_build_sink: self.owned.deps_requiring_build_sink.take(),
            pnpmfile_hook: loaded.pnpmfile_hook.take(),
            deploy_manifest_hook: self.options.deploy_manifest_hook,
            manifest_spec_bumps: self.options.manifest_spec_bumps,
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
            materialized,
            resolve_only: self.mode.resolve_only,
            dry_run: self.install.dry_run,
            peer_issues_sink_is_none: self.mode.peer_issues_sink_is_none,
            existing_wanted_lockfile: lockfiles.wanted.loaded,
            lockfile: lockfiles.wanted.get(),
            included: self.mode.included,
            node_linker: self.install.node_linker,
            current_lockfile: loaded.current.take(),
            project_manifests,
            filtered_install: scope.importers.filtered_install,
            is_inconsistent: dispatched.modules.is_inconsistent,
            previous_modules_metadata: dispatched.modules.previous_modules_metadata,
            config: self.install.config,
            modules_manifest: dispatched.modules.old_modules,
            rebuild: self.options.rebuild.take(),
            take_frozen_path: dispatched.take_frozen_path,
            lockfile_synthesized_from_current: lockfiles.wanted.synthesized_from_current(),
            lockfile_was_fast_updated: lockfiles.wanted.was_fast_updated(),
            save_lockfile: self.options.save_lockfile,
            mutation: self.install.mutation,
            manifest_dir: self.workspace.manifest_dir,
            selection: self.options.selection.take(),
            supported_architectures: self.owned.supported_architectures.take(),
            catalog_context_present: self.workspace.catalog_context_present,
            verified_file_integrity_baseline: self.mode.verified_file_integrity_baseline,
            prefix: std::mem::take(&mut self.workspace.prefix),
            requested_importer_ids: scope.importers.requested_importer_ids.as_ref(),
            workspace_root: std::mem::take(&mut self.workspace.workspace_root),
            workspace_manifest_dir: std::mem::take(&mut self.workspace.workspace_manifest_dir),
            real_importer_ids: &scope.importers.real_importer_ids,
            catalogs: std::mem::take(&mut self.workspace.catalogs),
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
        wanted_shared: lockfiles.wanted.loader_handle(loaded.shared.take()),
        merge_wanted: loaded.merge_wanted_lockfile,
        current: loaded.current.as_ref(),
        verification,
        verification_override: dispatched.modules.lockfile_verification_override.take(),
    }
}
