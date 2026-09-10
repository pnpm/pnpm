use super::{
    ApplyMaterializationInputs, Arc, AtomicU8, ContextLog, DependencyGroup,
    FastUpdateLockfileOptions, FreshnessCheckError, FreshnessScope, HashSet, Host,
    InMemoryPackageMetaCache, IncludedDependencies, Install, InstallError, InstallRunOptions,
    IsTerminal, Lockfile, LogEvent, LogLevel, MaterializationInputs, OptimisticRepeatInstallCheck,
    OptimisticRepeatInstallDecision, PackageManifest, Path, PathBuf, PnpmLog,
    PrepareModulesStateInputs, PreparedModulesState, Reporter, ScopeLog, Stage, StageLog,
    SummaryLog, UpdateSeedPolicy, apply_materialization_result, build_project_manifests_list,
    build_resolution_verifiers, build_root_importer_project_manifests_list,
    build_selected_project_manifests_list, check_lockfile_freshness,
    check_optimistic_repeat_install, configured_or_discovered_workspace_dir,
    dev_preinstall_already_ran, emit_initial_package_manifest,
    get_catalogs_from_workspace_manifest, gvs_build_marker_present,
    gvs_build_markers_may_require_recovery, load_workspace_projects, lockfile_root_dir,
    map_frozen_lockfile_error, materialize, prepare_modules_state, prior_hoisted_dependencies,
    prune_merged_branch_lockfile, run_dev_preinstall, selected_manifest_freshness_inputs,
    try_fast_update_lockfile, unapproved_recorded_ignored_builds, verify_lockfile_eagerly,
};
use pnpm_config::Config;
use pnpm_executor::DEV_PREINSTALL_STAGE;
use pnpm_store_dir::VerifiedFileIntegrity;

use crate::{ProjectMutation, catalog_cleanup::post_install_prune};

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Runs the install, then the passes over what it wrote: deleting the
    /// per-branch lockfiles it has just folded into the wanted lockfile,
    /// and pruning the `pnpm-workspace.yaml` exclude entries that lockfile
    /// no longer resolves.
    ///
    /// Both live out here because every success path of
    /// [`Self::run_inner_impl`] — including the short-circuits that do
    /// nothing but rewrite the lockfile — has to run them.
    pub(super) async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
    ) -> Result<(), InstallError> {
        // The branch lockfiles become disposable only once the merge has
        // been written for good. An install that neither reads nor saves a
        // lockfile never merged them, and one that only reports what it
        // would do has its lockfile taken back afterwards — deleting them
        // in either case drops resolutions no file is left holding.
        let merge_will_be_saved = self.config.merge_git_branch_lockfiles
            && self.config.lockfile
            && options.save_lockfile
            && !options.lockfile_check
            && !self.dry_run;
        let branch_lockfiles_to_clean = merge_will_be_saved
            .then(|| {
                let manifest_dir =
                    self.manifest.path().parent().expect("manifest path always has a parent dir");
                lockfile_root_dir(self.config, manifest_dir).map_err(InstallError::FindWorkspaceDir)
            })
            .transpose()?;
        let prune_excludes = self.prunes_workspace_excludes(&options);
        let (config, manifest) = (self.config, self.manifest);
        let outcome = Box::pin(self.run_inner_impl::<Reporter>(options)).await?;
        if let Some(lockfile_dir) = branch_lockfiles_to_clean {
            Lockfile::clean_git_branch_lockfiles(&lockfile_dir)
                .map_err(InstallError::CleanGitBranchLockfiles)?;
        }
        if prune_excludes
            && let InstallRunOutcome::LockfileSettled { workspace_manifest_dir } = outcome
        {
            post_install_prune(config, Some(&workspace_manifest_dir), manifest)
                .map_err(InstallError::WriteWorkspaceManifest)?;
        }
        Ok(())
    }

    /// Whether this run owes the [`post_install_prune`] pass over the
    /// `minimumReleaseAgeExclude` / `trustPolicyExclude` lists. `add`,
    /// `update` and `remove` run it themselves once their manifest edits
    /// are persisted; the whole-workspace commands (`install`, `dedupe`)
    /// have only this run to do it. It reads the lockfile back from disk,
    /// so the gates of the branch-lockfile cleanup apply: a run that
    /// leaves the lockfile untouched, has it restored afterwards, or only
    /// reports gives it nothing new to see.
    fn prunes_workspace_excludes(&self, options: &InstallRunOptions<'_, '_>) -> bool {
        self.persist_policy_excludes
            && matches!(self.mutation, ProjectMutation::InstallWorkspace)
            && self.config.lockfile
            && options.save_lockfile
            && !options.lockfile_check
            && !self.dry_run
            && (self.config.minimum_release_age_exclude_prune
                || self.config.trust_policy_exclude_prune)
    }

    /// Separate what every phase reads from what one of them consumes.
    fn split(self) -> (InstallView<'a>, InstallOwned) {
        (
            InstallView {
                resolved_packages: self.resolved_packages,
                http_client: self.http_client,
                config: self.config,
                manifest: self.manifest,
                emit_initial_manifest: self.emit_initial_manifest,
                lockfile: self.lockfile,
                lockfile_path: self.lockfile_path,
                frozen_lockfile: self.frozen_lockfile,
                prefer_frozen_lockfile: self.prefer_frozen_lockfile,
                ignore_manifest_check: self.ignore_manifest_check,
                skip_runtimes: self.skip_runtimes,
                trust_lockfile: self.trust_lockfile,
                update_checksums: self.update_checksums,
                mutation: self.mutation,
                installs_only: self.installs_only,
                node_linker: self.node_linker,
                lockfile_only: self.lockfile_only,
                dry_run: self.dry_run,
                persist_policy_excludes: self.persist_policy_excludes,
                disable_optimistic_repeat_install: self.disable_optimistic_repeat_install,
            },
            InstallOwned {
                tarball_mem_cache: self.tarball_mem_cache,
                http_client_arc: self.http_client_arc,
                dependency_groups: self.dependency_groups.into_iter().collect(),
                supported_architectures: self.supported_architectures,
                update_seed_policy: self.update_seed_policy,
                preferred_versions_override: self.preferred_versions_override,
                auth_override: self.auth_override,
                resolution_observer: self.resolution_observer,
                peer_issues_sink: self.peer_issues_sink,
                deps_requiring_build_sink: self.deps_requiring_build_sink,
                catalogs_override: self.catalogs_override,
                pnpmfile_hook_override: self.pnpmfile_hook_override,
                workspace_projects_override: self.workspace_projects_override,
            },
        )
    }

    async fn run_inner_impl<Reporter: self::Reporter + 'static>(
        self,
        mut options: InstallRunOptions<'a, '_>,
    ) -> Result<InstallRunOutcome, InstallError> {
        let (install, mut owned) = self.split();
        install.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        owned.http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let mode = RunMode::settle(install, &owned, &options)?;
        let workspace = InstallWorkspace::discover::<Reporter>(install, &mut owned, &options)?;
        let scope = InstallScope::select(
            install,
            &workspace.workspace_root,
            workspace_projects(
                workspace.loaded_workspace_projects.as_deref(),
                options.selection.as_ref(),
            ),
            workspace.workspace_projects_are_overridden,
            &options,
        );
        if scope.is_already_up_to_date::<Reporter>(install, &owned, &mode, &workspace)? {
            Reporter::emit(&LogEvent::Summary(SummaryLog {
                level: LogLevel::Debug,
                prefix: workspace.prefix,
            }));
            return Ok(InstallRunOutcome::AlreadyUpToDate);
        }
        let loaded = load_lockfiles::<Reporter>(
            install,
            &mut owned,
            &mode,
            &workspace,
            &scope,
            options.selection.as_ref(),
            &options.read_package_hooked_manifest_paths,
        )
        .await?;
        let project_manifests = loaded.manifests.view(&scope.project_manifests);
        let lockfiles = settle_wanted_lockfile::<Reporter>(
            install,
            &mode,
            &workspace,
            &scope,
            &loaded,
            &project_manifests,
            options.selection.as_ref(),
        )
        .await?;
        let verification = Verification::set_up(
            install,
            &owned,
            lockfiles.wanted.get().is_some(),
            &workspace.workspace_root,
        )?;
        let Some(dispatched) = dispatch::<Reporter>(
            Settled {
                install,
                owned: &owned,
                mode: &mode,
                workspace: &workspace,
                scope: &scope,
                loaded: &loaded,
                project_manifests: &project_manifests,
                lockfiles: &lockfiles,
                verification: &verification,
            },
            &mut options,
        )
        .await?
        else {
            return Ok(InstallRunOutcome::LockfileSettled {
                workspace_manifest_dir: workspace.workspace_manifest_dir,
            });
        };

        let materialized = materialize::<Reporter>(MaterializationInputs {
            install,
            effective_node_version: mode.effective_node_version,
            tarball_mem_cache: owned.tarball_mem_cache,
            http_client_arc: owned.http_client_arc,
            lockfile: lockfiles.wanted.get(),
            lockfile_shared: lockfiles.wanted.loader_handle(loaded.shared),
            merge_wanted_lockfile: loaded.merge_wanted_lockfile,
            take_frozen_path: dispatched.take_frozen_path,
            lockfile_verification_override: dispatched.modules.lockfile_verification_override,
            resolution_verifiers: verification.resolution_verifiers,
            derived_lockfile_path: verification.derived_lockfile_path,
            dependency_groups: owned.dependency_groups,
            project_manifests: &project_manifests,
            lockfile_specifier_project_manifests: options.lockfile_specifier_project_manifests,
            workspace_projects: workspace_projects(
                workspace.loaded_workspace_projects.as_deref(),
                options.selection.as_ref(),
            ),
            requested_importer_ids: scope.importers.requested_importer_ids.as_ref(),
            real_importer_ids: &scope.importers.real_importer_ids,
            workspace_root: &workspace.workspace_root,
            included: mode.included,
            rebuild: options.rebuild.as_ref(),
            current_lockfile: loaded.current.as_ref(),
            supported_architectures: owned.supported_architectures.as_ref(),
            early_host_detection: loaded.early_host_detection,
            modules_manifest: dispatched.modules.old_modules.as_ref(),
            prior_hoisted_dependencies: prior_hoisted_dependencies(
                dispatched.modules.previous_modules_metadata.as_ref(),
            ),
            planned_canonical_fetches: verification.planned_canonical_fetches,
            prune_orphans: !scope.importers.filtered_install,
            logged_methods: &AtomicU8::new(0),
            meta_cache: verification.meta_cache,
            resolve_only: mode.resolve_only,
            can_prompt: mode.can_prompt,
            update_seed_policy: owned.update_seed_policy,
            preferred_versions_override: owned.preferred_versions_override,
            auth_override: owned.auth_override,
            resolution_observer: owned.resolution_observer,
            peer_issues_sink: owned.peer_issues_sink,
            deps_requiring_build_sink: owned.deps_requiring_build_sink,
            pnpmfile_hook: loaded.pnpmfile_hook,
            deploy_manifest_hook: options.deploy_manifest_hook,
            save_lockfile: options.save_lockfile,
            manifest_spec_bumps: options.manifest_spec_bumps,
            catalogs: &workspace.catalogs,
            prefix: &workspace.prefix,
        })
        .await?;

        let workspace_manifest_dir = workspace.workspace_manifest_dir.clone();
        apply_materialization_result::<Reporter>(ApplyMaterializationInputs {
            materialized: materialized.materialized,
            resolve_only: mode.resolve_only,
            dry_run: install.dry_run,
            peer_issues_sink_is_none: mode.peer_issues_sink_is_none,
            existing_wanted_lockfile: lockfiles.wanted.loaded,
            lockfile: lockfiles.wanted.get(),
            included: mode.included,
            node_linker: install.node_linker,
            current_lockfile: loaded.current,
            project_manifests: &project_manifests,
            filtered_install: scope.importers.filtered_install,
            is_inconsistent: dispatched.modules.is_inconsistent,
            previous_modules_metadata: dispatched.modules.previous_modules_metadata,
            config: install.config,
            modules_manifest: dispatched.modules.old_modules,
            rebuild: options.rebuild,
            take_frozen_path: dispatched.take_frozen_path,
            lockfile_synthesized_from_current: lockfiles.wanted.synthesized_from_current(),
            lockfile_was_fast_updated: lockfiles.wanted.was_fast_updated(),
            save_lockfile: options.save_lockfile,
            mutation: install.mutation,
            manifest_dir: workspace.manifest_dir,
            selection: options.selection,
            supported_architectures: owned.supported_architectures,
            catalog_context_present: workspace.catalog_context_present,
            verified_file_integrity_baseline: mode.verified_file_integrity_baseline,
            prefix: workspace.prefix,
            requested_importer_ids: scope.importers.requested_importer_ids,
            workspace_root: workspace.workspace_root,
            workspace_manifest_dir: workspace.workspace_manifest_dir,
            real_importer_ids: scope.importers.real_importer_ids,
            catalogs: workspace.catalogs,
        })
        .await?;

        // Only now wait out the store-index writer's teardown — its
        // final flush and the WAL checkpoint `SQLite` runs when the
        // connection closes (~40 ms of otherwise pure tail on a cold
        // install) have been overlapping every write above since the
        // install paths dropped their writer handles. An error path
        // that returned before this point dropped the handle instead,
        // detaching the task: an interrupted checkpoint is exactly the
        // crash case WAL recovery exists for.
        pnpm_store_dir::StoreIndexWriter::drain(
            materialized.store_index_teardown,
            "; some rows may not be persisted",
        )
        .await;
        Ok(InstallRunOutcome::LockfileSettled { workspace_manifest_dir })
    }
}

