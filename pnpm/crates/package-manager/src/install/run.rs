use super::{
    ApplyMaterializationInputs, Arc, AtomicU8, ContextLog, DependencyGroup,
    FastUpdateLockfileOptions, FreshnessCheckError, FreshnessScope, HashSet, Host,
    InMemoryPackageMetaCache, IncludedDependencies, Install, InstallError, InstallRunOptions,
    IsTerminal, Lockfile, LogEvent, LogLevel, MaterializationInputs, MaterializationOutput,
    OptimisticRepeatInstallCheck, OptimisticRepeatInstallDecision, PackageManifest, Path, PathBuf,
    PnpmLog, PrepareModulesStateInputs, PreparedModulesState, Reporter, ScopeLog, Stage, StageLog,
    SummaryLog, UpdateSeedPolicy, apply_materialization_result, build_project_manifests_list,
    build_resolution_verifiers, build_root_importer_project_manifests_list,
    build_selected_project_manifests_list, check_lockfile_freshness,
    check_optimistic_repeat_install, configured_or_discovered_workspace_dir,
    dev_preinstall_already_ran, emit_initial_package_manifest,
    get_catalogs_from_workspace_manifest, gvs_build_marker_present,
    gvs_build_markers_may_require_recovery, load_workspace_projects, lockfile_root_dir,
    map_frozen_lockfile_error, materialize, prepare_modules_state, prune_merged_branch_lockfile,
    run_dev_preinstall, selected_manifest_freshness_inputs, try_fast_update_lockfile,
    unapproved_recorded_ignored_builds, verify_lockfile_eagerly,
};
use pnpm_config::Config;
use pnpm_executor::DEV_PREINSTALL_STAGE;
use pnpm_store_dir::VerifiedFileIntegrity;

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    /// Runs the install, then deletes the per-branch lockfiles it has
    /// just folded into the wanted lockfile.
    ///
    /// The cleanup lives out here because every success path of
    /// [`Self::run_inner_impl`] — including the short-circuits that do
    /// nothing but rewrite the lockfile — has to leave them gone.
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
        Box::pin(self.run_inner_impl::<Reporter>(options)).await?;
        if let Some(lockfile_dir) = branch_lockfiles_to_clean {
            Lockfile::clean_git_branch_lockfiles(&lockfile_dir)
                .map_err(InstallError::CleanGitBranchLockfiles)?;
        }
        Ok(())
    }

    async fn run_inner_impl<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
    ) -> Result<(), InstallError> {
        let InstallRunOptions {
            lockfile_verification_override,
            rebuild,
            selection,
            root_manifest_as_workspace_root,
            deploy_manifest_hook,
            lockfile_specifier_project_manifests,
            read_package_hooked_manifest_paths,
            save_lockfile,
            lockfile_check: _,
            manifest_spec_bumps,
            prompt_eligibility_override,
        } = options;
        let Install {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            emit_initial_manifest,
            lockfile,
            lockfile_path,
            dependency_groups,
            frozen_lockfile,
            prefer_frozen_lockfile,
            ignore_manifest_check,
            skip_runtimes,
            trust_lockfile,
            update_checksums,
            mutation,
            installs_only,
            supported_architectures,
            node_linker,
            lockfile_only,
            dry_run,
            persist_policy_excludes,
            update_seed_policy,
            preferred_versions_override,
            auth_override,
            resolution_observer,
            peer_issues_sink,
            deps_requiring_build_sink,
            catalogs_override,
            disable_optimistic_repeat_install,
            pnpmfile_hook_override,
            workspace_projects_override,
        } = self;
        let effective_node_version = super::effective_node_version(config, manifest);
        http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        http_client_arc.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let can_prompt = prompt_eligibility_override
            .unwrap_or_else(|| !is_ci::cached() && std::io::stdin().is_terminal());
        let peer_issues_sink_is_none = peer_issues_sink.is_none();
        // Taken before any fetching so the store-verification figures
        // this install reports are its own — a recursive workspace run
        // and a long-lived embedder (the NAPI addon) both drive several
        // installs through the same process-global tally.
        let verified_file_integrity_baseline = VerifiedFileIntegrity::snapshot();

        // `--lockfile-only` with `lockfile: false` (pnpm's
        // `useLockfile: false`) is a config conflict: the only output the
        // flag produces is the lockfile, and that write is disabled.
        // Fail fast rather than run a resolve that writes nothing.
        if lockfile_only && !config.lockfile {
            return Err(InstallError::ConfigConflictLockfileOnlyWithNoLockfile);
        }

        // `enableModulesDir: false` (with the global virtual store off) is
        // "resolve and write the lockfile, materialize nothing" — the same
        // pipeline `--lockfile-only` takes, entered from config. It stays
        // outside the `lockfile: false` conflict above (pnpm accepts that
        // combination and simply writes nothing), and never turns a
        // rebuild — which runs against an already-materialized
        // `node_modules` — into a silent no-op.
        let lockfile_only = lockfile_only
            || (rebuild.is_none()
                && !config.enable_modules_dir
                && !config.enable_global_virtual_store);

        // `--dry-run` resolves but never materializes, so it borrows the
        // lockfile-only plumbing (skip node_modules / `.modules.yaml` /
        // workspace-state) while additionally skipping the lockfile write.
        // Both lockfile-only paths must stop after writing the wanted lockfile:
        // neither may write `.modules.yaml`, the current lockfile, or workspace state.
        // The frozen path returns below; the fresh path returns in `complete_resolve_only`.
        let resolve_only = lockfile_only || dry_run;

        if config.frozen_store && config.force {
            return Err(InstallError::ConfigConflictFrozenStoreWithForce);
        }

        if config.virtual_store_only
            && !config.enable_modules_dir
            && !config.enable_global_virtual_store
        {
            return Err(InstallError::ConfigConflictVirtualStoreOnlyWithNoModulesDir);
        }

        let prefer_frozen_lockfile =
            prefer_frozen_lockfile.unwrap_or(config.prefer_frozen_lockfile);

        // Collect once so the same set drives both the install dispatch
        // and the `included` field of `.modules.yaml` written below.
        // This is the same set the dependency-graph walker observes.
        let dependency_groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
        let included = IncludedDependencies {
            dependencies: dependency_groups.contains(&DependencyGroup::Prod),
            dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
            optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
        };

        // Project root for the [bunyan]-envelope `prefix`. This is
        // emitted as `lockfileDir`, the directory containing
        // `pnpm-lock.yaml`. With workspace support that equals the
        // workspace root — pacquet finds it via [`find_workspace_dir`].
        // Falls back to the manifest's parent dir when no
        // `pnpm-workspace.yaml` exists in any ancestor (the
        // single-project case). Closes pnpm/pacquet#357.
        //
        // [bunyan]: <https://github.com/trentm/node-bunyan>
        let manifest_dir = manifest.path().parent().expect("manifest path always has a parent dir");
        let workspace_dir_opt = configured_or_discovered_workspace_dir(config, manifest_dir)
            .map_err(InstallError::FindWorkspaceDir)?;
        let workspace_manifest_dir =
            workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf());
        // Catalogs and workspace packages still come from the real
        // workspace dir (`workspace_dir_opt`), which `lockfile_root_dir`
        // parts ways with under `sharedWorkspaceLockfile: false`.
        let workspace_root =
            lockfile_root_dir(config, manifest_dir).map_err(InstallError::FindWorkspaceDir)?;

        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pnpm_workspace::read_workspace_manifest(dir)
                .map_err(InstallError::ReadWorkspaceManifest)?,
            None => None,
        };
        let catalog_context_present = catalogs_override.is_some()
            || config.catalogs.is_some()
            || (!config.ignore_workspace && workspace_dir_opt.is_some());
        // Prefer a caller-supplied in-memory catalogs set
        // (`catalogs_override`, e.g. `pacquet update --latest --no-save`
        // resolving a bumped `catalog:` entry that is not written to disk),
        // then catalogs an `updateConfig` pnpmfile hook produced
        // (`config.catalogs`, the complete set after the hook pass), and
        // finally the raw workspace-manifest read. `None` at every layer
        // falls back to the manifest, mirroring pnpm's post-`updateConfig`
        // `config.catalogs`.
        let catalogs = match catalogs_override.or_else(|| config.catalogs.clone()) {
            Some(catalogs) => catalogs,
            None => get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
                .map_err(InstallError::InvalidCatalogsConfiguration)?,
        };
        // Use `to_string_lossy` rather than `to_str().expect(...)` so a
        // valid filesystem path with non-UTF-8 bytes (possible on Unix)
        // doesn't panic the installer. `prefix` is used only for
        // reporter envelopes, so a lossy conversion is acceptable —
        // the rest of the install path uses the same pattern for
        // paths threaded into log events.
        let prefix = workspace_root.to_string_lossy().into_owned();

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
        let workspace_projects_are_overridden = workspace_projects_override.is_some();
        let loaded_workspace_projects = match (selection.as_ref(), workspace_projects_override) {
            (Some(_), _) => None,
            (None, Some(projects)) => Some(projects),
            (None, None) => load_workspace_projects(
                workspace_dir_opt.as_deref().unwrap_or(&workspace_root),
                workspace_manifest.as_ref(),
            )
            .map_err(InstallError::FindWorkspaceProjects)?,
        };
        let workspace_projects = selection.as_ref().map_or_else(
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
        if selection.is_none() && config.shares_one_lockfile() {
            let workspace_wide = mutation.is_full_install().then_some(workspace_projects).flatten();
            Reporter::emit(&LogEvent::Scope(ScopeLog {
                level: LogLevel::Debug,
                selected: workspace_wide.map_or(1, <[_]>::len),
                total: workspace_wide.map(<[_]>::len),
                workspace_prefix: workspace_dir_opt
                    .as_deref()
                    .map(|dir| dir.to_string_lossy().into_owned()),
            }));
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

        let project_manifests = match selection.as_ref() {
            Some(selection) => build_selected_project_manifests_list(
                manifest,
                selection.all_projects,
                selection.active_manifest_is_standin,
            ),
            None if root_manifest_as_workspace_root => {
                build_root_importer_project_manifests_list(&workspace_root, manifest, None)
            }
            None if workspace_projects_are_overridden || !config.shares_one_lockfile() => {
                build_root_importer_project_manifests_list(
                    &workspace_root,
                    manifest,
                    // Dedicated per-project lockfiles record a single "."
                    // importer per project; sibling projects only feed the
                    // `workspace:` resolver, never the importer list.
                    config.shares_one_lockfile().then_some(workspace_projects).flatten(),
                )
            }
            None => build_project_manifests_list(manifest, workspace_projects),
        };
        let install_importer_ids = selection.as_ref().map(|selection| {
            selection
                .install_dirs
                .iter()
                .map(|project_dir| {
                    pnpm_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
                })
                .collect::<HashSet<_>>()
        });
        let selected_importer_ids = selection.as_ref().map(|selection| {
            selection
                .selected_dirs
                .iter()
                .map(|project_dir| {
                    pnpm_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
                })
                .collect::<HashSet<_>>()
        });
        let real_importer_ids = project_manifests
            .iter()
            .map(|(project_dir, _)| {
                pnpm_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
            })
            .collect::<HashSet<_>>();
        let filtered_install = selected_importer_ids
            .as_ref()
            .is_some_and(|selected_importer_ids| selected_importer_ids != &real_importer_ids);
        let requested_importer_ids = if filtered_install { install_importer_ids } else { None };
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
        let prune_stale_importers = !filtered_install
            && mutation.is_full_install()
            && workspace_projects.is_some()
            && !workspace_projects_are_overridden
            && config.shares_one_lockfile();
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
        let optimistic_decision = mutation.is_full_install()
            && matches!(update_seed_policy, UpdateSeedPolicy::KeepAll)
            && !frozen_lockfile
            && !config.force
            && !disable_optimistic_repeat_install
            && check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
                workspace_root: &workspace_root,
                config,
                node_linker,
                included,
                supported_architectures: supported_architectures.as_ref(),
                project_manifests: &project_manifests,
                is_workspace_install: workspace_manifest.is_some(),
                lockfile,
                catalogs: &catalogs,
            }) == OptimisticRepeatInstallDecision::UpToDate;
        if optimistic_decision {
            // Keep `strictDepBuilds` enforced across reruns: an install
            // that already recorded unapproved ignored builds must keep
            // failing until they are approved, not exit 0 via the fast
            // path. An `allowBuilds` change that newly permits one is
            // already caught by `settings_match` (the policy is part of
            // the workspace state), which reports drift and skips this
            // branch, so the full install runs and rebuilds it.
            //
            // A corrupt / unreadable `.modules.yaml` can't prove there are
            // no recorded ignored builds, so under strict mode fall through
            // to the full install rather than short-circuiting on a
            // swallowed read error.
            let marker_safe = if gvs_build_markers_may_require_recovery(config) {
                match lockfile.get() {
                    Ok(Some(wanted)) => !gvs_build_marker_present(
                        wanted,
                        config,
                        &workspace_root,
                        effective_node_version.as_deref(),
                    ),
                    Ok(None) => true,
                    Err(_) => false,
                }
            } else {
                true
            };
            let strict_builds_safe = if config.strict_dep_builds {
                match pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir) {
                    Ok(Some(modules)) => match unapproved_recorded_ignored_builds(&modules, config)
                    {
                        Ok(Some(package_names)) => {
                            return Err(InstallError::IgnoredBuilds { package_names });
                        }
                        Ok(None) => true,
                        // Unreadable state or a malformed `allowBuilds`:
                        // can't trust the fast path, run the full install.
                        Err(_) => false,
                    },
                    Ok(None) => true,
                    Err(_) => false,
                }
            } else {
                true
            };
            if marker_safe && strict_builds_safe {
                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Info,
                    message: "Already up to date".to_string(),
                    prefix: prefix.clone(),
                }));
                Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
                return Ok(());
            }
        }

        // Past the fast path every install flavor reads the wanted
        // lockfile; start its read + parse on a background thread so it
        // overlaps the cycle check below. The forced load further down
        // joins it. (A run the pipeline knew would get here — frozen /
        // forced — started this prefetch before project discovery, and
        // this call is then a no-op.)
        lockfile.prefetch();

        // Report the projects this install covers depending on each
        // other in a cycle — after the short-circuit above, because pnpm
        // returns from "Already up to date" before reaching its own
        // check, and before any resolution, because a
        // `disallowWorkspaceCycles` failure must not be paid for.
        if !config.ignore_workspace_cycles
            && let Some(workspace_dir) = workspace_dir_opt.as_deref()
        {
            let scope = match selection.as_ref() {
                // A plan that already sequenced this very graph hands
                // its cycle report over; the install then skips
                // rebuilding the graph just to find them again.
                Some(selection) => match selection.workspace_cycles {
                    crate::PrecomputedWorkspaceCycles::Known(cycles) => {
                        crate::report_workspace_cycles::<Reporter>(config, workspace_dir, cycles)
                            .map_err(InstallError::CyclicWorkspaceDependencies)?;
                        None
                    }
                    crate::PrecomputedWorkspaceCycles::Unknown => {
                        Some((selection.all_projects, Some(selection.selected_dirs)))
                    }
                },
                // A single-project mutation (`add`, `update`, ...) has no
                // set to cycle within; only a full install covers the
                // whole workspace.
                None => mutation
                    .is_full_install()
                    .then_some(workspace_projects)
                    .flatten()
                    .map(|projects| (projects, None)),
            };
            if let Some((projects, selected_dirs)) = scope {
                let cycles = crate::install_scope_cycles(config, projects, selected_dirs);
                crate::report_workspace_cycles::<Reporter>(
                    config,
                    workspace_dir,
                    cycles.as_deref(),
                )
                .map_err(InstallError::CyclicWorkspaceDependencies)?;
            }
        }

        // Read the *current* lockfile (`<virtual_store_dir>/lock.yaml`)
        // off the reactor while the wanted lockfile parses on this
        // task: both are megabyte-scale YAML documents on a large
        // workspace, and neither read depends on the other. The result
        // is consumed further down, where the install dispatch needs
        // it.
        let current_lockfile_task = tokio::task::spawn_blocking({
            let virtual_store_dir = config.virtual_store_dir.clone();
            move || Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        });

        // Past the repeat-install fast path every install flavor needs
        // the wanted lockfile's contents; force the deferred load here.
        // A broken lockfile is regenerable state, so only a frozen
        // install treats it as fatal (upstream `readLockfiles`).
        let phase_start = std::time::Instant::now();
        let lockfile_source = lockfile;
        // The fold's "before" is read out here rather than at its use site
        // below: a load that failed leaves nothing cached, so asking later
        // would retry it and turn a lockfile this arm chose to ignore into
        // a fatal one.
        let (lockfile, lockfile_shared, merge_wanted_lockfile, pre_merge_importers) =
            match lockfile_source.get() {
                Ok(lockfile) => (
                    lockfile,
                    lockfile_source.shared().map_err(InstallError::LoadWantedLockfile)?,
                    lockfile_source.get_for_merge().map_err(InstallError::LoadWantedLockfile)?,
                    lockfile_source
                        .pre_merge_importers()
                        .map_err(InstallError::LoadWantedLockfile)?,
                ),
                Err(error) if !frozen_lockfile => {
                    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                        level: LogLevel::Warn,
                        message: format!(
                            "Ignoring broken lockfile at {}: {error}",
                            workspace_root.display(),
                        ),
                        prefix: prefix.clone(),
                    }));
                    (None, None, None, None)
                }
                Err(error) => return Err(InstallError::LoadWantedLockfile(error)),
            };
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "load_wanted_lockfile",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Spawn the installability host detection (`node --version`,
        // ~150 ms of node startup) as soon as the wanted lockfile is
        // parsed, so the probe overlaps planning on the frozen path and
        // the whole resolution on the fresh path. A constraint-free
        // lockfile spawns nothing — the probe's result would go unused
        // (see `detect_installability_host` for why that matters) —
        // and neither does `--force` (skips the checks) or a
        // resolve-only pass (returns before them). The scan is of the
        // *wanted* lockfile: a fresh resolve whose new graph gains
        // constraints the old lockfile lacked just detects the host at
        // its own site, as before.
        let early_host_detection = (!config.force
            && !resolve_only
            && lockfile.is_some_and(|lockfile| match (&lockfile.snapshots, &lockfile.packages) {
                (Some(snapshots), Some(packages)) if !snapshots.is_empty() => {
                    pnpm_deps_restorer::any_installability_constraint(snapshots, packages)
                }
                _ => false,
            }))
        .then(|| {
            pnpm_deps_restorer::materialization_plan::HostDetection::spawn(
                config.engine_strict,
                super::effective_node_version(config, manifest),
                supported_architectures.clone(),
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
        if config.enable_global_virtual_store {
            // Create the store root before calling `register_project` so
            // its `path_contains` guard can canonicalize the path
            // instead of falling through to a literal comparison that
            // wrongly matches against `<workspace>/../pacquet-store/v11`-
            // shaped relative store paths (resolved-on-disk: outside the
            // workspace; lexical: starts with the workspace prefix).
            if let Err(error) =
                std::fs::create_dir_all(pnpm_store_dir::StoreDir::root(&config.store_dir))
            {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "Failed to ensure store root exists before project registry write; install continues",
                );
            }
            if let Err(error) = pnpm_store_dir::register_project(&config.store_dir, &workspace_root)
            {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "Failed to register workspace root in the store project registry; install continues",
                );
            }
        }

        // `pnpm:package-manifest initial` carries the on-disk
        // `package.json` body for this importer. Fires before
        // `pnpm:context` so consumers that key off manifest contents
        // have it ready when the install header renders.
        if emit_initial_manifest {
            emit_initial_package_manifest::<Reporter>(manifest);
        }

        // The pnpmfile whose checksum the freshness gates compare
        // against a lockfile's `pnpmfileChecksum`, resolved the way the
        // install that records one resolves it. Building the handle
        // costs a `stat`. The Node worker only starts if a gate has to
        // ask whether the pnpmfile exports hooks. The handle is handed to
        // the resolve path below so an install spawns at most one.
        let pnpmfile_hook = match pnpmfile_hook_override {
            Some(hook) => Some(hook),
            None if config.ignore_pnpmfile => None,
            None => pnpm_hooks::finder::load_pnpmfiles(
                &workspace_root,
                crate::pnpmfile_selection(config),
            )
            .map_err(InstallError::MissingPnpmfile)?,
        };

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
        let extended_project_manifests: Vec<(PathBuf, PackageManifest)> =
            extend_project_manifests(config, &project_manifests)?;
        let project_manifests: Vec<(PathBuf, &PackageManifest)> =
            if extended_project_manifests.is_empty() {
                project_manifests
            } else {
                extended_project_manifests
                    .iter()
                    .map(|(project_dir, manifest)| (project_dir.clone(), manifest))
                    .collect()
            };
        let read_package_log = pnpmfile_hook.as_ref().map(|hook| {
            hook.source_path().map_or_else(
                || Arc::new(|_| {}) as pnpm_hooks::LogFn,
                |from| {
                    crate::install_with_fresh_lockfile::hook_log_fn::<Reporter>(
                        &workspace_root,
                        from,
                        "readPackage",
                    )
                },
            )
        });
        let every_project_manifest_is_pre_hooked = project_manifests
            .iter()
            .all(|(_, manifest)| read_package_hooked_manifest_paths.contains(manifest.path()));
        let hooked_project_manifests: Vec<(PathBuf, PackageManifest)> =
            match (pnpmfile_hook.as_ref(), read_package_log.as_ref()) {
                (Some(hook), Some(log)) if !every_project_manifest_is_pre_hooked => {
                    futures_util::future::try_join_all(project_manifests.iter().map(
                        |(project_dir, manifest)| {
                            let ctx = pnpm_hooks::HookContext { log: Arc::clone(log), dir: None };
                            let pre_hooked =
                                read_package_hooked_manifest_paths.contains(manifest.path());
                            async move {
                                if pre_hooked {
                                    return Ok((project_dir.clone(), (*manifest).clone()));
                                }
                                let value = hook
                                    .read_package(manifest.value().clone(), ctx)
                                    .await
                                    .map_err(InstallError::ReadPackageHook)?;
                                let mut hooked = (*manifest).clone();
                                *hooked.value_mut() = (*value).clone();
                                Ok::<_, InstallError>((project_dir.clone(), hooked))
                            }
                        },
                    ))
                    .await?
                }
                _ => Vec::new(),
            };
        let project_manifests: Vec<(PathBuf, &PackageManifest)> =
            if hooked_project_manifests.is_empty() {
                project_manifests
            } else {
                hooked_project_manifests
                    .iter()
                    .map(|(project_dir, manifest)| (project_dir.clone(), manifest))
                    .collect()
            };
        let manifest_freshness_inputs = match selection.as_ref() {
            Some(selection) => selected_manifest_freshness_inputs(
                &workspace_root,
                &project_manifests,
                selection.install_dirs,
            ),
            None => project_manifests
                .iter()
                .map(|(project_dir, manifest)| {
                    (
                        pnpm_workspace::importer_id_from_root_dir(&workspace_root, project_dir),
                        *manifest,
                    )
                })
                .collect(),
        };

        // Load the *current* lockfile that records what the previous
        // install actually materialized in `<virtual_store_dir>/lock.yaml`.
        // The frozen-lockfile path diffs each wanted snapshot against
        // this on a per-`PackageKey` basis to decide whether the
        // already-installed slot is still usable. `Ok(None)` on a
        // first install (the file doesn't exist yet). A corrupted /
        // version-incompatible file is disposable state: pnpm warns and
        // continues with an empty current lockfile because the wanted
        // lockfile and filesystem remain authoritative.
        let phase_start = std::time::Instant::now();
        let current_lockfile =
            match current_lockfile_task.await.expect("join the current-lockfile load task") {
                Ok(lockfile) => lockfile,
                Err(error) => {
                    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                        level: LogLevel::Warn,
                        message: format!(
                            "Ignoring broken lockfile at {}: {error}",
                            config.virtual_store_dir.display(),
                        ),
                        prefix: prefix.clone(),
                    }));
                    None
                }
            };
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "load_current_lockfile",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Synthesize the wanted lockfile from `<virtual_store_dir>/lock.yaml`
        // when `pnpm-lock.yaml` is absent and the materialized snapshot still
        // satisfies the manifest. The install then skips resolution and
        // regenerates `pnpm-lock.yaml` from the synthesized object.
        let synthesized_lockfile: Option<Lockfile> = match current_lockfile.as_ref() {
            Some(current) if lockfile.is_none() && !frozen_lockfile && prefer_frozen_lockfile => {
                check_lockfile_freshness(
                    current,
                    &manifest_freshness_inputs,
                    config,
                    &catalogs,
                    pnpmfile_hook.as_ref(),
                    FreshnessScope {
                        ignore_manifest_check,
                        allow_missing_dependency_free_importers: true,
                        prune_stale_importers,
                    },
                )
                .await
                .ok()
                .map(|()| current.clone())
            }
            _ => None,
        };
        let lockfile_synthesized_from_current = synthesized_lockfile.is_some();
        // The dry-run diff baseline is the actual on-disk `pnpm-lock.yaml`
        // (`None` when it is absent), captured before the synthesized-from-
        // current fallback below. Diffing against the synthesized lockfile
        // would hide the change of a real install creating `pnpm-lock.yaml`.
        let existing_wanted_lockfile = lockfile;
        let lockfile = lockfile.or(synthesized_lockfile.as_ref());
        // The branch lockfiles were folded in at load, before any manifest
        // was known. Reconcile the fold against them now, while every
        // later stage — the fast update, the freshness check, and the
        // rewrite the merge is saved by — still reads the same object.
        let merged_branch_lockfile = match (pre_merge_importers, lockfile) {
            (Some(pre_merge_importers), Some(lockfile)) => prune_merged_branch_lockfile(
                lockfile,
                pre_merge_importers,
                &manifest_freshness_inputs,
                config.auto_install_peers,
            ),
            _ => None,
        };
        let lockfile = merged_branch_lockfile.as_ref().or(lockfile);
        let can_fast_update_lockfile = !frozen_lockfile
            && !dry_run
            && prefer_frozen_lockfile
            && mutation.may_fast_update_lockfile();
        let fast_updated_lockfile = if can_fast_update_lockfile {
            try_fast_update_lockfile::<Reporter>(FastUpdateLockfileOptions {
                lockfile,
                manifests: &manifest_freshness_inputs,
                project_manifests: &project_manifests,
                config,
                catalogs: &catalogs,
                pnpmfile_hook: pnpmfile_hook.as_ref(),
                ignore_manifest_check,
                prune_stale_importers,
            })
            .await
        } else {
            None
        };
        let lockfile_was_fast_updated = fast_updated_lockfile.is_some();
        let lockfile = fast_updated_lockfile.as_ref().or(lockfile);

        // One per-install packument cache shared with both the
        // lockfile-verifier (below) and the resolver in
        // `install_with_fresh_lockfile` (further down). The
        // single instance lets a name the resolver fetched during this
        // install short-circuit the verifier's own fetch chain, and
        // vice versa.
        let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
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
        let planned_canonical_fetches =
            pnpm_resolving_resolver_base::PlannedCanonicalFetches::default();
        let resolution_verifiers = if trust_lockfile {
            Vec::new()
        } else {
            build_resolution_verifiers(
                config,
                Arc::clone(&http_client_arc),
                Some(Arc::clone(&meta_cache)
                    as Arc<dyn pnpm_resolving_npm_resolver::PackageMetaCache>),
                auth_override.clone(),
                None,
                Some(std::sync::Arc::clone(&planned_canonical_fetches)),
            )
            .map_err(InstallError::BuildVerifiers)?
        };
        let derived_lockfile_path = lockfile.map(|_| {
            lockfile_path.map_or_else(
                || workspace_root.join(config.wanted_lockfile_name()),
                Path::to_path_buf,
            )
        });

        // `@pnpm/cli.default-reporter` renders these fields in the install header;
        // `currentLockfileExists` flips after the virtual-store lockfile is written.
        Reporter::emit(&LogEvent::Context(ContextLog {
            level: LogLevel::Debug,
            current_lockfile_exists: current_lockfile.is_some(),
            store_dir: config.store_dir.display().to_string(),
            virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
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
        if !config.ignore_scripts
            && !resolve_only
            && !ignore_manifest_check
            && rebuild.is_none()
            && !dev_preinstall_already_ran()
        {
            // pnpm reads the hook off the root project's in-memory
            // manifest and only shells out when it is defined. Falling
            // back to the executor's own read covers a root that isn't
            // among the importers, as a filtered install's is not —
            // pnpm's `safeReadProjectManifestOnly` fallback.
            let normalized_root = pnpm_fs::lexical_normalize(&workspace_root);
            let root_defines_hook = project_manifests
                .iter()
                .find(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir) == normalized_root)
                .is_none_or(|(_, manifest)| {
                    matches!(manifest.script(DEV_PREINSTALL_STAGE, true), Ok(Some(_)))
                });
            if root_defines_hook {
                run_dev_preinstall::<Reporter>(config, &workspace_root)?;
            }
        }

        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: prefix.clone(),
            stage: Stage::ImportingStarted,
        }));

        // Install-scoped dedupe state for `pnpm:package-import-method`.
        // Threaded down to `link_file::log_method_once` so each install
        // emits the channel afresh — a per-importer capture rather than
        // a process-static.
        let logged_methods = AtomicU8::new(0);

        tracing::info!(target: "pacquet::install", "Start all");

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
        if update_checksums && frozen_lockfile {
            return Err(InstallError::FrozenLockfileWithUpdateChecksums);
        }

        // Compute the dispatch decision once. `take_frozen_path` is true
        // for both state 1 (--frozen-lockfile) and state 2 (auto-frozen
        // via prefer-frozen-lockfile). The freshness check fires for both
        // — fatal for state 1, fall-through for state 2.
        //
        // `--dry-run` always takes the fresh-resolve path: it must compute
        // the would-be lockfile to diff against the existing one, and the
        // frozen freshness gate would otherwise abort on a stale lockfile
        // instead of reporting the change.
        let take_frozen_path = if dry_run {
            false
        } else if frozen_lockfile {
            let Some(lockfile) = lockfile else {
                return Err(InstallError::NoLockfile);
            };
            // Run the freshness gates; on failure surface a fatal
            // InstallError via `FreshnessCheckError`'s `From` impl.
            // The check is run for its side effect (the typed
            // outcome) — the borrowed lockfile / manifests are consumed
            // again inside the frozen branch below.
            check_lockfile_freshness(
                lockfile,
                &manifest_freshness_inputs,
                config,
                &catalogs,
                pnpmfile_hook.as_ref(),
                FreshnessScope {
                    ignore_manifest_check,
                    allow_missing_dependency_free_importers: false,
                    // pnpm's importer-set gate sits in the auto-frozen branch
                    // of `isFrozenInstallPossible`, which an explicit
                    // `--frozen-lockfile` short-circuits past, so a removed
                    // project does not fail the install there.
                    prune_stale_importers: false,
                },
            )
            .await
            .map_err(InstallError::from)?;
            true
        } else if update_checksums {
            false
        } else if let Some(lockfile) = lockfile {
            // Auto-frozen via `preferFrozenLockfile`. Skip when the
            // user opted out (`--no-prefer-frozen-lockfile` /
            // `preferFrozenLockfile: false`); otherwise consult the
            // freshness gate. A `Stale` / `NoImporter` outcome routes
            // to the fresh-resolve path; a malformed
            // `pnpm.overrides` is a user-config error that surfaces
            // regardless of dispatch.
            if prefer_frozen_lockfile {
                match check_lockfile_freshness(
                    lockfile,
                    &manifest_freshness_inputs,
                    config,
                    &catalogs,
                    pnpmfile_hook.as_ref(),
                    FreshnessScope {
                        ignore_manifest_check,
                        allow_missing_dependency_free_importers: true,
                        prune_stale_importers,
                    },
                )
                .await
                {
                    // Even an up-to-date lockfile may not go frozen: a
                    // custom resolver's `shouldRefreshResolution` can
                    // force the fresh-resolve path. The hook's verdict
                    // blocks the frozen install. A lockfile
                    // synthesized from the current snapshot skips the
                    // check (it only gates on a non-empty wanted
                    // lockfile). A throwing hook aborts the install.
                    Ok(()) => {
                        lockfile_synthesized_from_current
                            || config.ignore_pnpmfile
                            || !crate::check_custom_resolver_force_resolve::force_resolve_from_pnpmfile(
                                lockfile,
                                pnpmfile_hook.as_deref(),
                            )
                            .await
                            .map_err(InstallError::CustomResolverForceResolve)?
                    }
                    Err(
                        error @ (FreshnessCheckError::Stale(_)
                        | FreshnessCheckError::NoImporter { .. }),
                    ) => {
                        tracing::info!(
                            target: "pacquet::install",
                            reason = %error,
                            "lockfile not usable as-is; falling through to a fresh resolve",
                        );
                        false
                    }
                    Err(
                        error @ (FreshnessCheckError::InvalidOverrides(_)
                        | FreshnessCheckError::CalcPatchHashes(_)),
                    ) => {
                        return Err(error.into());
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        if lockfile_only && take_frozen_path {
            let lockfile = lockfile.expect("frozen dispatch verified lockfile is present");
            // This path materializes nothing, so there's no fetch to overlap;
            // verify eagerly to keep the gate before the early return.
            if let Some(lockfile_verification_override) = lockfile_verification_override {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
            } else {
                verify_lockfile_eagerly::<Reporter>(
                    lockfile,
                    &resolution_verifiers,
                    derived_lockfile_path.as_deref(),
                    &config.cache_dir,
                )
                .await?;
            }
            if config.lockfile {
                lockfile
                    .save_to_path(&workspace_root.join(config.wanted_lockfile_name()))
                    .map_err(InstallError::SaveWantedLockfile)?;
            }
            Reporter::emit(&LogEvent::Stage(StageLog {
                level: LogLevel::Debug,
                prefix: prefix.clone(),
                stage: Stage::ImportingDone,
            }));
            Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
            return Ok(());
        }

        let Some(PreparedModulesState {
            old_modules,
            previous_modules_metadata,
            is_inconsistent,
            lockfile_verification_override,
        }) = prepare_modules_state::<Reporter>(PrepareModulesStateInputs {
            resolve_only,
            take_frozen_path,
            config,
            filtered_install,
            installs_only,
            workspace_root: &workspace_root,
            included,
            current_lockfile: current_lockfile.as_ref(),
            requested_importer_ids: requested_importer_ids.as_ref(),
            node_linker,
            disable_optimistic_repeat_install,
            lockfile,
            supported_architectures: supported_architectures.as_ref(),
            rebuild: rebuild.as_ref(),
            resolution_verifiers: &resolution_verifiers,
            derived_lockfile_path: derived_lockfile_path.as_deref(),
            lockfile_verification_override,
            lockfile_synthesized_from_current,
            lockfile_was_fast_updated,
            save_lockfile,
            catalogs: &catalogs,
            project_manifests: &project_manifests,
            effective_node_version: effective_node_version.as_deref(),
            prefix: &prefix,
        })
        .await?
        else {
            return Ok(());
        };
        let modules_manifest = old_modules.as_ref();
        let prior_hoisted_dependencies =
            previous_modules_metadata.as_ref().map(|modules| &modules.hoisted_dependencies);
        let prune_orphans = !filtered_install;

        let MaterializationOutput {
            ignored_builds,
            deferred_builds,
            injected_deps,
            hoisted_dependencies,
            hoisted_locations,
            install_skipped,
            peer_issue_importer_ids,
            fresh_lockfile,
            store_index_teardown,
        } = materialize::<Reporter>(MaterializationInputs {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            // The dispatch above may have swapped `lockfile` for a
            // synthesized, branch-pruned, or fast-updated document; the
            // loader's handle is forwarded only while it still *is* the
            // lockfile, so a downstream consumer can never seed from a
            // superseded document.
            lockfile_shared: lockfile_shared.filter(|shared| {
                lockfile.is_some_and(|lockfile| std::ptr::eq(lockfile, Arc::as_ptr(shared)))
            }),
            merge_wanted_lockfile,
            take_frozen_path,
            lockfile_verification_override,
            resolution_verifiers,
            derived_lockfile_path,
            dependency_groups,
            project_manifests: &project_manifests,
            lockfile_specifier_project_manifests,
            workspace_projects,
            requested_importer_ids: requested_importer_ids.as_ref(),
            real_importer_ids: &real_importer_ids,
            workspace_root: &workspace_root,
            included,
            node_linker,
            rebuild: rebuild.as_ref(),
            ignore_manifest_check,
            mutation,
            current_lockfile: current_lockfile.as_ref(),
            supported_architectures: supported_architectures.as_ref(),
            early_host_detection,
            skip_runtimes,
            modules_manifest,
            prior_hoisted_dependencies,
            planned_canonical_fetches,
            prune_orphans,
            logged_methods: &logged_methods,
            update_checksums,
            meta_cache,
            resolve_only,
            dry_run,
            can_prompt,
            persist_policy_excludes,
            update_seed_policy,
            preferred_versions_override,
            auth_override,
            resolution_observer,
            peer_issues_sink,
            deps_requiring_build_sink,
            pnpmfile_hook,
            deploy_manifest_hook,
            save_lockfile,
            manifest_spec_bumps,
            catalogs: &catalogs,
            prefix: &prefix,
        })
        .await?;

        let phase_start = std::time::Instant::now();
        apply_materialization_result::<Reporter>(ApplyMaterializationInputs {
            resolve_only,
            dry_run,
            peer_issues_sink_is_none,
            existing_wanted_lockfile,
            peer_issue_importer_ids,
            fresh_lockfile,
            prefix,
            lockfile,
            requested_importer_ids,
            workspace_root,
            workspace_manifest_dir,
            included,
            install_skipped,
            node_linker,
            current_lockfile,
            real_importer_ids,
            project_manifests,
            filtered_install,
            is_inconsistent,
            previous_modules_metadata,
            config,
            hoisted_dependencies,
            hoisted_locations,
            injected_deps,
            ignored_builds,
            deferred_builds,
            modules_manifest: old_modules,
            rebuild,
            take_frozen_path,
            lockfile_synthesized_from_current,
            lockfile_was_fast_updated,
            save_lockfile,
            mutation,
            manifest_dir,
            selection,
            supported_architectures,
            catalogs,
            catalog_context_present,
            verified_file_integrity_baseline,
        })
        .await?;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "apply_materialization_result",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );

        // Only now wait out the store-index writer's teardown — its
        // final flush and the WAL checkpoint `SQLite` runs when the
        // connection closes (~40 ms of otherwise pure tail on a cold
        // install) have been overlapping every write above since the
        // install paths dropped their writer handles. An error path
        // that returned before this point dropped the handle instead,
        // detaching the task: an interrupted checkpoint is exactly the
        // crash case WAL recovery exists for.
        pnpm_store_dir::StoreIndexWriter::drain(
            store_index_teardown,
            "; some rows may not be persisted",
        )
        .await;
        Ok(())
    }
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