/// How far [`Install::run_inner_impl`] got, for the passes
/// [`Install::run_inner`] runs over what it wrote.
enum InstallRunOutcome {
    /// The repeat-install fast path found nothing to do; no file changed.
    AlreadyUpToDate,
    /// The run settled the wanted lockfile, rewriting it wherever it had
    /// drifted, so the exclude lists in `pnpm-workspace.yaml` under
    /// `workspace_manifest_dir` may now name versions nothing resolves.
    LockfileSettled { workspace_manifest_dir: PathBuf },
}

/// The install's borrowed and `Copy` inputs, as one value every phase reads.
#[derive(Clone, Copy)]
pub(super) struct InstallView<'a> {
    pub(super) resolved_packages: &'a super::ResolvedPackages,
    pub(super) http_client: &'a super::ThrottledClient,
    pub(super) config: &'static Config,
    pub(super) manifest: &'a PackageManifest,
    pub(super) emit_initial_manifest: bool,
    pub(super) lockfile: super::MaybeLazyLockfile<'a>,
    pub(super) lockfile_path: Option<&'a Path>,
    pub(super) frozen_lockfile: bool,
    pub(super) prefer_frozen_lockfile: Option<bool>,
    pub(super) ignore_manifest_check: bool,
    pub(super) skip_runtimes: bool,
    pub(super) trust_lockfile: bool,
    pub(super) update_checksums: bool,
    pub(super) mutation: crate::ProjectMutation,
    pub(super) installs_only: bool,
    pub(super) node_linker: super::NodeLinker,
    pub(super) lockfile_only: bool,
    pub(super) dry_run: bool,
    pub(super) persist_policy_excludes: bool,
    pub(super) disable_optimistic_repeat_install: bool,
}

/// The install's owned inputs, each consumed by one phase.
struct InstallOwned {
    tarball_mem_cache: Arc<super::MemCache>,
    http_client_arc: Arc<super::ThrottledClient>,
    dependency_groups: Vec<DependencyGroup>,
    supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    update_seed_policy: UpdateSeedPolicy,
    preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    auth_override: Option<Arc<super::AuthHeaders>>,
    resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    peer_issues_sink: Option<crate::PeerIssuesSink>,
    deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    catalogs_override: Option<super::Catalogs>,
    pnpmfile_hook_override: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    workspace_projects_override: Option<Vec<pnpm_workspace::Project>>,
}

/// What the run's flags settle into before anything is read from disk.
struct RunMode {
    lockfile_only: bool,
    resolve_only: bool,
    prefer_frozen_lockfile: bool,
    included: IncludedDependencies,
    can_prompt: bool,
    peer_issues_sink_is_none: bool,
    effective_node_version: Option<String>,
    verified_file_integrity_baseline: VerifiedFileIntegrity,
}

impl RunMode {
    fn settle(
        install: InstallView<'_>,
        owned: &InstallOwned,
        options: &InstallRunOptions<'_, '_>,
    ) -> Result<Self, InstallError> {
        // Taken before any fetching so the store-verification figures
        // this install reports are its own — a recursive workspace run
        // and a long-lived embedder (the NAPI addon) both drive several
        // installs through the same process-global tally.
        let verified_file_integrity_baseline = VerifiedFileIntegrity::snapshot();
        // `--lockfile-only` with `lockfile: false` (pnpm's
        // `useLockfile: false`) is a config conflict: the only output the
        // flag produces is the lockfile, and that write is disabled.
        // Fail fast rather than run a resolve that writes nothing.
        reject_lockfile_only_without_lockfile(install.config, install.lockfile_only)?;
        // `enableModulesDir: false` (with the global virtual store off) is
        // "resolve and write the lockfile, materialize nothing" — the same
        // pipeline `--lockfile-only` takes, entered from config. It stays
        // outside the `lockfile: false` conflict above (pnpm accepts that
        // combination and simply writes nothing), and never turns a
        // rebuild — which runs against an already-materialized
        // `node_modules` — into a silent no-op.
        let lockfile_only = effective_lockfile_only(
            install.config,
            install.lockfile_only,
            options.rebuild.as_ref(),
        );
        reject_conflicting_store_config(install.config)?;
        Ok(Self {
            lockfile_only,
            // `--dry-run` resolves but never materializes, so it borrows the
            // lockfile-only plumbing (skip node_modules / `.modules.yaml` /
            // workspace-state) while additionally skipping the lockfile write.
            // Both lockfile-only paths must stop after writing the wanted lockfile:
            // neither may write `.modules.yaml`, the current lockfile, or workspace state.
            // The frozen path returns below; the fresh path returns in `complete_resolve_only`.
            resolve_only: lockfile_only || install.dry_run,
            prefer_frozen_lockfile: install
                .prefer_frozen_lockfile
                .unwrap_or(install.config.prefer_frozen_lockfile),
            // The same set the dependency-graph walker observes, written to
            // `.modules.yaml` as `included`.
            included: super::included_dependencies(&owned.dependency_groups),
            can_prompt: options.prompt_eligibility_override.unwrap_or_else(prompts_are_answerable),
            peer_issues_sink_is_none: owned.peer_issues_sink.is_none(),
            effective_node_version: super::effective_node_version(install.config, install.manifest),
            verified_file_integrity_baseline,
        })
    }
}

/// The directories the run anchors on.
struct WorkspaceDirs<'a> {
    manifest_dir: &'a Path,
    workspace_dir: Option<PathBuf>,
    workspace_manifest_dir: PathBuf,
    workspace_root: PathBuf,
}

impl<'a> WorkspaceDirs<'a> {
    // Project root for the [bunyan]-envelope `prefix`. This is
    // emitted as `lockfileDir`, the directory containing
    // `pnpm-lock.yaml`. With workspace support that equals the
    // workspace root — pacquet finds it via [`find_workspace_dir`].
    // Falls back to the manifest's parent dir when no
    // `pnpm-workspace.yaml` exists in any ancestor (the
    // single-project case). Closes pnpm/pacquet#357.
    //
    // [bunyan]: <https://github.com/trentm/node-bunyan>
    fn find(install: InstallView<'a>) -> Result<Self, InstallError> {
        let manifest_dir =
            install.manifest.path().parent().expect("manifest path always has a parent dir");
        let workspace_dir = configured_or_discovered_workspace_dir(install.config, manifest_dir)
            .map_err(InstallError::FindWorkspaceDir)?;
        Ok(Self {
            manifest_dir,
            workspace_manifest_dir: workspace_dir
                .clone()
                .unwrap_or_else(|| manifest_dir.to_path_buf()),
            // Catalogs and workspace packages still come from the real
            // workspace dir, which `lockfile_root_dir` parts ways with
            // under `sharedWorkspaceLockfile: false`.
            workspace_root: lockfile_root_dir(install.config, manifest_dir)
                .map_err(InstallError::FindWorkspaceDir)?,
            workspace_dir,
        })
    }
}

/// The workspace the run installs into: the directories it anchors on,
/// its catalogs and its projects.
struct InstallWorkspace<'a> {
    manifest_dir: &'a Path,
    workspace_dir: Option<PathBuf>,
    workspace_manifest_dir: PathBuf,
    workspace_root: PathBuf,
    workspace_manifest: Option<pnpm_workspace::WorkspaceManifest>,
    catalog_context_present: bool,
    catalogs: super::Catalogs,
    prefix: String,
    workspace_projects_are_overridden: bool,
    loaded_workspace_projects: Option<Vec<pnpm_workspace::Project>>,
}

/// The projects the run installs, and how a selection narrows them.
struct InstallScope<'w> {
    project_manifests: Vec<(PathBuf, &'w PackageManifest)>,
    importers: ImporterSelection,
    prune_stale_importers: bool,
}

/// The importers a selection narrows the run to.
struct ImporterSelection {
    real_importer_ids: HashSet<String>,
    filtered_install: bool,
    requested_importer_ids: Option<HashSet<String>>,
}

impl ImporterSelection {
    fn select(
        selection: Option<&crate::WorkspaceInstallSelection<'_>>,
        workspace_root: &Path,
        project_manifests: &[(PathBuf, &PackageManifest)],
    ) -> Self {
        let real_importer_ids = importer_ids(
            workspace_root,
            project_manifests.iter().map(|(project_dir, _)| project_dir.as_path()),
        );
        let filtered_install = selection.is_some_and(|selection| {
            importer_ids(workspace_root, selection.selected_dirs.iter().map(PathBuf::as_path))
                != real_importer_ids
        });
        Self {
            requested_importer_ids: filtered_install.then_some(selection).flatten().map(
                |selection| {
                    importer_ids(
                        workspace_root,
                        selection.install_dirs.iter().map(PathBuf::as_path),
                    )
                },
            ),
            real_importer_ids,
            filtered_install,
        }
    }
}

fn importer_ids<'d>(
    workspace_root: &Path,
    dirs: impl Iterator<Item = &'d Path>,
) -> HashSet<String> {
    dirs.map(|project_dir| pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        .collect()
}

impl<'a> InstallWorkspace<'a> {
    /// Consumes the catalogs and workspace-projects overrides off `owned`.
    fn discover<Reporter: self::Reporter>(
        install: InstallView<'a>,
        owned: &mut InstallOwned,
        options: &InstallRunOptions<'_, '_>,
    ) -> Result<Self, InstallError> {
        let dirs = WorkspaceDirs::find(install)?;
        let workspace_manifest = dirs
            .workspace_dir
            .as_deref()
            .map(pnpm_workspace::read_workspace_manifest)
            .transpose()
            .map_err(InstallError::ReadWorkspaceManifest)?
            .flatten();
        let catalog_context_present = catalog_context_present(
            install.config,
            owned.catalogs_override.as_ref(),
            dirs.workspace_dir.as_ref(),
        );
        // Prefer a caller-supplied in-memory catalogs set
        // (`catalogs_override`, e.g. `pacquet update --latest --no-save`
        // resolving a bumped `catalog:` entry that is not written to disk),
        // then catalogs an `updateConfig` pnpmfile hook produced
        // (`config.catalogs`, the complete set after the hook pass), and
        // finally the raw workspace-manifest read. `None` at every layer
        // falls back to the manifest, mirroring pnpm's post-`updateConfig`
        // `config.catalogs`.
        let catalogs =
            match owned.catalogs_override.take().or_else(|| install.config.catalogs.clone()) {
                Some(catalogs) => catalogs,
                None => get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
                    .map_err(InstallError::InvalidCatalogsConfiguration)?,
            };
        // Walk every workspace project's `package.json` once. The
        // resulting `Vec` feeds both the up-to-date short-circuit
        // below and the fresh-install path's `workspace:`-spec lookup
        // / per-importer manifest list further down. `None` when no
        // `pnpm-workspace.yaml` exists in or above `workspace_root` —
        // single-project installs only have the root manifest, which
        // the short-circuit and the install paths both reach via
        // `manifest` directly.
        //
        // An embedder that supplies its importers in memory
        // (`workspace_projects_override`) bypasses the on-disk walk
        // entirely; the override's `Vec` is used verbatim.
        let workspace_projects_are_overridden = owned.workspace_projects_override.is_some();
        let loaded_workspace_projects = discovered_workspace_projects(
            options.selection.is_some(),
            owned.workspace_projects_override.take(),
            dirs.workspace_dir.as_deref().unwrap_or(&dirs.workspace_root),
            workspace_manifest.as_ref(),
        )?;
        let workspace_projects = options.selection.as_ref().map_or_else(
            || loaded_workspace_projects.as_deref(),
            |selection| Some(selection.all_projects),
        );
        // Report what this run covers. A narrowed one already reported its
        // own scope where the `--filter` was resolved, and so did the
        // dedicated-lockfile plan that installs each selected project
        // separately — those child installs must not report over the top
        // of it.
        //
        // A full install (pnpm's `mutation: "install"`) is the workspace-wide
        // one and counts every project; a partial one (`add`, `update`,
        // `remove`, ...) targets the project it was run in and reports the
        // single-project shape, with no `total`, exactly as pnpm's
        // non-recursive `scopeLogger` call does.
        if options.selection.is_none() {
            emit_scope_log::<Reporter>(
                install.config,
                install.mutation,
                workspace_projects,
                dirs.workspace_dir.as_deref(),
            );
        }
        Ok(Self {
            // Use `to_string_lossy` rather than `to_str().expect(...)` so a
            // valid filesystem path with non-UTF-8 bytes (possible on Unix)
            // doesn't panic the installer. `prefix` is used only for
            // reporter envelopes, so a lossy conversion is acceptable —
            // the rest of the install path uses the same pattern for
            // paths threaded into log events.
            prefix: dirs.workspace_root.to_string_lossy().into_owned(),
            manifest_dir: dirs.manifest_dir,
            workspace_dir: dirs.workspace_dir,
            workspace_manifest_dir: dirs.workspace_manifest_dir,
            workspace_root: dirs.workspace_root,
            workspace_manifest,
            catalog_context_present,
            catalogs,
            workspace_projects_are_overridden,
            loaded_workspace_projects,
        })
    }
}

/// The projects the run sees: the selection's when one narrows the run,
/// else what the workspace walk loaded.
fn workspace_projects<'s>(
    loaded: Option<&'s [pnpm_workspace::Project]>,
    selection: Option<&crate::WorkspaceInstallSelection<'s>>,
) -> Option<&'s [pnpm_workspace::Project]> {
    selection.map_or(loaded, |selection| Some(selection.all_projects))
}

impl<'w> InstallScope<'w> {
    fn select(
        install: InstallView<'w>,
        workspace_root: &Path,
        workspace_projects: Option<&'w [pnpm_workspace::Project]>,
        workspace_projects_are_overridden: bool,
        options: &InstallRunOptions<'w, 'w>,
    ) -> Self {
        let project_manifests = install_project_manifests(&ProjectManifestScope {
            manifest: install.manifest,
            selection: options.selection.as_ref(),
            workspace_root,
            workspace_projects,
            root_manifest_as_workspace_root: options.root_manifest_as_workspace_root,
            workspace_projects_are_overridden,
            config: install.config,
        });
        let importers = ImporterSelection::select(
            options.selection.as_ref(),
            workspace_root,
            &project_manifests,
        );
        // Only an install that covers a whole workspace sees the complete
        // project list, so only it may conclude that an importer the
        // lockfile records belongs to a project that is gone. This is
        // pnpm's `pruneLockfileImporters`, which its recursive install
        // defaults to the same condition (`pkgs.length ===
        // allProjects.length`) — outside a workspace there is no project
        // list to compare against.
        // A `NodeApiProject[]` handed in by an API consumer carries no
        // promise of listing every workspace project, so it cannot stand
        // in for the project list either.
        let prune_stale_importers = may_prune_stale_importers(&StaleImporterPrune {
            filtered_install: importers.filtered_install,
            mutation: install.mutation,
            workspace_projects,
            workspace_projects_are_overridden,
            config: install.config,
        });
        Self { project_manifests, importers, prune_stale_importers }
    }

    // Optimistic repeat-install short-circuit. When nothing has
    // changed since the previous successful install (settings,
    // workspace structure, manifest mtimes), skip the entire
    // install pipeline and emit pnpm's "Already up to date" log.
    // The fast path runs before any of the install setup (no
    // lockfile reads, no verifier fan-out, no `getContext`).
    //
    // Disabled when `--frozen-lockfile` is requested: an explicit
    // headless install should always go through the dispatch so a
    // `NoLockfile` or `OutdatedLockfile` error still fires when
    // the lockfile is missing or stale.

    // Only a full `pacquet install` may short-circuit. `add` and
    // `remove` mutate the manifest in memory and persist it after
    // this run returns, so the on-disk mtimes the check reads still
    // describe the pre-mutation project — without this gate a fresh
    // workspace state would read as "nothing changed → already up
    // to date" and the mutation would never be resolved or
    // materialized. `pacquet update` is
    // excluded through its seed policy: a compatible bump leaves
    // the manifest byte-identical, which the check would likewise
    // read as up to date and skip the registry re-resolution.
    //
    // A `--filter` narrowing does not disqualify the run: the check
    // validates the whole workspace (`project_manifests` covers every
    // project even when only a subset is selected), and it refuses a
    // workspace state a filtered install wrote, so "nothing changed"
    // still means every selected project is materialized.
    fn is_already_up_to_date<Reporter: self::Reporter>(
        &self,
        install: InstallView<'_>,
        owned: &InstallOwned,
        mode: &RunMode,
        workspace: &InstallWorkspace<'_>,
    ) -> Result<bool, InstallError> {
        install_is_already_up_to_date::<Reporter>(&UpToDateCheck {
            config: install.config,
            workspace_root: &workspace.workspace_root,
            node_linker: install.node_linker,
            included: mode.included,
            supported_architectures: owned.supported_architectures.as_ref(),
            project_manifests: &self.project_manifests,
            is_workspace_install: workspace.workspace_manifest.is_some(),
            lockfile: install.lockfile,
            catalogs: &workspace.catalogs,
            mutation: install.mutation,
            update_seed_policy: &owned.update_seed_policy,
            frozen_lockfile: install.frozen_lockfile,
            disable_optimistic_repeat_install: install.disable_optimistic_repeat_install,
            effective_node_version: mode.effective_node_version.as_deref(),
            prefix: &workspace.prefix,
        })
    }
}

/// The lockfiles as loaded, and what the wanted one's load let start: the
/// host probe, the pnpmfile, and the project manifests as its hooks
/// rewrote them.
struct Loaded<'a> {
    lockfile: Option<&'a Lockfile>,
    shared: Option<Arc<Lockfile>>,
    merge_wanted_lockfile: Option<&'a Lockfile>,
    pre_merge_importers:
        Option<&'a std::collections::HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
    current: Option<Lockfile>,
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    manifests: HookedManifests,
}

/// Consumes the pnpmfile override off `owned`.
async fn load_lockfiles<'a, Reporter: self::Reporter + 'static>(
    install: InstallView<'a>,
    owned: &mut InstallOwned,
    mode: &RunMode,
    workspace: &InstallWorkspace<'a>,
    scope: &InstallScope<'a>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    pre_hooked_paths: &HashSet<PathBuf>,
) -> Result<Loaded<'a>, InstallError> {
    // Past the fast path every install flavor reads the wanted
    // lockfile; start its read + parse on a background thread so it
    // overlaps the cycle check below. The forced load further down
    // joins it. (A run the pipeline knew would get here — frozen /
    // forced — started this prefetch before project discovery, and
    // this call is then a no-op.)
    install.lockfile.prefetch();
    // Report the projects this install covers depending on each
    // other in a cycle — after the short-circuit above, because pnpm
    // returns from "Already up to date" before reaching its own
    // check, and before any resolution, because a
    // `disallowWorkspaceCycles` failure must not be paid for.
    report_install_scope_cycles::<Reporter>(
        install.config,
        workspace,
        selection,
        (
            install.mutation,
            workspace_projects(workspace.loaded_workspace_projects.as_deref(), selection),
        ),
    )?;
    let current_lockfile_task = spawn_current_lockfile_load(install.config);
    // Past the repeat-install fast path every install flavor needs
    // the wanted lockfile's contents; force the deferred load here.
    // A broken lockfile is regenerable state, so only a frozen
    // install treats it as fatal (upstream `readLockfiles`).
    let phase_start = std::time::Instant::now();
    let wanted = load_wanted_lockfile::<Reporter>(
        install.lockfile,
        install.frozen_lockfile,
        (&workspace.workspace_root, &workspace.prefix),
    )?;
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "load_wanted_lockfile",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    // Spawn the installability host detection (`node --version`,
    // ~150 ms of node startup) as soon as the wanted lockfile is
    // parsed, so the probe overlaps planning on the frozen path and
    // the whole resolution on the fresh path.
    let early_host_detection =
        needs_early_host_detection(install.config, mode.resolve_only, wanted.lockfile).then(|| {
            pnpm_deps_restorer::materialization_plan::HostDetection::spawn(
                install.config.engine_strict,
                mode.effective_node_version.clone(),
                owned.supported_architectures.clone(),
            )
        });
    // Register the project against the shared store for prune
    // tracking, once per install at the workspace root. Register
    // the workspace root once, not per importer — store prune walks
    // the workspace's `node_modules/.pnpm/` to find every installed
    // package, so one registry entry per workspace is enough.
    //
    // Gated on `enable_global_virtual_store` because pacquet wires
    // the prune-by-registry path only under GVS for now; pnpm
    // registers unconditionally, so once the non-GVS prune path
    // lands the gate should be dropped. Best-effort: a registry
    // write failure shouldn't fail the install. Surface as
    // `tracing::warn!` so the failure is diagnosable but the
    // install carries on.
    register_workspace_in_store(install.config, &workspace.workspace_root);
    // `pnpm:package-manifest initial` carries the on-disk
    // `package.json` body for this importer. Fires before
    // `pnpm:context` so consumers that key off manifest contents
    // have it ready when the install header renders.
    if install.emit_initial_manifest {
        emit_initial_package_manifest::<Reporter>(install.manifest);
    }
    // The pnpmfile whose checksum the freshness gates compare
    // against a lockfile's `pnpmfileChecksum`, resolved the way the
    // install that records one resolves it. Building the handle
    // costs a `stat`. The Node worker only starts if a gate has to
    // ask whether the pnpmfile exports hooks. The handle is handed to
    // the resolve path below so an install spawns at most one.
    let pnpmfile_hook = resolve_pnpmfile_hook(
        install.config,
        &workspace.workspace_root,
        owned.pnpmfile_hook_override.take(),
    )?;
    let manifests = HookedManifests::hook::<Reporter>(
        install.config,
        &workspace.workspace_root,
        &scope.project_manifests,
        pnpmfile_hook.as_ref(),
        pre_hooked_paths,
    )
    .await?;
    Ok(Loaded {
        lockfile: wanted.lockfile,
        shared: wanted.shared,
        merge_wanted_lockfile: wanted.merge,
        pre_merge_importers: wanted.pre_merge_importers,
        current: join_current_lockfile_load::<Reporter>(
            current_lockfile_task,
            install.config,
            &workspace.prefix,
        )
        .await,
        early_host_detection,
        pnpmfile_hook,
        manifests,
    })
}

type CurrentLockfileLoad =
    tokio::task::JoinHandle<Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError>>;

// Read the *current* lockfile (`<virtual_store_dir>/lock.yaml`)
// off the reactor while the wanted lockfile parses on this
// task: both are megabyte-scale YAML documents on a large
// workspace, and neither read depends on the other. The result
// is consumed further down, where the install dispatch needs
// it.
fn spawn_current_lockfile_load(config: &Config) -> CurrentLockfileLoad {
    let virtual_store_dir = config.virtual_store_dir.clone();
    tokio::task::spawn_blocking(move || {
        Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
    })
}

// Load the *current* lockfile that records what the previous
// install actually materialized in `<virtual_store_dir>/lock.yaml`.
// The frozen-lockfile path diffs each wanted snapshot against
// this on a per-`PackageKey` basis to decide whether the
// already-installed slot is still usable. `Ok(None)` on a
// first install (the file doesn't exist yet). A corrupted /
// version-incompatible file is disposable state: pnpm warns and
// continues with an empty current lockfile because the wanted
// lockfile and filesystem remain authoritative.
async fn join_current_lockfile_load<Reporter: self::Reporter>(
    task: CurrentLockfileLoad,
    config: &Config,
    prefix: &str,
) -> Option<Lockfile> {
    let phase_start = std::time::Instant::now();
    let current_lockfile = load_current_lockfile::<Reporter>(
        task.await.expect("join the current-lockfile load task"),
        config,
        prefix,
    );
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "load_current_lockfile",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    current_lockfile
}

/// The project manifests as `packageExtensions` and the pnpmfile's
/// `readPackage` rewrote them. An empty layer means the layer below it
/// stands.
struct HookedManifests {
    extended: Vec<(PathBuf, PackageManifest)>,
    hooked: Vec<(PathBuf, PackageManifest)>,
}

impl HookedManifests {
    // pnpm's `getContext` runs `readPackage` over every project
    // manifest before anything reads it, so a hook that rewrites a
    // project's own specifier steers the resolution, the freshness
    // gates, and the importer entries the lockfile records alike.
    // The optimistic repeat-install check above stays on the on-disk
    // manifests on purpose: it is the one gate that must not spawn
    // the Node worker.
    // `packageExtensions` runs ahead of the pnpmfile's `readPackage`,
    // the order the resolver applies them in. The freshness gates below
    // compare against these, because the lockfile they check was written
    // from the extended manifests too — a peer an extension injects into
    // a workspace project is auto-installed and recorded, so a check that
    // read the file on disk would see it as a dependency that vanished.
    async fn hook<Reporter: self::Reporter>(
        config: &Config,
        workspace_root: &Path,
        declared: &[(PathBuf, &PackageManifest)],
        pnpmfile_hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
        pre_hooked_paths: &HashSet<PathBuf>,
    ) -> Result<Self, InstallError> {
        let extended = extend_project_manifests(config, declared)?;
        let extended_view = manifests_view(declared, &extended);
        let read_package_log =
            pnpmfile_hook.map(|hook| read_package_log::<Reporter>(hook, workspace_root));
        let every_project_manifest_is_pre_hooked =
            extended_view.iter().all(|(_, manifest)| pre_hooked_paths.contains(manifest.path()));
        let hooked = hook_project_manifests(
            (pnpmfile_hook, read_package_log.as_ref()),
            &extended_view,
            pre_hooked_paths,
            every_project_manifest_is_pre_hooked,
        )
        .await?;
        Ok(Self { extended, hooked })
    }

    fn view<'v>(
        &'v self,
        declared: &'v [(PathBuf, &'v PackageManifest)],
    ) -> std::borrow::Cow<'v, [(PathBuf, &'v PackageManifest)]> {
        if self.hooked.is_empty() {
            manifests_view(declared, &self.extended)
        } else {
            manifests_view(declared, &self.hooked)
        }
    }
}

fn read_package_log<Reporter: self::Reporter>(
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    workspace_root: &Path,
) -> pnpm_hooks::LogFn {
    hook.source_path().map_or_else(
        || Arc::new(|_| {}) as pnpm_hooks::LogFn,
        |from| {
            crate::install_with_fresh_lockfile::hook_log_fn::<Reporter>(
                workspace_root,
                from,
                "readPackage",
            )
        },
    )
}

/// The wanted lockfile as the run resolves against it: the loaded one, or
/// what synthesis, the branch fold and the fast update made of it. Each
/// later layer stands in for the ones below when it applied.
struct WantedLockfile<'a> {
    /// The on-disk `pnpm-lock.yaml`, the dry-run diff's baseline.
    loaded: Option<&'a Lockfile>,
    synthesized: Option<Lockfile>,
    merged_branch: Option<Lockfile>,
    fast_updated: Option<Lockfile>,
}

impl WantedLockfile<'_> {
    fn get(&self) -> Option<&Lockfile> {
        self.fast_updated
            .as_ref()
            .or(self.merged_branch.as_ref())
            .or(self.synthesized.as_ref())
            .or(self.loaded)
    }

    fn synthesized_from_current(&self) -> bool {
        self.synthesized.is_some()
    }

    /// The loader's `Arc` handle, forwarded only while the loaded document
    /// still is the wanted lockfile: synthesis, the branch fold and the
    /// fast update each replace it, and a consumer must never seed from
    /// a superseded document.
    fn loader_handle(&self, shared: Option<Arc<Lockfile>>) -> Option<Arc<Lockfile>> {
        shared.filter(|shared| {
            self.get().is_some_and(|lockfile| std::ptr::eq(lockfile, Arc::as_ptr(shared)))
        })
    }

    fn was_fast_updated(&self) -> bool {
        self.fast_updated.is_some()
    }
}

/// The wanted lockfile settled, with the manifests the freshness gates
/// compare it against.
struct Lockfiles<'a> {
    wanted: WantedLockfile<'a>,
    manifest_freshness_inputs: Vec<(String, &'a PackageManifest)>,
}

async fn settle_wanted_lockfile<'a: 'w, 'w, Reporter: self::Reporter + 'static>(
    install: InstallView<'a>,
    mode: &RunMode,
    workspace: &InstallWorkspace<'_>,
    scope: &InstallScope<'_>,
    loaded: &Loaded<'a>,
    project_manifests: &'w [(PathBuf, &'w PackageManifest)],
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
) -> Result<Lockfiles<'w>, InstallError> {
    let mut lockfiles = Lockfiles {
        wanted: WantedLockfile {
            loaded: loaded.lockfile,
            synthesized: None,
            merged_branch: None,
            fast_updated: None,
        },
        manifest_freshness_inputs: manifest_freshness_inputs(
            &workspace.workspace_root,
            project_manifests,
            selection,
        ),
    };
    // Synthesize the wanted lockfile from `<virtual_store_dir>/lock.yaml`
    // when `pnpm-lock.yaml` is absent and the materialized snapshot still
    // satisfies the manifest. The install then skips resolution and
    // regenerates `pnpm-lock.yaml` from the synthesized object.
    lockfiles.wanted.synthesized = synthesize_lockfile_from_current(
        loaded.current.as_ref(),
        SynthesizeScope {
            lockfile_is_absent: loaded.lockfile.is_none(),
            frozen_lockfile: install.frozen_lockfile,
            prefer_frozen_lockfile: mode.prefer_frozen_lockfile,
            workspace_root: &workspace.workspace_root,
            manifest_freshness_inputs: &lockfiles.manifest_freshness_inputs,
            config: install.config,
            catalogs: &workspace.catalogs,
            pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
            ignore_manifest_check: install.ignore_manifest_check,
            prune_stale_importers: scope.prune_stale_importers,
        },
    )
    .await;
    // The branch lockfiles were folded in at load, before any manifest
    // was known. Reconcile the fold against them now, while every
    // later stage — the fast update, the freshness check, and the
    // rewrite the merge is saved by — still reads the same object.
    lockfiles.wanted.merged_branch = loaded
        .pre_merge_importers
        .zip(lockfiles.wanted.get())
        .and_then(|(pre_merge_importers, lockfile)| {
            prune_merged_branch_lockfile(
                lockfile,
                pre_merge_importers,
                &lockfiles.manifest_freshness_inputs,
                install.config.auto_install_peers,
            )
        });
    if may_fast_update_lockfile(
        install.frozen_lockfile,
        install.dry_run,
        mode.prefer_frozen_lockfile,
        install.mutation,
    ) {
        lockfiles.wanted.fast_updated =
            try_fast_update_lockfile::<Reporter>(FastUpdateLockfileOptions {
                lockfile: lockfiles.wanted.get(),
                lockfile_dir: &workspace.workspace_root,
                manifests: &lockfiles.manifest_freshness_inputs,
                project_manifests,
                config: install.config,
                catalogs: &workspace.catalogs,
                pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
                ignore_manifest_check: install.ignore_manifest_check,
                prune_stale_importers: scope.prune_stale_importers,
            })
            .await;
    }
    Ok(lockfiles)
}

// One per-install packument cache shared with both the
// lockfile-verifier (below) and the resolver in
// `install_with_fresh_lockfile` (further down). The
// single instance lets a name the resolver fetched during this
// install short-circuit the verifier's own fetch chain, and
// vice versa.
// Resolution verifiers re-apply `minimumReleaseAge` /
// `trustPolicy='no-downgrade'` (plus the tarball-URL anti-tamper
// check) to every entry in the loaded `pnpm-lock.yaml`. They are
// built here — cheap, no I/O — but the verification fan-out itself
// is dispatched per path below: on the frozen materialization path
// it runs concurrently with the fetch (see [`InstallFrozenLockfile`])
// so the per-entry registry round trips overlap the download;
// every other path (fresh resolve, the lockfile-only / up-to-date
// short-circuits) verifies eagerly via [`verify_lockfile_eagerly`]
// before it proceeds. `trust_lockfile` (the OR of yaml's
// `trustLockfile` and the `--trust-lockfile` CLI flag, resolved in
// [`crate::cli_args::install::InstallArgs::run`]; the opt-out for
// environments that treat the on-disk lockfile as
// already-trusted) or no active resolution policy leaves the list
// empty, making every gate a no-op — fresh local resolution is
// already filtered by the resolver's own per-version gate
// (`minimumReleaseAge` via `ResolveResult::policy_violation`,
// `trustPolicy='no-downgrade'` via the npm resolver's
// `fail_if_trust_downgraded_for_pick`). The list is built whenever
// a policy could apply, independent of whether a lockfile is loaded, so the
// fresh-resolve path can record the freshly written lockfile as
// already-verified (see `record_lockfile_verified` below).
// Shared with `CreateVirtualStore`, which fills it after its
// warm/cold partition so the verifier's age gate can lean on
// this install's own canonical tarball fetches instead of a
// metadata body per entry.
struct Verification {
    meta_cache: Arc<InMemoryPackageMetaCache>,
    planned_canonical_fetches: pnpm_resolving_resolver_base::PlannedCanonicalFetches,
    resolution_verifiers: Vec<Arc<dyn super::ResolutionVerifier>>,
    derived_lockfile_path: Option<PathBuf>,
}

impl Verification {
    fn set_up(
        install: InstallView<'_>,
        owned: &InstallOwned,
        has_lockfile: bool,
        workspace_root: &Path,
    ) -> Result<Self, InstallError> {
        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
        let planned_canonical_fetches =
            pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
        let resolution_verifiers = install_resolution_verifiers(
            install.config,
            install.trust_lockfile,
            (&owned.http_client_arc, &meta_cache, owned.auth_override.as_ref()),
            &planned_canonical_fetches,
        )?;
        Ok(Self {
            meta_cache,
            planned_canonical_fetches,
            resolution_verifiers,
            derived_lockfile_path: has_lockfile.then(|| {
                install.lockfile_path.map_or_else(
                    || workspace_root.join(install.config.wanted_lockfile_name()),
                    Path::to_path_buf,
                )
            }),
        })
    }
}

/// Everything the run has settled before it dispatches.
#[derive(Clone, Copy)]
struct Settled<'r, 'a> {
    install: InstallView<'a>,
    owned: &'r InstallOwned,
    mode: &'r RunMode,
    workspace: &'r InstallWorkspace<'a>,
    scope: &'r InstallScope<'a>,
    loaded: &'r Loaded<'a>,
    project_manifests: &'r [(PathBuf, &'a PackageManifest)],
    lockfiles: &'r Lockfiles<'a>,
    verification: &'r Verification,
}

/// Which path the install takes, and the modules state it starts from.
struct Dispatched<'install> {
    take_frozen_path: bool,
    modules: PreparedModulesState<'install>,
}

/// Announce the install, run `pnpm:devPreinstall`, and decide between the
/// frozen and the fresh path. Returns `None` when the run is complete: a
/// `--lockfile-only` frozen install, or a modules state found up to date.
/// Consumes the lockfile verification override off `options`.
async fn dispatch<'install, Reporter: self::Reporter + 'static>(
    settled: Settled<'_, '_>,
    options: &mut InstallRunOptions<'install, '_>,
) -> Result<Option<Dispatched<'install>>, InstallError> {
    let Settled {
        install,
        owned,
        mode,
        workspace,
        scope,
        loaded,
        project_manifests,
        lockfiles,
        verification,
    } = settled;
    announce_import::<Reporter>(settled, options.rebuild.as_ref())?;
    // Dispatch priority, following the CLI + `preferFrozenLockfile`
    // semantics:
    //
    // 1. `--frozen-lockfile` flag → frozen path. Lockfile must exist
    //    and the freshness check (settings + per-importer specifier
    //    match) must pass, otherwise fail.
    //
    // 2. No flag, lockfile present, `prefer_frozen_lockfile == true`,
    //    and the freshness check passes → frozen path (same code as
    //    state 1). The `preferFrozenLockfile` fast path: when the
    //    lockfile matches the manifest, the install silently goes
    //    headless instead of re-resolving against the registry.
    //
    // 3. No flag, lockfile present, but either `prefer_frozen_lockfile`
    //    is off or the freshness check fails → fresh-resolve path,
    //    seeded from the existing lockfile so unrelated entries keep
    //    their pins (the `update: false` resolver mode).
    //
    // 4. No lockfile → fresh-resolve path with no seed, writes a
    //    brand-new `pnpm-lock.yaml`.
    //
    reject_frozen_with_update_checksums(install.update_checksums, install.frozen_lockfile)?;
    // Compute the dispatch decision once. `take_frozen_path` is true
    // for both state 1 (--frozen-lockfile) and state 2 (auto-frozen
    // via prefer-frozen-lockfile). The freshness check fires for both
    // — fatal for state 1, fall-through for state 2.
    //
    // `--dry-run` always takes the fresh-resolve path: it must compute
    // the would-be lockfile to diff against the existing one, and the
    // frozen freshness gate would otherwise abort on a stale lockfile
    // instead of reporting the change.
    let take_frozen_path = decide_frozen_path(&FrozenDispatch {
        dry_run: install.dry_run,
        frozen_lockfile: install.frozen_lockfile,
        update_checksums: install.update_checksums,
        prefer_frozen_lockfile: mode.prefer_frozen_lockfile,
        lockfile: lockfiles.wanted.get(),
        lockfile_synthesized_from_current: lockfiles.wanted.synthesized_from_current(),
        workspace_root: &workspace.workspace_root,
        manifest_freshness_inputs: &lockfiles.manifest_freshness_inputs,
        config: install.config,
        catalogs: &workspace.catalogs,
        pnpmfile_hook: loaded.pnpmfile_hook.as_ref(),
        ignore_manifest_check: install.ignore_manifest_check,
        prune_stale_importers: scope.prune_stale_importers,
    })
    .await?;

    if take_frozen_path && mode.lockfile_only {
        let lockfile =
            lockfiles.wanted.get().expect("frozen dispatch verified lockfile is present");
        finish_frozen_lockfile_only::<Reporter>(
            lockfile,
            install.config,
            LockfileOnlyFrozen {
                workspace_root: &workspace.workspace_root,
                prefix: &workspace.prefix,
                resolution_verifiers: &verification.resolution_verifiers,
                derived_lockfile_path: verification.derived_lockfile_path.as_deref(),
                lockfile_verification_override: options.lockfile_verification_override.take(),
            },
        )
        .await?;
        Reporter::emit(&LogEvent::Summary(SummaryLog {
            level: LogLevel::Debug,
            prefix: workspace.prefix.clone(),
        }));
        return Ok(None);
    }

    Ok(prepare_modules_state::<Reporter>(PrepareModulesStateInputs {
        resolve_only: mode.resolve_only,
        take_frozen_path,
        config: install.config,
        filtered_install: scope.importers.filtered_install,
        installs_only: install.installs_only,
        workspace_root: &workspace.workspace_root,
        included: mode.included,
        current_lockfile: loaded.current.as_ref(),
        requested_importer_ids: scope.importers.requested_importer_ids.as_ref(),
        node_linker: install.node_linker,
        disable_optimistic_repeat_install: install.disable_optimistic_repeat_install,
        lockfile: lockfiles.wanted.get(),
        supported_architectures: owned.supported_architectures.as_ref(),
        rebuild: options.rebuild.as_ref(),
        resolution_verifiers: &verification.resolution_verifiers,
        derived_lockfile_path: verification.derived_lockfile_path.as_deref(),
        lockfile_verification_override: options.lockfile_verification_override.take(),
        lockfile_synthesized_from_current: lockfiles.wanted.synthesized_from_current(),
        lockfile_was_fast_updated: lockfiles.wanted.was_fast_updated(),
        save_lockfile: options.save_lockfile,
        catalogs: &workspace.catalogs,
        project_manifests,
        effective_node_version: mode.effective_node_version.as_deref(),
        prefix: &workspace.prefix,
    })
    .await?
    .map(|modules| Dispatched { take_frozen_path, modules }))
}

fn announce_import<Reporter: self::Reporter>(
    settled: Settled<'_, '_>,
    rebuild: Option<&crate::RebuildOptions>,
) -> Result<(), InstallError> {
    let Settled { install, mode, workspace, loaded, project_manifests, .. } = settled;
    // `@pnpm/cli.default-reporter` renders these fields in the install header;
    // `currentLockfileExists` flips after the virtual-store lockfile is written.
    Reporter::emit(&LogEvent::Context(ContextLog {
        level: LogLevel::Debug,
        current_lockfile_exists: loaded.current.is_some(),
        store_dir: install.config.store_dir.display().to_string(),
        virtual_store_dir: install
            .config
            .effective_virtual_store_dir()
            .to_string_lossy()
            .into_owned(),
    }));
    // `pnpm:devPreinstall` runs ahead of everything the install does
    // with the lockfile — including the frozen path's freshness
    // check — because what it prepares is an input to resolution and
    // linking. What skips it:
    //
    // - `resolve_only`, which materializes nothing for the hook to
    //   prepare. pnpm reaches the same outcome by having
    //   `--lockfile-only` (and `--dry-run`, which sets it) imply
    //   `ignoreScripts`.
    // - A rebuild, which resolves and links nothing.
    // - `ignore_manifest_check`, which covers `pacquet fetch` (pnpm's
    //   `ignorePackageManifest`, installing from the lockfile alone)
    //   and the TypeScript CLI delegating a frozen materialization,
    //   which already ran the hook before handing the install over.
    // - [`DEV_PREINSTALL_ALREADY_RAN_ENV`], the delegating CLI's
    //   marker for the one path that carries no flag of its own.
    run_dev_preinstall_hook::<Reporter>(&DevPreinstallScope {
        config: install.config,
        workspace_root: &workspace.workspace_root,
        project_manifests,
        resolve_only: mode.resolve_only,
        ignore_manifest_check: install.ignore_manifest_check,
        rebuild,
    })?;
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: workspace.prefix.clone(),
        stage: Stage::ImportingStarted,
    }));
    tracing::info!(target: "pacquet::install", "Start all");
    Ok(())
}

/// A prompt only reaches a person on an interactive terminal outside CI.
fn prompts_are_answerable() -> bool {
    !is_ci::cached() && std::io::stdin().is_terminal()
}

/// Whether anything could declare a catalog this install has to resolve
/// against.
fn catalog_context_present(
    config: &Config,
    catalogs_override: Option<&super::Catalogs>,
    workspace_dir: Option<&PathBuf>,
) -> bool {
    catalogs_override.is_some()
        || config.catalogs.is_some()
        || (!config.ignore_workspace && workspace_dir.is_some())
}

/// Walk every workspace project's `package.json` once, unless the caller
/// supplied the list. A selection carries its own projects, so it walks
/// nothing.
fn discovered_workspace_projects(
    has_selection: bool,
    workspace_projects_override: Option<Vec<pnpm_workspace::Project>>,
    workspace_dir: &Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
) -> Result<Option<Vec<pnpm_workspace::Project>>, InstallError> {
    if has_selection {
        return Ok(None);
    }
    if let Some(projects) = workspace_projects_override {
        return Ok(Some(projects));
    }
    load_workspace_projects(workspace_dir, workspace_manifest)
        .map_err(InstallError::FindWorkspaceProjects)
}

/// Resolution verifiers re-apply `minimumReleaseAge` /
/// `trustPolicy='no-downgrade'` (plus the tarball-URL anti-tamper check) to
/// every entry in the loaded `pnpm-lock.yaml`. `trust_lockfile` — the opt-out
/// for environments that treat the on-disk lockfile as already-trusted —
/// leaves the list empty, making every gate a no-op.
fn install_resolution_verifiers(
    config: &Config,
    trust_lockfile: bool,
    clients: (
        &Arc<pnpm_network::ThrottledClient>,
        &Arc<InMemoryPackageMetaCache>,
        Option<&Arc<super::AuthHeaders>>,
    ),
    planned_canonical_fetches: &pnpm_resolving_resolver_base::PlannedCanonicalFetches,
) -> Result<Vec<Arc<dyn super::ResolutionVerifier>>, InstallError> {
    if trust_lockfile {
        return Ok(Vec::new());
    }
    let (http_client_arc, meta_cache, auth_override) = clients;
    build_resolution_verifiers(
        config,
        Arc::clone(http_client_arc),
        Some(Arc::clone(meta_cache) as Arc<dyn pnpm_resolving_npm_resolver::PackageMetaCache>),
        auth_override.cloned(),
        None,
        Some(std::sync::Arc::clone(planned_canonical_fetches)),
    )
    .map_err(InstallError::BuildVerifiers)
}

fn reject_frozen_with_update_checksums(
    update_checksums: bool,
    frozen_lockfile: bool,
) -> Result<(), InstallError> {
    if update_checksums && frozen_lockfile {
        return Err(InstallError::FrozenLockfileWithUpdateChecksums);
    }
    Ok(())
}

/// `--lockfile-only` with `lockfile: false` asks for a lockfile the run is
/// forbidden to write.
fn reject_lockfile_only_without_lockfile(
    config: &Config,
    lockfile_only: bool,
) -> Result<(), InstallError> {
    if lockfile_only && !config.lockfile {
        return Err(InstallError::ConfigConflictLockfileOnlyWithNoLockfile);
    }
    Ok(())
}

/// `enableModulesDir: false` (with the global virtual store off) is "resolve
/// and write the lockfile, materialize nothing" — the same pipeline
/// `--lockfile-only` takes, entered from config. It stays outside the
/// `lockfile: false` conflict (pnpm accepts that combination and simply
/// writes nothing), and never turns a rebuild — which runs against an
/// already-materialized `node_modules` — into a silent no-op.
fn effective_lockfile_only(
    config: &Config,
    lockfile_only: bool,
    rebuild: Option<&crate::RebuildOptions>,
) -> bool {
    lockfile_only
        || (rebuild.is_none() && !config.enable_modules_dir && !config.enable_global_virtual_store)
}

fn reject_conflicting_store_config(config: &Config) -> Result<(), InstallError> {
    if config.frozen_store && config.force {
        return Err(InstallError::ConfigConflictFrozenStoreWithForce);
    }
    if config.virtual_store_only
        && !config.enable_modules_dir
        && !config.enable_global_virtual_store
    {
        return Err(InstallError::ConfigConflictVirtualStoreOnlyWithNoModulesDir);
    }
    Ok(())
}

/// A full install (pnpm's `mutation: "install"`) is the workspace-wide one and
/// counts every project; a partial one (`add`, `update`, `remove`, ...)
/// targets the project it was run in and reports the single-project shape,
/// with no `total`, exactly as pnpm's non-recursive `scopeLogger` call does.
fn emit_scope_log<Reporter: self::Reporter>(
    config: &Config,
    mutation: crate::ProjectMutation,
    workspace_projects: Option<&[pnpm_workspace::Project]>,
    workspace_dir: Option<&Path>,
) {
    if !config.shares_one_lockfile() {
        return;
    }
    let workspace_wide = mutation.is_full_install().then_some(workspace_projects).flatten();
    Reporter::emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected: workspace_wide.map_or(1, <[_]>::len),
        total: workspace_wide.map(<[_]>::len),
        workspace_prefix: workspace_dir.map(|dir| dir.to_string_lossy().into_owned()),
    }));
}

/// What decides which projects this install records as importers.
struct ProjectManifestScope<'a, 'scope> {
    manifest: &'a PackageManifest,
    selection: Option<&'scope crate::WorkspaceInstallSelection<'a>>,
    workspace_root: &'scope Path,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    root_manifest_as_workspace_root: bool,
    workspace_projects_are_overridden: bool,
    config: &'scope Config,
}

fn install_project_manifests<'a>(
    scope: &ProjectManifestScope<'a, '_>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    if let Some(selection) = scope.selection {
        return build_selected_project_manifests_list(
            scope.manifest,
            selection.all_projects,
            selection.active_manifest_is_standin,
        );
    }
    if scope.root_manifest_as_workspace_root {
        return build_root_importer_project_manifests_list(
            scope.workspace_root,
            scope.manifest,
            None,
        );
    }
    if scope.workspace_projects_are_overridden || !scope.config.shares_one_lockfile() {
        return build_root_importer_project_manifests_list(
            scope.workspace_root,
            scope.manifest,
            // Dedicated per-project lockfiles record a single "." importer per
            // project; sibling projects only feed the `workspace:` resolver,
            // never the importer list.
            scope.config.shares_one_lockfile().then_some(scope.workspace_projects).flatten(),
        );
    }
    build_project_manifests_list(scope.manifest, scope.workspace_projects)
}

/// What decides whether the install may drop importers no project claims.
struct StaleImporterPrune<'a> {
    filtered_install: bool,
    mutation: crate::ProjectMutation,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    workspace_projects_are_overridden: bool,
    config: &'a Config,
}

/// Only an install that covers a whole workspace sees the complete project
/// list, so only it may conclude that an importer the lockfile records belongs
/// to a project that is gone. This is pnpm's `pruneLockfileImporters`, which
/// its recursive install defaults to the same condition (`pkgs.length ===
/// allProjects.length`) — outside a workspace there is no project list to
/// compare against. A `NodeApiProject[]` handed in by an API consumer carries
/// no promise of listing every workspace project, so it cannot stand in for
/// the project list either.
fn may_prune_stale_importers(prune: &StaleImporterPrune<'_>) -> bool {
    !prune.filtered_install
        && prune.mutation.is_full_install()
        && prune.workspace_projects.is_some()
        && !prune.workspace_projects_are_overridden
        && prune.config.shares_one_lockfile()
}

/// Everything the optimistic repeat-install short-circuit consults.
struct UpToDateCheck<'a> {
    config: &'static Config,
    workspace_root: &'a Path,
    node_linker: super::NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    is_workspace_install: bool,
    lockfile: super::MaybeLazyLockfile<'a>,
    catalogs: &'a super::Catalogs,
    mutation: crate::ProjectMutation,
    update_seed_policy: &'a UpdateSeedPolicy,
    frozen_lockfile: bool,
    disable_optimistic_repeat_install: bool,
    effective_node_version: Option<&'a str>,
    prefix: &'a str,
}

/// Whether nothing has changed since the previous successful install
/// (settings, workspace structure, manifest mtimes), so the whole pipeline can
/// be skipped and pnpm's "Already up to date" log emitted. The fast path runs
/// before any of the install setup (no lockfile reads, no verifier fan-out, no
/// `getContext`).
///
/// Only a full `pacquet install` may short-circuit. `add` and `remove` mutate
/// the manifest in memory and persist it after this run returns, so the
/// on-disk mtimes the check reads still describe the pre-mutation project —
/// without this gate a fresh workspace state would read as "nothing changed →
/// already up to date" and the mutation would never be resolved or
/// materialized. `pacquet update` is excluded through its seed policy: a
/// compatible bump leaves the manifest byte-identical, which the check would
/// likewise read as up to date and skip the registry re-resolution. Disabled
/// under `--frozen-lockfile`: an explicit headless install should always go
/// through the dispatch so a `NoLockfile` or `OutdatedLockfile` error still
/// fires when the lockfile is missing or stale.
///
/// A `--filter` narrowing does not disqualify the run: the check validates the
/// whole workspace (`project_manifests` covers every project even when only a
/// subset is selected), and it refuses a workspace state a filtered install
/// wrote, so "nothing changed" still means every selected project is
/// materialized.
fn install_is_already_up_to_date<Reporter: self::Reporter>(
    check: &UpToDateCheck<'_>,
) -> Result<bool, InstallError> {
    let decided = check.mutation.is_full_install()
        && matches!(check.update_seed_policy, UpdateSeedPolicy::KeepAll)
        && !check.frozen_lockfile
        && !check.config.force
        && !check.disable_optimistic_repeat_install
        && check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
            workspace_root: check.workspace_root,
            config: check.config,
            node_linker: check.node_linker,
            included: check.included,
            supported_architectures: check.supported_architectures,
            project_manifests: check.project_manifests,
            is_workspace_install: check.is_workspace_install,
            lockfile: check.lockfile,
            catalogs: check.catalogs,
        }) == OptimisticRepeatInstallDecision::UpToDate;
    if !decided {
        return Ok(false);
    }
    if !build_state_allows_short_circuit(check)? {
        return Ok(false);
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Already up to date".to_string(),
        prefix: check.prefix.to_string(),
    }));
    Ok(true)
}

/// Whether the recorded build state lets the fast path stand.
///
/// A build marker lives in the shared slot, outside every project-state input
/// the repeat check reads. And `strictDepBuilds` stays enforced across reruns:
/// an install that already recorded unapproved ignored builds must keep
/// failing until they are approved, not exit 0 via the fast path. An
/// `allowBuilds` change that newly permits one is already caught by
/// `settings_match` (the policy is part of the workspace state), which reports
/// drift and skips this branch, so the full install runs and rebuilds it.
///
/// A corrupt / unreadable `.modules.yaml` can't prove there are no recorded
/// ignored builds, so under strict mode the fast path is refused rather than
/// short-circuiting on a swallowed read error.
fn build_state_allows_short_circuit(check: &UpToDateCheck<'_>) -> Result<bool, InstallError> {
    if gvs_build_markers_may_require_recovery(check.config) {
        match check.lockfile.get() {
            Ok(Some(wanted)) => {
                if gvs_build_marker_present(
                    wanted,
                    check.config,
                    check.workspace_root,
                    check.effective_node_version,
                ) {
                    return Ok(false);
                }
            }
            Ok(None) => {}
            Err(_) => return Ok(false),
        }
    }
    if !check.config.strict_dep_builds {
        return Ok(true);
    }
    match pnpm_modules_yaml::read_modules_layout::<Host>(&check.config.modules_dir) {
        Ok(Some(modules)) => match unapproved_recorded_ignored_builds(&modules, check.config) {
            Ok(Some(package_names)) => Err(InstallError::IgnoredBuilds { package_names }),
            Ok(None) => Ok(true),
            // Unreadable state or a malformed `allowBuilds`: can't trust the
            // fast path, run the full install.
            Err(_) => Ok(false),
        },
        Ok(None) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Report the projects this install covers depending on each other in a cycle
/// — after the repeat-install short-circuit, because pnpm returns from
/// "Already up to date" before reaching its own check, and before any
/// resolution, because a `disallowWorkspaceCycles` failure must not be paid
/// for.
fn report_install_scope_cycles<Reporter: self::Reporter>(
    config: &Config,
    workspace: &InstallWorkspace<'_>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    scope: (crate::ProjectMutation, Option<&[pnpm_workspace::Project]>),
) -> Result<(), InstallError> {
    if config.ignore_workspace_cycles {
        return Ok(());
    }
    let Some(workspace_dir) = workspace.workspace_dir.as_deref() else { return Ok(()) };
    let (mutation, workspace_projects) = scope;
    let scope = match selection {
        // A plan that already sequenced this very graph hands its cycle report
        // over; the install then skips rebuilding the graph just to find them
        // again.
        Some(selection) => match selection.workspace_cycles {
            crate::PrecomputedWorkspaceCycles::Known(cycles) => {
                return crate::report_workspace_cycles::<Reporter>(config, workspace_dir, cycles)
                    .map_err(InstallError::CyclicWorkspaceDependencies);
            }
            crate::PrecomputedWorkspaceCycles::Unknown => {
                Some((selection.all_projects, Some(selection.selected_dirs)))
            }
        },
        // A single-project mutation (`add`, `update`, ...) has no set to cycle
        // within; only a full install covers the whole workspace.
        None => mutation
            .is_full_install()
            .then_some(workspace_projects)
            .flatten()
            .map(|projects| (projects, None)),
    };
    let Some((projects, selected_dirs)) = scope else { return Ok(()) };
    let cycles = crate::install_scope_cycles(
        config,
        workspace_dir,
        &workspace.catalogs,
        projects,
        selected_dirs,
    )
    .map_err(InstallError::InvalidOverrides)?;
    crate::report_workspace_cycles::<Reporter>(config, workspace_dir, cycles.as_deref())
        .map_err(InstallError::CyclicWorkspaceDependencies)
}

/// The wanted lockfile's contents, with the loader's handles onto the same
/// document. A broken lockfile is regenerable state, so only a frozen install
/// treats it as fatal (upstream `readLockfiles`).
///
/// The fold's "before" is read out here rather than at its use site: a load
/// that failed leaves nothing cached, so asking later would retry it and turn
/// a lockfile this arm chose to ignore into a fatal one.
/// The wanted lockfile as its loader holds it: the document, the loader's
/// shared handle to it, the intact copy a filtered install splices back
/// over, and the importers as they were before the branch lockfiles were
/// folded in.
#[derive(Default)]
struct LoadedWantedLockfile<'a> {
    lockfile: Option<&'a Lockfile>,
    shared: Option<Arc<Lockfile>>,
    merge: Option<&'a Lockfile>,
    pre_merge_importers:
        Option<&'a std::collections::HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
}

fn load_wanted_lockfile<'a, Reporter: self::Reporter>(
    lockfile_source: super::MaybeLazyLockfile<'a>,
    frozen_lockfile: bool,
    context: (&Path, &str),
) -> Result<LoadedWantedLockfile<'a>, InstallError> {
    let (workspace_root, prefix) = context;
    match lockfile_source.get() {
        Ok(lockfile) => Ok(LoadedWantedLockfile {
            lockfile,
            shared: lockfile_source.shared().map_err(InstallError::LoadWantedLockfile)?,
            merge: lockfile_source.get_for_merge().map_err(InstallError::LoadWantedLockfile)?,
            pre_merge_importers: lockfile_source
                .pre_merge_importers()
                .map_err(InstallError::LoadWantedLockfile)?,
        }),
        Err(error) if !frozen_lockfile => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Ignoring broken lockfile at {}: {error}",
                    workspace_root.display(),
                ),
                prefix: prefix.to_string(),
            }));
            Ok(LoadedWantedLockfile::default())
        }
        Err(error) => Err(InstallError::LoadWantedLockfile(error)),
    }
}

/// A constraint-free lockfile spawns nothing — the probe's result would go
/// unused (see `detect_installability_host` for why that matters) — and
/// neither does `--force` (skips the checks) or a resolve-only pass (returns
/// before them). The scan is of the *wanted* lockfile: a fresh resolve whose
/// new graph gains constraints the old lockfile lacked detects the host at
/// its own site.
fn needs_early_host_detection(
    config: &Config,
    resolve_only: bool,
    lockfile: Option<&Lockfile>,
) -> bool {
    !config.force
        && !resolve_only
        && lockfile.is_some_and(|lockfile| match (&lockfile.snapshots, &lockfile.packages) {
            (Some(snapshots), Some(packages)) if !snapshots.is_empty() => {
                pnpm_deps_restorer::any_installability_constraint(snapshots, packages)
            }
            _ => false,
        })
}

/// Register the workspace root in the store's project registry, once per
/// install. Store prune walks the workspace's `node_modules/.pnpm/` to find
/// every installed package, so one entry per workspace is enough.
///
/// Gated on `enableGlobalVirtualStore` because pacquet wires the
/// prune-by-registry path only under GVS for now; pnpm registers
/// unconditionally, so once the non-GVS prune path lands the gate should be
/// dropped.
///
/// Best-effort: a registry write failure shouldn't fail the install, so it is
/// surfaced as `tracing::warn!` instead.
fn register_workspace_in_store(config: &Config, workspace_root: &Path) {
    if !config.enable_global_virtual_store {
        return;
    }
    // Create the store root before calling `register_project` so its
    // `path_contains` guard can canonicalize the path instead of falling
    // through to a literal comparison that wrongly matches against
    // `<workspace>/../pacquet-store/v11`-shaped relative store paths
    // (resolved-on-disk: outside the workspace; lexical: starts with the
    // workspace prefix).
    if let Err(error) = std::fs::create_dir_all(pnpm_store_dir::StoreDir::root(&config.store_dir)) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to ensure store root exists before project registry write; install continues",
        );
    }
    if let Err(error) = pnpm_store_dir::register_project(&config.store_dir, workspace_root) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to register workspace root in the store project registry; install continues",
        );
    }
}

/// The pnpmfile whose checksum the freshness gates compare against a
/// lockfile's `pnpmfileChecksum`, resolved the way the install that records
/// one resolves it. Building the handle costs a `stat`; the Node worker only
/// starts if a gate has to ask whether the pnpmfile exports hooks.
fn resolve_pnpmfile_hook(
    config: &Config,
    workspace_root: &Path,
    override_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> Result<Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>, InstallError> {
    if let Some(hook) = override_hook {
        return Ok(Some(hook));
    }
    if config.ignore_pnpmfile {
        return Ok(None);
    }
    pnpm_hooks::finder::load_pnpmfiles(workspace_root, crate::pnpmfile_selection(config))
        .map_err(InstallError::MissingPnpmfile)
}

/// Borrow the rewritten manifests when a pass produced any, keeping the
/// caller's own list otherwise.
fn manifests_view<'v>(
    declared: &'v [(PathBuf, &'v PackageManifest)],
    rewritten: &'v [(PathBuf, PackageManifest)],
) -> std::borrow::Cow<'v, [(PathBuf, &'v PackageManifest)]> {
    if rewritten.is_empty() {
        return std::borrow::Cow::Borrowed(declared);
    }
    std::borrow::Cow::Owned(
        rewritten.iter().map(|(project_dir, manifest)| (project_dir.clone(), manifest)).collect(),
    )
}

/// pnpm's `getContext` runs `readPackage` over every project manifest before
/// anything reads it, so a hook that rewrites a project's own specifier steers
/// the resolution, the freshness gates, and the importer entries the lockfile
/// records alike. Empty when no hook applies or every manifest was hooked
/// already.
async fn hook_project_manifests(
    hook: (Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>, Option<&pnpm_hooks::LogFn>),
    project_manifests: &[(PathBuf, &PackageManifest)],
    pre_hooked_paths: &HashSet<PathBuf>,
    every_manifest_is_pre_hooked: bool,
) -> Result<Vec<(PathBuf, PackageManifest)>, InstallError> {
    let (Some(hook), Some(log)) = hook else { return Ok(Vec::new()) };
    if every_manifest_is_pre_hooked {
        return Ok(Vec::new());
    }
    futures_util::future::try_join_all(project_manifests.iter().map(|(project_dir, manifest)| {
        let ctx = pnpm_hooks::HookContext { log: Arc::clone(log), dir: None };
        let pre_hooked = pre_hooked_paths.contains(manifest.path());
        async move { hook_one_manifest(hook, ctx, project_dir, manifest, pre_hooked).await }
    }))
    .await
}

async fn hook_one_manifest(
    hook: &Arc<dyn pnpm_hooks::PnpmfileHooks>,
    ctx: pnpm_hooks::HookContext,
    project_dir: &Path,
    manifest: &PackageManifest,
    pre_hooked: bool,
) -> Result<(PathBuf, PackageManifest), InstallError> {
    if pre_hooked {
        return Ok((project_dir.to_path_buf(), manifest.clone()));
    }
    let value = hook
        .read_package(manifest.value().clone(), ctx)
        .await
        .map_err(InstallError::ReadPackageHook)?;
    let mut hooked = manifest.clone();
    *hooked.value_mut() = (*value).clone();
    Ok((project_dir.to_path_buf(), hooked))
}

/// The `(importer_id, manifest)` pairs the freshness gates compare the
/// lockfile against.
fn manifest_freshness_inputs<'a>(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
) -> Vec<(String, &'a PackageManifest)> {
    let Some(selection) = selection else {
        return project_manifests
            .iter()
            .map(|(project_dir, manifest)| {
                (pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
            })
            .collect();
    };
    selected_manifest_freshness_inputs(workspace_root, project_manifests, selection.install_dirs)
}

/// A corrupted or version-incompatible current lockfile is disposable state:
/// pnpm warns and continues with none, because the wanted lockfile and
/// filesystem remain authoritative.
fn load_current_lockfile<Reporter: self::Reporter>(
    loaded: Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError>,
    config: &Config,
    prefix: &str,
) -> Option<Lockfile> {
    match loaded {
        Ok(lockfile) => lockfile,
        Err(error) => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Ignoring broken lockfile at {}: {error}",
                    config.virtual_store_dir.display(),
                ),
                prefix: prefix.to_string(),
            }));
            None
        }
    }
}

/// What decides whether the current lockfile may stand in for a missing
/// `pnpm-lock.yaml`.
struct SynthesizeScope<'a> {
    lockfile_is_absent: bool,
    frozen_lockfile: bool,
    prefer_frozen_lockfile: bool,
    workspace_root: &'a Path,
    manifest_freshness_inputs: &'a [(String, &'a PackageManifest)],
    config: &'a Config,
    catalogs: &'a super::Catalogs,
    pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    ignore_manifest_check: bool,
    prune_stale_importers: bool,
}

/// Synthesize the wanted lockfile from `<virtual_store_dir>/lock.yaml` when
/// `pnpm-lock.yaml` is absent and the materialized snapshot still satisfies
/// the manifest. The install then skips resolution and regenerates
/// `pnpm-lock.yaml` from the synthesized object.
async fn synthesize_lockfile_from_current(
    current_lockfile: Option<&Lockfile>,
    scope: SynthesizeScope<'_>,
) -> Option<Lockfile> {
    let current = current_lockfile?;
    if !scope.lockfile_is_absent || scope.frozen_lockfile || !scope.prefer_frozen_lockfile {
        return None;
    }
    check_lockfile_freshness(
        current,
        scope.workspace_root,
        scope.manifest_freshness_inputs,
        scope.config,
        scope.catalogs,
        scope.pnpmfile_hook,
        FreshnessScope {
            ignore_manifest_check: scope.ignore_manifest_check,
            allow_missing_dependency_free_importers: true,
            prune_stale_importers: scope.prune_stale_importers,
        },
    )
    .await
    .ok()
    .map(|()| current.clone())
}

fn may_fast_update_lockfile(
    frozen_lockfile: bool,
    dry_run: bool,
    prefer_frozen_lockfile: bool,
    mutation: crate::ProjectMutation,
) -> bool {
    !frozen_lockfile && !dry_run && prefer_frozen_lockfile && mutation.may_fast_update_lockfile()
}

/// What decides whether the `devPreinstall` hook runs.
struct DevPreinstallScope<'a> {
    config: &'a Config,
    workspace_root: &'a Path,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    resolve_only: bool,
    ignore_manifest_check: bool,
    rebuild: Option<&'a crate::RebuildOptions>,
}

/// pnpm reads the hook off the root project's in-memory manifest and only
/// shells out when it is defined. Falling back to the executor's own read
/// covers a root that isn't among the importers, as a filtered install's is
/// not — pnpm's `safeReadProjectManifestOnly` fallback.
fn run_dev_preinstall_hook<Reporter: self::Reporter>(
    scope: &DevPreinstallScope<'_>,
) -> Result<(), InstallError> {
    if scope.config.ignore_scripts
        || scope.resolve_only
        || scope.ignore_manifest_check
        || scope.rebuild.is_some()
        || dev_preinstall_already_ran()
    {
        return Ok(());
    }
    let normalized_root = pnpm_fs::lexical_normalize(scope.workspace_root);
    let root_defines_hook = scope
        .project_manifests
        .iter()
        .find(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir) == normalized_root)
        .is_none_or(|(_, manifest)| {
            matches!(manifest.script(DEV_PREINSTALL_STAGE, true), Ok(Some(_)))
        });
    if !root_defines_hook {
        return Ok(());
    }
    run_dev_preinstall::<Reporter>(scope.config, scope.workspace_root)
}

/// What the frozen-vs-fresh dispatch decides on.
struct FrozenDispatch<'a> {
    dry_run: bool,
    frozen_lockfile: bool,
    update_checksums: bool,
    prefer_frozen_lockfile: bool,
    lockfile: Option<&'a Lockfile>,
    lockfile_synthesized_from_current: bool,
    workspace_root: &'a Path,
    manifest_freshness_inputs: &'a [(String, &'a PackageManifest)],
    config: &'a Config,
    catalogs: &'a super::Catalogs,
    pnpmfile_hook: Option<&'a Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    ignore_manifest_check: bool,
    prune_stale_importers: bool,
}

/// `take_frozen_path` is true for both state 1 (`--frozen-lockfile`) and state
/// 2 (auto-frozen via `preferFrozenLockfile`). The freshness check fires for
/// both — fatal for state 1, fall-through for state 2.
///
/// `--dry-run` always takes the fresh-resolve path: it must compute the
/// would-be lockfile to diff against the existing one, and the frozen
/// freshness gate would otherwise abort on a stale lockfile instead of
/// reporting the change.
async fn decide_frozen_path(dispatch: &FrozenDispatch<'_>) -> Result<bool, InstallError> {
    if dispatch.dry_run {
        return Ok(false);
    }
    if dispatch.frozen_lockfile {
        let Some(lockfile) = dispatch.lockfile else {
            return Err(InstallError::NoLockfile);
        };
        // Run the freshness gates; on failure surface a fatal InstallError via
        // `FreshnessCheckError`'s `From` impl. The check is run for its side
        // effect (the typed outcome) — the borrowed lockfile / manifests are
        // consumed again inside the frozen branch below.
        //
        // pnpm's importer-set gate sits in the auto-frozen branch of
        // `isFrozenInstallPossible`, which an explicit `--frozen-lockfile`
        // short-circuits past, so a removed project does not fail the install
        // there.
        check_lockfile_freshness(
            lockfile,
            dispatch.workspace_root,
            dispatch.manifest_freshness_inputs,
            dispatch.config,
            dispatch.catalogs,
            dispatch.pnpmfile_hook,
            FreshnessScope {
                ignore_manifest_check: dispatch.ignore_manifest_check,
                allow_missing_dependency_free_importers: false,
                prune_stale_importers: false,
            },
        )
        .await
        .map_err(InstallError::from)?;
        return Ok(true);
    }
    if dispatch.update_checksums {
        return Ok(false);
    }
    let Some(lockfile) = dispatch.lockfile else { return Ok(false) };
    // Auto-frozen via `preferFrozenLockfile`. Skip when the user opted out
    // (`--no-prefer-frozen-lockfile` / `preferFrozenLockfile: false`).
    if !dispatch.prefer_frozen_lockfile {
        return Ok(false);
    }
    auto_frozen_path(dispatch, lockfile).await
}

/// Consult the freshness gate for an auto-frozen install. A `Stale` /
/// `NoImporter` outcome routes to the fresh-resolve path; a malformed
/// `pnpm.overrides` is a user-config error that surfaces regardless of
/// dispatch.
async fn auto_frozen_path(
    dispatch: &FrozenDispatch<'_>,
    lockfile: &Lockfile,
) -> Result<bool, InstallError> {
    match check_lockfile_freshness(
        lockfile,
        dispatch.workspace_root,
        dispatch.manifest_freshness_inputs,
        dispatch.config,
        dispatch.catalogs,
        dispatch.pnpmfile_hook,
        FreshnessScope {
            ignore_manifest_check: dispatch.ignore_manifest_check,
            allow_missing_dependency_free_importers: true,
            prune_stale_importers: dispatch.prune_stale_importers,
        },
    )
    .await
    {
        // Even an up-to-date lockfile may not go frozen: a custom resolver's
        // `shouldRefreshResolution` can force the fresh-resolve path. The
        // hook's verdict blocks the frozen install. A lockfile synthesized
        // from the current snapshot skips the check (it only gates on a
        // non-empty wanted lockfile). A throwing hook aborts the install.
        Ok(()) => Ok(dispatch.lockfile_synthesized_from_current
            || dispatch.config.ignore_pnpmfile
            || !crate::check_custom_resolver_force_resolve::force_resolve_from_pnpmfile(
                lockfile,
                dispatch.pnpmfile_hook.map(std::convert::AsRef::as_ref),
            )
            .await
            .map_err(InstallError::CustomResolverForceResolve)?),
        Err(error @ (FreshnessCheckError::Stale(_) | FreshnessCheckError::NoImporter { .. })) => {
            tracing::info!(
                target: "pacquet::install",
                reason = %error,
                "lockfile not usable as-is; falling through to a fresh resolve",
            );
            Ok(false)
        }
        Err(
            error @ (FreshnessCheckError::InvalidOverrides(_)
            | FreshnessCheckError::CalcPatchHashes(_)),
        ) => Err(error.into()),
    }
}

/// What a frozen `--lockfile-only` run still has to verify and write.
struct LockfileOnlyFrozen<'a, 'install> {
    workspace_root: &'a Path,
    prefix: &'a str,
    resolution_verifiers: &'a [Arc<dyn super::ResolutionVerifier>],
    derived_lockfile_path: Option<&'a Path>,
    lockfile_verification_override: Option<super::LockfileVerificationOverride<'install>>,
}

/// This path materializes nothing, so there's no fetch to overlap; verify
/// eagerly to keep the gate before the early return.
async fn finish_frozen_lockfile_only<Reporter: self::Reporter + 'static>(
    lockfile: &Lockfile,
    config: &Config,
    finish: LockfileOnlyFrozen<'_, '_>,
) -> Result<(), InstallError> {
    if let Some(lockfile_verification_override) = finish.lockfile_verification_override {
        lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
    } else {
        verify_lockfile_eagerly::<Reporter>(
            lockfile,
            finish.resolution_verifiers,
            finish.derived_lockfile_path,
            &config.cache_dir,
        )
        .await?;
    }
    if config.lockfile {
        lockfile
            .save_to_path(&finish.workspace_root.join(config.wanted_lockfile_name()))
            .map_err(InstallError::SaveWantedLockfile)?;
    }
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: finish.prefix.to_string(),
        stage: Stage::ImportingDone,
    }));
    Ok(())
}

/// `project_manifests` with `packageExtensions` applied — pnpm's built-in
/// compatibility set and the user's, in that order, matching what the
/// resolver hands the rest of the install.
///
/// Empty when no extension applies, so the caller keeps using the manifests
/// it read from disk rather than a set of identical clones.
fn extend_project_manifests(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Result<Vec<(PathBuf, PackageManifest)>, InstallError> {
    let compat_extender = (!config.ignore_compatibility_db)
        .then(crate::compat_package_extensions::compat_package_extender);
    let extender = match config.package_extensions.as_ref() {
        Some(extensions) => crate::PackageExtender::new(extensions)
            .map(|extender| (!extender.is_empty()).then_some(extender))
            .map_err(InstallError::InvalidPackageExtensionSelector)?,
        None => None,
    };
    let selects = |manifest: &PackageManifest| {
        compat_extender.is_some_and(|extender| extender.matches(manifest.value()))
            || extender.as_ref().is_some_and(|extender| extender.matches(manifest.value()))
    };
    // A workspace project is rarely named by an extension — pnpm's
    // compatibility set names published packages — so this usually finds
    // nothing and the caller keeps the manifests it read from disk.
    if !project_manifests.iter().any(|(_, manifest)| selects(manifest)) {
        return Ok(Vec::new());
    }
    Ok(project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            let mut extended = (*manifest).clone();
            if let Some(compat_extender) = compat_extender {
                compat_extender.apply(extended.value_mut());
            }
            if let Some(extender) = extender.as_ref() {
                extender.apply(extended.value_mut());
            }
            (project_dir.clone(), extended)
        })
        .collect())
}
