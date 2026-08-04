use super::{
    Arc, AtomicU8, AuthHeaders, BTreeMap, BTreeSet, Catalogs, Config, ContextLog, DependencyGroup,
    DepsRequiringBuildSink, FastUpdateImporterLockfileOptions, FreshnessCheckError, HashSet,
    HoistedDependencies, Host, InMemoryPackageMetaCache, IncludedDependencies, Install,
    InstallError, InstallFrozenLockfile, InstallRunOptions, InstallWithFreshLockfile, IsTerminal,
    Lockfile, LogEvent, LogLevel, MemCache, Modules, NodeLinker, OptimisticRepeatInstallCheck,
    OptimisticRepeatInstallDecision, PackageManifest, Path, PathBuf, PeerIssuesSink, PnpmLog,
    ProjectMutation, ProjectScriptsInputs, RebuildOptions, Reporter, ResolutionVerifier,
    ResolvedPackages, ScopeLog, Stage, StageLog, SummaryLog, SystemTime, ThrottledClient,
    UpdateSeedPolicy, WorkspaceInstallSelection, build_modules_manifest,
    build_project_manifests_list, build_resolution_verifiers,
    build_root_importer_project_manifests_list, build_selected_project_manifests_list,
    build_workspace_packages_map, build_workspace_state, check_lockfile_freshness,
    check_modules_settings_diff, check_optimistic_repeat_install,
    configured_or_discovered_workspace_dir, dev_preinstall_already_ran, drain_settled_projects,
    emit_initial_package_manifest, frozen_tree_intact, get_catalogs_from_workspace_manifest,
    gvs_build_marker_present, gvs_build_markers_may_require_recovery,
    has_newly_allowed_ignored_builds, has_revoked_allowed_builds, load_workspace_projects,
    map_frozen_lockfile_error, merge_filtered_modules_metadata, merge_pending_builds,
    modules_consistent_with, modules_layout_consistent_with, node_version_from_engines_runtime,
    order_project_lifecycle_groups, project_requires_lifecycle_scripts,
    projects_running_own_scripts, record_lockfile_verified, run_dev_preinstall,
    run_projects_lifecycle_scripts, selected_manifest_freshness_inputs,
    try_fast_update_importer_lockfile, unapproved_recorded_ignored_builds, update_workspace_state,
    verify_lockfile_eagerly, write_modules_manifest,
};
use pacquet_executor::DEV_PREINSTALL_STAGE;

impl<'a, DependencyGroupList> Install<'a, DependencyGroupList>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    pub(super) async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        options: InstallRunOptions<'a, '_>,
    ) -> Result<(), InstallError> {
        let InstallRunOptions {
            lockfile_verification_override,
            rebuild,
            selection,
            root_manifest_as_workspace_root,
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
            update_seed_policy,
            auth_override,
            resolution_observer,
            peer_issues_sink,
            deps_requiring_build_sink,
            catalogs_override,
            disable_optimistic_repeat_install,
            pnpmfile_hook_override,
            workspace_projects_override,
        } = self;
        let can_prompt = prompt_eligibility_override
            .unwrap_or_else(|| !is_ci::cached() && std::io::stdin().is_terminal());
        let peer_issues_sink_is_none = peer_issues_sink.is_none();

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
        // Dedicated per-project lockfiles (`sharedWorkspaceLockfile:
        // false`) anchor everything `workspace_root` names — the wanted
        // lockfile, importer ids, reporter prefixes, the workspace-state
        // file — at the active project, mirroring pnpm's `lockfileDir =
        // sharedWorkspaceLockfile ? workspaceDir : projectDir`. Catalogs
        // and workspace packages still come from the real workspace dir
        // (`workspace_dir_opt`).
        let workspace_root = if config.shared_workspace_lockfile {
            workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf())
        } else {
            manifest_dir.to_path_buf()
        };

        let workspace_manifest = match workspace_dir_opt.as_deref() {
            Some(dir) => pacquet_workspace::read_workspace_manifest(dir)
                .map_err(InstallError::ReadWorkspaceManifest)?,
            None => None,
        };
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
        if selection.is_none() && config.shared_workspace_lockfile {
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
        let manifest_is_root_importer = root_manifest_as_workspace_root
            || workspace_projects_are_overridden
            || !config.shared_workspace_lockfile;
        let project_manifests = match selection.as_ref() {
            Some(selection) => build_selected_project_manifests_list(
                manifest,
                selection.all_projects,
                selection.active_manifest_is_standin,
            ),
            None if manifest_is_root_importer => build_root_importer_project_manifests_list(
                &workspace_root,
                manifest,
                // Dedicated per-project lockfiles record a single "."
                // importer per project; sibling projects only feed the
                // `workspace:` resolver, never the importer list.
                config.shared_workspace_lockfile.then_some(workspace_projects).flatten(),
            ),
            None => build_project_manifests_list(&workspace_root, manifest, workspace_projects),
        };
        let manifest_freshness_inputs = match selection.as_ref() {
            Some(selection) => selected_manifest_freshness_inputs(
                &workspace_root,
                &project_manifests,
                selection.selected_dirs,
            ),
            None => project_manifests
                .iter()
                .map(|(project_dir, manifest)| {
                    (
                        pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir),
                        *manifest,
                    )
                })
                .collect(),
        };
        let selected_importer_ids = selection.as_ref().map(|selection| {
            selection
                .selected_dirs
                .iter()
                .map(|project_dir| {
                    pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
                })
                .collect::<HashSet<_>>()
        });
        let real_importer_ids = project_manifests
            .iter()
            .map(|(project_dir, _)| {
                pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
            })
            .collect::<HashSet<_>>();
        let filtered_install = selected_importer_ids
            .as_ref()
            .is_some_and(|selected_importer_ids| selected_importer_ids != &real_importer_ids);
        let requested_importer_ids = if filtered_install { selected_importer_ids } else { None };
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
        let optimistic_decision = mutation.is_full_install()
            && matches!(update_seed_policy, UpdateSeedPolicy::KeepAll)
            && !filtered_install
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
                    Ok(Some(wanted)) => !gvs_build_marker_present(wanted, config, &workspace_root),
                    Ok(None) => true,
                    Err(_) => false,
                }
            } else {
                true
            };
            let strict_builds_safe = if config.strict_dep_builds {
                match pacquet_modules_yaml::read_modules_layout::<Host>(&config.modules_dir) {
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

        // Past the repeat-install fast path every install flavor needs
        // the wanted lockfile's contents; force the deferred load here.
        // A broken lockfile is regenerable state, so only a frozen
        // install treats it as fatal (upstream `readLockfiles`).
        let lockfile = match lockfile.get() {
            Ok(lockfile) => lockfile,
            Err(error) if !frozen_lockfile => {
                Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                    level: LogLevel::Warn,
                    message: format!(
                        "Ignoring broken lockfile at {}: {error}",
                        workspace_root.display(),
                    ),
                    prefix: prefix.clone(),
                }));
                None
            }
            Err(error) => return Err(InstallError::LoadWantedLockfile(error)),
        };

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
                std::fs::create_dir_all(pacquet_store_dir::StoreDir::root(&config.store_dir))
            {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "Failed to ensure store root exists before project registry write; install continues",
                );
            }
            if let Err(error) =
                pacquet_store_dir::register_project(&config.store_dir, &workspace_root)
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
        let pnpmfile_hook = pnpmfile_hook_override
            .or_else(|| pacquet_hooks::finder::load_pnpmfile(&workspace_root));

        // Load the *current* lockfile that records what the previous
        // install actually materialized in `<virtual_store_dir>/lock.yaml`.
        // The frozen-lockfile path diffs each wanted snapshot against
        // this on a per-`PackageKey` basis to decide whether the
        // already-installed slot is still usable. `Ok(None)` on a
        // first install (the file doesn't exist yet). A corrupted /
        // version-incompatible file is disposable state: pnpm warns and
        // continues with an empty current lockfile because the wanted
        // lockfile and filesystem remain authoritative.
        let current_lockfile =
            match Lockfile::load_current_from_virtual_store_dir(&config.virtual_store_dir) {
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
                    ignore_manifest_check,
                    true,
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
        let can_fast_update_importers =
            !frozen_lockfile && !dry_run && prefer_frozen_lockfile && mutation.is_full_install();
        let fast_updated_lockfile = if can_fast_update_importers {
            try_fast_update_importer_lockfile(FastUpdateImporterLockfileOptions {
                lockfile,
                manifests: &manifest_freshness_inputs,
                config,
                catalogs: &catalogs,
                pnpmfile_hook: pnpmfile_hook.as_ref(),
                ignore_manifest_check,
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
        let resolution_verifiers = if trust_lockfile {
            Vec::new()
        } else {
            build_resolution_verifiers(
                config,
                Arc::clone(&http_client_arc),
                Some(Arc::clone(&meta_cache)
                    as Arc<dyn pacquet_resolving_npm_resolver::PackageMetaCache>),
                auth_override.clone(),
                None,
            )
            .map_err(InstallError::BuildVerifiers)?
        };
        let derived_lockfile_path = lockfile.map(|_| {
            lockfile_path
                .map_or_else(|| workspace_root.join(Lockfile::FILE_NAME), Path::to_path_buf)
        });

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
            let normalized_root = pacquet_fs::lexical_normalize(&workspace_root);
            let root_defines_hook = project_manifests
                .iter()
                .find(|(project_dir, _)| {
                    pacquet_fs::lexical_normalize(project_dir) == normalized_root
                })
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
                ignore_manifest_check,
                false,
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
                    ignore_manifest_check,
                    true,
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
                            || !crate::check_custom_resolver_force_resolve::force_resolve_from_pnpmfile(
                                lockfile,
                                &workspace_root,
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
                    .save_to_path(&workspace_root.join(Lockfile::FILE_NAME))
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
            catalogs: &catalogs,
            project_manifests: &project_manifests,
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
            fresh_lockfile,
        } = materialize::<Reporter>(MaterializationInputs {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc,
            config,
            manifest,
            lockfile,
            take_frozen_path,
            lockfile_verification_override,
            resolution_verifiers,
            derived_lockfile_path,
            dependency_groups,
            project_manifests: &project_manifests,
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
            skip_runtimes,
            modules_manifest,
            prior_hoisted_dependencies,
            prune_orphans,
            logged_methods: &logged_methods,
            update_checksums,
            meta_cache,
            resolve_only,
            dry_run,
            can_prompt,
            update_seed_policy,
            auth_override,
            resolution_observer,
            peer_issues_sink,
            deps_requiring_build_sink,
            pnpmfile_hook,
            catalogs: &catalogs,
            prefix: &prefix,
        })
        .await?;

        finish_install::<Reporter>(FinishInstallInputs {
            resolve_only,
            dry_run,
            peer_issues_sink_is_none,
            existing_wanted_lockfile,
            fresh_lockfile,
            prefix,
            lockfile,
            requested_importer_ids,
            workspace_root,
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
            mutation,
            manifest_dir,
            selection,
            supported_architectures,
            catalogs,
        })
        .await
    }
}

struct PrepareModulesStateInputs<'a, 'install> {
    resolve_only: bool,
    take_frozen_path: bool,
    config: &'static Config,
    filtered_install: bool,
    installs_only: bool,
    workspace_root: &'a Path,
    included: IncludedDependencies,
    current_lockfile: Option<&'a Lockfile>,
    requested_importer_ids: Option<&'a HashSet<String>>,
    node_linker: NodeLinker,
    disable_optimistic_repeat_install: bool,
    lockfile: Option<&'a Lockfile>,
    supported_architectures: Option<&'a pacquet_package_is_installable::SupportedArchitectures>,
    rebuild: Option<&'a RebuildOptions>,
    resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    derived_lockfile_path: Option<&'a Path>,
    lockfile_verification_override: Option<super::LockfileVerificationOverride<'install>>,
    lockfile_synthesized_from_current: bool,
    lockfile_was_fast_updated: bool,
    catalogs: &'a Catalogs,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    prefix: &'a String,
}

struct PreparedModulesState<'install> {
    old_modules: Option<pacquet_modules_yaml::ModulesLayout>,
    previous_modules_metadata: Option<Modules>,
    is_inconsistent: bool,
    lockfile_verification_override: Option<super::LockfileVerificationOverride<'install>>,
}

async fn prepare_modules_state<'install, Reporter: self::Reporter + 'static>(
    inputs: PrepareModulesStateInputs<'_, 'install>,
) -> Result<Option<PreparedModulesState<'install>>, InstallError> {
    let PrepareModulesStateInputs {
        resolve_only,
        take_frozen_path,
        config,
        filtered_install,
        installs_only,
        workspace_root,
        included,
        current_lockfile,
        requested_importer_ids,
        node_linker,
        disable_optimistic_repeat_install,
        lockfile,
        supported_architectures,
        rebuild,
        resolution_verifiers,
        derived_lockfile_path,
        lockfile_verification_override,
        lockfile_synthesized_from_current,
        lockfile_was_fast_updated,
        catalogs,
        project_manifests,
        prefix,
    } = inputs;
    // A no-op still refreshes workspace state so `verifyDepsBeforeRun`
    // does not treat the materialized tree as stale.
    let modules_manifest_res = if !resolve_only || take_frozen_path {
        pacquet_modules_yaml::read_modules_layout::<Host>(&config.modules_dir)
    } else {
        Ok(None)
    };
    let read_failed = modules_manifest_res.is_err();
    if let Err(err) = &modules_manifest_res {
        tracing::warn!(
            target: "pacquet::install",
            ?err,
            "failed to read .modules.yaml; treating as an inconsistent node_modules directory",
        );
    }
    let old_modules = modules_manifest_res.ok().flatten();
    let modules_manifest = old_modules.as_ref();
    // A filtered install rewrites `.modules.yaml` from the selected
    // projects' state merged over the previous file's, so losing the
    // previous contents would drop every unselected project's entries.
    // An unreadable *layout* is already handled as an inconsistent
    // `node_modules` — the purge rebuilds everything and the merge is
    // skipped — but a file whose layout parses while some later field
    // does not would otherwise merge against `None` and silently prune
    // those entries, so that case fails instead.
    let previous_modules_metadata = if !resolve_only && !read_failed {
        match pacquet_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir) {
            Ok(modules) => modules,
            // A filtered install merges the unselected importers'
            // entries out of this file, so it cannot proceed
            // without it; an unfiltered install only loses the
            // orphan hoist-link cleanup.
            Err(error) if filtered_install => return Err(InstallError::ReadModules(error)),
            Err(error) => {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "failed to fully parse .modules.yaml; skipping orphan hoist-link cleanup",
                );
                None
            }
        }
    } else {
        None
    };
    // The purge keys off *layout* drift only, not `included`: an
    // included (`--prod`<->full) change is handled by relinking, so it
    // must not wipe the user's `node_modules` contents. See
    // [`modules_layout_consistent_with`].
    let is_inconsistent = read_failed
        || match &modules_manifest {
            Some(modules) => !modules_layout_consistent_with(modules, config, node_linker),
            // Treat existence-check errors conservatively as inconsistent.
            None => config
                .modules_dir
                .join(pacquet_modules_yaml::MODULES_FILENAME)
                .try_exists()
                .unwrap_or(true),
        };

    if !resolve_only && is_inconsistent {
        // A plain install may recreate the drifted modules dir;
        // `add` / `remove` must surface the drift instead
        // (upstream `validateModules` with `forceNewModules =
        // installsOnly`).
        if !installs_only && let Some(modules) = modules_manifest {
            check_modules_settings_diff(modules, config)?;
        }
        let (is_safe, target_dir) = if config.modules_dir.exists() {
            match (
                std::fs::canonicalize(&config.modules_dir),
                std::fs::canonicalize(workspace_root),
            ) {
                (Ok(modules_canon), Ok(root_canon)) => {
                    (modules_canon.starts_with(&root_canon), Some(modules_canon))
                }
                _ => (false, None),
            }
        } else {
            (true, None)
        };
        if is_safe {
            if let Some(target) = target_dir {
                match std::fs::read_dir(&target) {
                    Ok(entries) => {
                        for entry_res in entries {
                            let entry = match entry_res {
                                Ok(e) => e,
                                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                                    continue;
                                }
                                Err(err) => return Err(InstallError::RemoveModulesDir(err)),
                            };
                            let file_name = entry.file_name();
                            let file_name_str = file_name.to_string_lossy();

                            let is_hidden = file_name_str.starts_with('.');
                            let is_pnpm_hidden = file_name_str == ".bin"
                                || file_name_str == ".modules.yaml"
                                || config
                                    .virtual_store_dir
                                    .file_name()
                                    .is_some_and(|n| n == file_name_str.as_ref())
                                || modules_manifest.as_ref().is_some_and(|manifest| {
                                    let mut old_vs =
                                        std::path::PathBuf::from(&manifest.virtual_store_dir);
                                    if old_vs.is_relative() {
                                        old_vs = config.modules_dir.join(old_vs);
                                    }
                                    old_vs.starts_with(&config.modules_dir)
                                        && old_vs
                                            .file_name()
                                            .is_some_and(|n| n == file_name_str.as_ref())
                                });

                            if is_hidden && !is_pnpm_hidden {
                                continue;
                            }

                            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                                #[cfg(windows)]
                                let is_removed =
                                    pacquet_fs::remove_symlink_dir(&entry.path()).is_ok();
                                #[cfg(not(windows))]
                                let is_removed = false;

                                if !is_removed
                                    && let Err(err) = std::fs::remove_dir_all(entry.path())
                                    && err.kind() != std::io::ErrorKind::NotFound
                                {
                                    return Err(InstallError::RemoveModulesDir(err));
                                }
                            } else if let Err(err) = std::fs::remove_file(entry.path())
                                && err.kind() != std::io::ErrorKind::NotFound
                            {
                                return Err(InstallError::RemoveModulesDir(err));
                            }
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(InstallError::RemoveModulesDir(err)),
                }
            }
        } else {
            if filtered_install {
                return Err(InstallError::UnsafeFilteredModulesDir {
                    modules_dir: config.modules_dir.clone(),
                    workspace_root: workspace_root.to_path_buf(),
                });
            }
            tracing::warn!(
                ?config.modules_dir,
                "refusing to remove inconsistent modules directory outside the project root",
            );
        }
    }

    // Remove direct links from dependency groups excluded by this
    // run. Unfiltered installs can use the global `included` value
    // recorded in `.modules.yaml`; filtered installs may retain
    // importers materialized with different group sets, so they
    // conservatively prune every excluded group from only the
    // selected workspace-link closure.
    if !resolve_only
        && !is_inconsistent
        && let Some(modules) = modules_manifest
        && let Some(current) = current_lockfile.as_ref()
        && (filtered_install || modules.included != included)
    {
        let selected_prune_importer_ids = requested_importer_ids.as_ref().map(|requested| {
            crate::materialization_closure(
                current,
                workspace_root,
                requested,
                included,
                &crate::SkippedSnapshots::new(),
            )
            .importer_ids
        });
        let previously_included = if filtered_install {
            IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            }
        } else {
            modules.included
        };
        crate::prune_direct_deps_excluded_by_groups(
            current,
            previously_included,
            included,
            workspace_root,
            config,
            selected_prune_importer_ids.as_ref(),
        )
        .map_err(InstallError::PruneDirectDeps)?;
    }

    let modules_cache_prune_due = modules_manifest.as_ref().is_some_and(|modules| {
        crate::prune_virtual_store::should_prune_virtual_store(
            crate::prune_virtual_store::same_dir(
                config.effective_virtual_store_dir(),
                &config.global_virtual_store_dir,
            ),
            Some(modules.pruned_at.as_str()),
            config.modules_cache_max_age,
            SystemTime::now(),
        )
    });

    if take_frozen_path
            && !filtered_install
            && !disable_optimistic_repeat_install
            // `--force` reinstalls everything, so an up-to-date tree
            // must not short-circuit the materialization.
            && !config.force
            && let Some(wanted_lockfile) = lockfile
            && let Some(current) = current_lockfile
            && wanted_lockfile == current
            && let Some(modules) = modules_manifest.as_ref()
            && modules_consistent_with(modules, config, node_linker, included)
            // A `supportedArchitectures` change alters the skip set
            // without touching the lockfile or `.modules.yaml`, so the
            // unchanged-layout premise doesn't hold and the platform
            // packages must be re-evaluated.
            && crate::optimistic_repeat_install::recorded_supported_architectures_match(
                workspace_root,
                supported_architectures,
            )
            // An `allowBuilds` change that now permits a previously-ignored
            // build must rebuild it, even though the lockfile and layout are
            // unchanged.
            && !has_newly_allowed_ignored_builds(modules, config)
            // The mirror image: an approval the user has since withdrawn
            // must be re-evaluated, or a strict install would exit 0 on a
            // package it is no longer allowed to build.
            && !has_revoked_allowed_builds(modules, config)
            // A build marker lives in the shared slot, outside every
            // project-state input checked above. Let materialization inspect
            // buildable and patched GVS slots instead of declaring the local
            // tree complete from importer links alone.
            && !gvs_build_marker_present(wanted_lockfile, config, workspace_root)
            // An explicit `pacquet rebuild` always re-runs the build phase,
            // so it never short-circuits here.
            && rebuild.is_none()
            && !modules_cache_prune_due
            && frozen_tree_intact(wanted_lockfile, modules, config, workspace_root, node_linker)
    {
        // The full frozen path runs the offline structural
        // name gate before any materialization; the up-to-date
        // early return must not skip it (the resolution-verifier
        // fan-out below is policy-gated and can be empty).
        pacquet_lockfile_verification::verify_lockfile_dependency_names(wanted_lockfile)
            .map_err(InstallError::LockfileVerification)?;
        // Nothing to materialize means no fetch to overlap; verify
        // eagerly before the up-to-date early return.
        if let Some(lockfile_verification_override) = lockfile_verification_override {
            lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
        } else {
            verify_lockfile_eagerly::<Reporter>(
                wanted_lockfile,
                resolution_verifiers,
                derived_lockfile_path,
                &config.cache_dir,
            )
            .await?;
        }
        // Keep `strictDepBuilds` enforced on the up-to-date path: a
        // rerun after an `ERR_PNPM_IGNORED_BUILDS` failure must not
        // exit 0 just because the lockfile and layout are unchanged.
        // Checked after verification (a tampered lockfile fails first)
        // and before the "up to date" log so the command doesn't
        // claim success.
        // `Err` (malformed `allowBuilds`) is unreachable here — the
        // `has_newly_allowed_ignored_builds` guard above returns `true`
        // on the same `from_config` error and skips this block — so a
        // bad policy is surfaced by the full install instead.
        if config.strict_dep_builds
            && let Ok(Some(package_names)) = unapproved_recorded_ignored_builds(modules, config)
        {
            return Err(InstallError::IgnoredBuilds { package_names });
        }
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Info,
            message: "Lockfile is up to date, resolution step is skipped".to_string(),
            prefix: prefix.clone(),
        }));
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: prefix.clone(),
            stage: Stage::ImportingDone,
        }));
        if (lockfile_synthesized_from_current || lockfile_was_fast_updated) && config.lockfile {
            wanted_lockfile
                .save_to_path(&workspace_root.join(Lockfile::FILE_NAME))
                .map_err(InstallError::SaveWantedLockfile)?;
        }
        update_workspace_state(
            workspace_root,
            &build_workspace_state(
                workspace_root,
                config,
                node_linker,
                included,
                supported_architectures,
                catalogs,
                project_manifests,
                filtered_install,
            ),
        )
        .map_err(InstallError::WriteWorkspaceState)?;
        Reporter::emit(&LogEvent::Summary(SummaryLog {
            level: LogLevel::Debug,
            prefix: prefix.to_owned(),
        }));
        return Ok(None);
    }

    Ok(Some(PreparedModulesState {
        old_modules,
        previous_modules_metadata,
        is_inconsistent,
        lockfile_verification_override,
    }))
}

struct MaterializationInputs<'a, 'install> {
    tarball_mem_cache: Arc<MemCache>,
    resolved_packages: &'a ResolvedPackages,
    http_client: &'a ThrottledClient,
    http_client_arc: Arc<ThrottledClient>,
    config: &'static Config,
    manifest: &'a PackageManifest,
    lockfile: Option<&'a Lockfile>,
    take_frozen_path: bool,
    lockfile_verification_override: Option<super::LockfileVerificationOverride<'install>>,
    resolution_verifiers: Vec<Arc<dyn ResolutionVerifier>>,
    derived_lockfile_path: Option<PathBuf>,
    dependency_groups: Vec<DependencyGroup>,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    workspace_projects: Option<&'a [pacquet_workspace::Project]>,
    requested_importer_ids: Option<&'a HashSet<String>>,
    real_importer_ids: &'a HashSet<String>,
    workspace_root: &'a Path,
    included: IncludedDependencies,
    node_linker: NodeLinker,
    rebuild: Option<&'a RebuildOptions>,
    ignore_manifest_check: bool,
    mutation: ProjectMutation,
    current_lockfile: Option<&'a Lockfile>,
    supported_architectures: Option<&'a pacquet_package_is_installable::SupportedArchitectures>,
    skip_runtimes: bool,
    modules_manifest: Option<&'a pacquet_modules_yaml::ModulesLayout>,
    prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    prune_orphans: bool,
    logged_methods: &'a AtomicU8,
    update_checksums: bool,
    meta_cache: Arc<InMemoryPackageMetaCache>,
    resolve_only: bool,
    dry_run: bool,
    can_prompt: bool,
    update_seed_policy: UpdateSeedPolicy,
    auth_override: Option<Arc<AuthHeaders>>,
    resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    peer_issues_sink: Option<PeerIssuesSink>,
    deps_requiring_build_sink: Option<DepsRequiringBuildSink>,
    pnpmfile_hook: Option<Arc<dyn pacquet_hooks::PnpmfileHooks>>,
    catalogs: &'a Catalogs,
    prefix: &'a String,
}

struct MaterializationOutput {
    ignored_builds: Vec<String>,
    deferred_builds: Vec<String>,
    injected_deps: BTreeMap<String, Vec<String>>,
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    install_skipped: crate::SkippedSnapshots,
    fresh_lockfile: Option<Lockfile>,
}

async fn materialize<Reporter: self::Reporter + 'static>(
    inputs: MaterializationInputs<'_, '_>,
) -> Result<MaterializationOutput, InstallError> {
    let MaterializationInputs {
        tarball_mem_cache,
        resolved_packages,
        http_client,
        http_client_arc,
        config,
        manifest,
        lockfile,
        take_frozen_path,
        lockfile_verification_override,
        resolution_verifiers,
        derived_lockfile_path,
        dependency_groups,
        project_manifests,
        workspace_projects,
        requested_importer_ids,
        real_importer_ids,
        workspace_root,
        included,
        node_linker,
        rebuild,
        ignore_manifest_check,
        mutation,
        current_lockfile,
        supported_architectures,
        skip_runtimes,
        modules_manifest,
        prior_hoisted_dependencies,
        prune_orphans,
        logged_methods,
        update_checksums,
        meta_cache,
        resolve_only,
        dry_run,
        can_prompt,
        update_seed_policy,
        auth_override,
        resolution_observer,
        peer_issues_sink,
        deps_requiring_build_sink,
        pnpmfile_hook,
        catalogs,
        prefix,
    } = inputs;
    let ignored_builds: Vec<String>;
    let deferred_builds: Vec<String>;
    let injected_deps: BTreeMap<String, Vec<String>>;
    let effective_node_version =
        config.node_version.clone().or_else(|| node_version_from_engines_runtime(manifest.value()));
    let (hoisted_dependencies, hoisted_locations, install_skipped, fresh_lockfile): (
        HoistedDependencies,
        BTreeMap<String, Vec<String>>,
        crate::SkippedSnapshots,
        Option<Lockfile>,
    ) = if take_frozen_path {
        let lockfile = lockfile.expect("dispatch verified lockfile is present");
        // pnpm's headless installer announces itself whenever it is
        // entered — also on a cold `node_modules` and on subset
        // (`--filter`) installs — not only when nothing needs to be
        // materialized. `pnpm fetch` gets upstream's
        // ignorePackageManifest wording instead; it is the one
        // caller combining `ignore_manifest_check` with a non-full
        // install, and the flag alone can't identify it because
        // `install --ignore-manifest-check` is a user-facing way to
        // skip the frozen freshness gate on a full install.
        // Upstream's headless entry returns before the announcement
        // for an empty lockfile (`isEmptyLockfile`), and an explicit
        // `pnpm rebuild` is not an install, so both stay silent.
        if rebuild.is_none() && !lockfile.is_empty() {
            let message = if ignore_manifest_check && !mutation.is_full_install() {
                "Importing packages to virtual store"
            } else {
                "Lockfile is up to date, resolution step is skipped"
            };
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Info,
                message: message.to_string(),
                prefix: prefix.clone(),
            }));
        }
        let initial_materialization_ids = requested_importer_ids.map(|selected| {
            if matches!(node_linker, NodeLinker::Hoisted) {
                lockfile.importers.keys().cloned().collect()
            } else {
                selected.clone()
            }
        });
        let empty_skipped = crate::SkippedSnapshots::new();
        let materialization = initial_materialization_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                lockfile,
                workspace_root,
                importer_ids,
                included,
                &empty_skipped,
            )
        });
        let materialization_lockfile =
            materialization.as_ref().map_or(lockfile, |closure| &closure.lockfile);
        let project_anchor_ids = match requested_importer_ids {
            Some(selected) if matches!(node_linker, NodeLinker::Hoisted) => selected.clone(),
            Some(_) => materialization
                .as_ref()
                .expect("selected install has a materialization closure")
                .importer_ids
                .clone(),
            None => real_importer_ids.clone(),
        };
        let frozen_project_manifests = project_manifests
            .iter()
            .filter(|(project_dir, _)| {
                let importer_id =
                    pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir);
                project_anchor_ids.contains(&importer_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let Lockfile { lockfile_version, importers, packages, snapshots, .. } =
            materialization_lockfile;
        let lockfile_major = lockfile_version.major;
        let supported_lockfile_major = matches!(lockfile_major, 9 | 12);
        debug_assert!(supported_lockfile_major);

        let mut frozen_verification_override = lockfile_verification_override;
        if requested_importer_ids.is_some() {
            if let Some(verification_override) = frozen_verification_override.take() {
                verification_override.await.map_err(map_frozen_lockfile_error)?;
            } else {
                verify_lockfile_eagerly::<Reporter>(
                    lockfile,
                    &resolution_verifiers,
                    derived_lockfile_path.as_deref(),
                    &config.cache_dir,
                )
                .await?;
            }
        }
        let frozen_resolution_verifiers = if requested_importer_ids.is_some() {
            &[][..]
        } else {
            resolution_verifiers.as_slice()
        };

        let frozen_result = InstallFrozenLockfile {
            http_client,
            config,
            importers,
            packages: packages.as_ref(),
            snapshots: snapshots.as_ref(),
            lockfile: materialization_lockfile,
            resolution_verifiers: frozen_resolution_verifiers,
            lockfile_verification_override: frozen_verification_override,
            lockfile_path: derived_lockfile_path.as_deref(),
            current_lockfile,
            // `--force` relinks every package, so the per-snapshot
            // "unchanged since the previous install" skip must not
            // see the current lockfile — pnpm's
            // `lockfileToDepGraph(..., opts.force ? null :
            // currentLockfile)`. `current_lockfile` itself stays:
            // pnpm's prune runs on the real current lockfile even
            // under force.
            current_snapshots: (!config.force)
                .then_some(current_lockfile)
                .flatten()
                .and_then(|lockfile| lockfile.snapshots.as_ref()),
            current_packages: (!config.force)
                .then_some(current_lockfile)
                .flatten()
                .and_then(|lockfile| lockfile.packages.as_ref()),
            dependency_groups,
            project_manifests: &frozen_project_manifests,
            package_map_project_manifests: project_manifests,
            logged_methods,
            workspace_root,
            requester: prefix,
            supported_architectures,
            skip_runtimes,
            node_version: effective_node_version.clone(),
            node_linker,
            tarball_mem_cache: Some(&tarball_mem_cache),
            seed_skipped: modules_manifest.map(|manifest| manifest.skipped.clone()),
            rebuild,
            prior_hoisted_dependencies,
            prune_orphans,
        }
        .run::<Reporter>()
        .await
        // Surface a verification failure as the same top-level
        // `LockfileVerification` variant the eager paths use, rather
        // than nesting it under `FrozenLockfile` — the concurrent gate
        // is the same gate, just run alongside the fetch.
        .map_err(map_frozen_lockfile_error)?;

        ignored_builds = frozen_result.ignored_builds;
        deferred_builds = frozen_result.deferred_builds;
        injected_deps = frozen_result.injected_deps;
        (
            frozen_result.hoisted_dependencies,
            frozen_result.hoisted_locations,
            frozen_result.skipped,
            None,
        )
    } else {
        // Re-verify the existing lockfile before the fresh resolve,
        // matching the pre-resolution gate: a committed lockfile that
        // bypassed the policy locally is caught here even though the
        // resolver re-resolves from it. No-op when there's no lockfile
        // (state 4) or verification is disabled. The fresh path's own
        // resolution is the slow part, so this stays a blocking gate.
        if let Some(lockfile_verification_override) = lockfile_verification_override {
            lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
        } else if let Some(loaded_lockfile) = lockfile {
            verify_lockfile_eagerly::<Reporter>(
                loaded_lockfile,
                &resolution_verifiers,
                derived_lockfile_path.as_deref(),
                &config.cache_dir,
            )
            .await?;
        }

        let workspace_packages = build_workspace_packages_map(workspace_projects);
        // Build the per-importer manifest list. The root importer
        // (`"."`) always reuses the in-memory `Install.manifest`
        // — `pacquet add` mutates that value before calling install,
        // so re-reading from disk would walk the pre-add shape and
        // miss the freshly-added dep. Sibling importers come from
        // the `find_workspace_projects` walk, which read them off
        // disk for `workspace_packages` already.
        let importer_manifests: BTreeMap<String, &PackageManifest> = project_manifests
            .iter()
            .map(|(project_dir, manifest)| {
                (
                    pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir),
                    *manifest,
                )
            })
            .collect();
        let fresh_result = InstallWithFreshLockfile {
            tarball_mem_cache,
            resolved_packages,
            http_client,
            http_client_arc: Arc::clone(&http_client_arc),
            config,
            importer_manifests,
            dependency_groups,
            logged_methods,
            requester: prefix,
            catalogs: catalogs.clone(),
            lockfile_dir: workspace_root,
            workspace_packages,
            update_checksums,
            meta_cache: Arc::clone(&meta_cache),
            // States 3 and 4 of the dispatch share this branch.
            // State 3 (lockfile present but stale or
            // `preferFrozenLockfile: false`) passes the existing
            // lockfile so the resolver seeds
            // `getPreferredVersionsFromLockfileAndManifests` with
            // already-pinned `(name, version)` pairs — unrelated
            // entries keep their pins on rewrite (the `update: false`
            // mode). State 4 (no lockfile) passes `None`.
            wanted_lockfile: lockfile,
            node_version: effective_node_version,
            node_linker,
            supported_architectures,
            lockfile_only: resolve_only,
            skip_runtimes,
            dry_run,
            can_prompt,
            is_full_install: mutation.is_full_install(),
            update_seed_policy,
            auth_override,
            resolution_observer,
            peer_issues_sink: peer_issues_sink.clone(),
            deps_requiring_build_sink: deps_requiring_build_sink.as_ref().map(Arc::clone),
            pnpmfile_hook_override: pnpmfile_hook,
            real_importer_ids: requested_importer_ids.map(|_| real_importer_ids),
            selected_importer_ids: requested_importer_ids,
            current_lockfile,
            prior_hoisted_dependencies,
            prune_orphans,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallError::WithFreshLockfile)?;

        if fresh_result.can_record_lockfile_verification
            && let Some(lockfile) = fresh_result.wanted_lockfile.as_ref()
        {
            // Record under the same path the verification gates key
            // their cache on, so the next install's stat shortcut hits.
            let lockfile_path = derived_lockfile_path
                .clone()
                .unwrap_or_else(|| workspace_root.join(Lockfile::FILE_NAME));
            record_lockfile_verified(
                Some(&config.cache_dir),
                &lockfile_path,
                lockfile,
                &resolution_verifiers,
            );
        }

        ignored_builds = fresh_result.ignored_builds;
        deferred_builds = fresh_result.deferred_builds;
        injected_deps = fresh_result.injected_deps;
        (
            fresh_result.hoisted_dependencies,
            fresh_result.hoisted_locations,
            fresh_result.skipped,
            fresh_result.wanted_lockfile,
        )
    };

    Ok(MaterializationOutput {
        ignored_builds,
        deferred_builds,
        injected_deps,
        hoisted_dependencies,
        hoisted_locations,
        install_skipped,
        fresh_lockfile,
    })
}

struct FinishInstallInputs<'a, 'selection> {
    resolve_only: bool,
    dry_run: bool,
    peer_issues_sink_is_none: bool,
    existing_wanted_lockfile: Option<&'a Lockfile>,
    fresh_lockfile: Option<Lockfile>,
    prefix: String,
    lockfile: Option<&'a Lockfile>,
    requested_importer_ids: Option<HashSet<String>>,
    workspace_root: PathBuf,
    included: IncludedDependencies,
    install_skipped: crate::SkippedSnapshots,
    node_linker: NodeLinker,
    current_lockfile: Option<Lockfile>,
    real_importer_ids: HashSet<String>,
    project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
    filtered_install: bool,
    is_inconsistent: bool,
    previous_modules_metadata: Option<Modules>,
    config: &'static Config,
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    injected_deps: BTreeMap<String, Vec<String>>,
    ignored_builds: Vec<String>,
    deferred_builds: Vec<String>,
    modules_manifest: Option<pacquet_modules_yaml::ModulesLayout>,
    rebuild: Option<RebuildOptions>,
    take_frozen_path: bool,
    lockfile_synthesized_from_current: bool,
    lockfile_was_fast_updated: bool,
    mutation: ProjectMutation,
    manifest_dir: &'a Path,
    selection: Option<WorkspaceInstallSelection<'selection>>,
    supported_architectures: Option<pacquet_package_is_installable::SupportedArchitectures>,
    catalogs: Catalogs,
}

async fn finish_install<Reporter: self::Reporter + 'static>(
    inputs: FinishInstallInputs<'_, '_>,
) -> Result<(), InstallError> {
    let FinishInstallInputs {
        resolve_only,
        dry_run,
        peer_issues_sink_is_none,
        existing_wanted_lockfile,
        fresh_lockfile,
        prefix,
        lockfile,
        requested_importer_ids,
        workspace_root,
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
        modules_manifest,
        rebuild,
        take_frozen_path,
        lockfile_synthesized_from_current,
        lockfile_was_fast_updated,
        mutation,
        manifest_dir,
        selection,
        supported_architectures,
        catalogs,
    } = inputs;
    let modules_manifest = modules_manifest.as_ref();
    tracing::info!(target: "pacquet::install", "Complete all");

    // Resolve-only runs must not persist state that claims materialization happened.
    if resolve_only {
        // `--dry-run` resolved a fresh lockfile but wrote nothing. Diff
        // it against the existing on-disk lockfile and print a report,
        // then exit 0 — npm-style preview semantics. A sink-driven dry
        // run (napi `getPeerDependencyIssues`) is a programmatic query,
        // not a preview — no report.
        if dry_run && peer_issues_sink_is_none {
            use std::io::Write as _;
            let report =
                crate::lockfile_diff::render_dry_run_report(&crate::lockfile_diff::diff_lockfiles(
                    existing_wanted_lockfile,
                    fresh_lockfile.as_ref(),
                    crate::lockfile_diff::ImporterDiffKey::Specifier,
                ));
            let mut stdout = std::io::stdout();
            let _ = writeln!(stdout, "{report}");
            let _ = stdout.flush();
        }
        Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));
        return Ok(());
    }

    let materialized_wanted_lockfile = fresh_lockfile.as_ref().or(lockfile);
    let selected_current_lockfile = materialized_wanted_lockfile.and_then(|wanted| {
        requested_importer_ids.as_ref().map(|requested| {
            crate::materialization_closure(
                wanted,
                &workspace_root,
                requested,
                included,
                &install_skipped,
            )
            .lockfile
        })
    });
    let materialized_current_lockfile = materialized_wanted_lockfile.map(|wanted| {
        if requested_importer_ids.is_some() && matches!(node_linker, NodeLinker::Hoisted) {
            crate::filter_lockfile_for_current(wanted, included, &install_skipped)
        } else if let Some(requested_importer_ids) = requested_importer_ids.as_ref() {
            crate::merge_filtered_current_lockfile(
                (!is_inconsistent).then_some(current_lockfile.as_ref()).flatten(),
                wanted,
                requested_importer_ids,
                included,
                &install_skipped,
                &workspace_root,
            )
        } else {
            crate::filter_lockfile_for_current(wanted, included, &install_skipped)
        }
    });
    let project_anchor_importer_ids = match requested_importer_ids.as_ref() {
        Some(requested) if matches!(node_linker, NodeLinker::Hoisted) => requested.clone(),
        Some(requested) => materialized_wanted_lockfile.map_or_else(
            || requested.clone(),
            |wanted| {
                crate::materialization_closure(
                    wanted,
                    &workspace_root,
                    requested,
                    included,
                    &install_skipped,
                )
                .importer_ids
            },
        ),
        None => real_importer_ids.clone(),
    };
    let materialized_project_manifests = project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            let importer_id =
                pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir);
            project_anchor_importer_ids.contains(&importer_id)
        })
        .cloned()
        .collect::<Vec<_>>();

    if filtered_install
        && !matches!(node_linker, NodeLinker::Hoisted)
        && crate::should_write_package_map(config, node_linker)
        && let Some(current) = materialized_current_lockfile.as_ref()
    {
        let runtime_major =
            crate::install_frozen_lockfile::find_runtime_node_major(current.snapshots.as_ref());
        let configured_major = config
            .node_version
            .as_deref()
            .and_then(crate::install_frozen_lockfile::parse_major_from_version);
        let engine_name = match runtime_major.or(configured_major) {
            Some(major) => Some(pacquet_graph_hasher::engine_name(major, None, None)),
            None if config.enable_global_virtual_store => tokio::task::spawn_blocking(|| {
                pacquet_graph_hasher::detect_node_major()
                    .map(|major| pacquet_graph_hasher::engine_name(major, None, None))
            })
            .await
            .ok()
            .flatten(),
            None => None,
        };
        let allow_build_policy = crate::AllowBuildPolicy::from_config(config)
            .expect("allow-build policy was validated by the install path");
        let layout = crate::VirtualStoreLayout::new(
            config,
            engine_name.as_deref(),
            current.snapshots.as_ref(),
            current.packages.as_ref(),
            Some(&allow_build_policy),
            Some(workspace_root.as_path()),
        );
        crate::package_map::write_package_map(
            current,
            &crate::package_map::PackageMapOptions {
                lockfile_dir: &workspace_root,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout: &layout,
                project_manifests: &project_manifests,
            },
        )
        .map_err(InstallError::WritePackageMap)?;
    }

    // Materialize `link:` direct deps straight from the in-memory
    // project manifests. `excludeLinksFromLockfile` keeps them out
    // of the lockfile importers, so the lockfile-driven symlink
    // passes inside the frozen/fresh paths never see them; pnpm
    // v11's `linkDirectDeps` linked them from the projects
    // regardless. Aliases the wanted lockfile *does* track are
    // skipped — those belong to the lockfile passes (and their
    // dedupe decisions). See [`crate::link_manifest_link_deps`].
    // These are importer symlinks like any other, so
    // `virtualStoreOnly` skips them too.
    if !config.virtual_store_only {
        crate::link_manifest_link_deps::<Reporter>(
            &workspace_root,
            &materialized_project_manifests,
            fresh_lockfile.as_ref().or(lockfile).and_then(|lockfile| {
                (!lockfile.importers.is_empty()).then_some(&lockfile.importers)
            }),
            // Honor a `modulesDir` override the same way the
            // lockfile-driven symlink pass does.
            config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules")),
            &crate::shim_extra_node_paths(config, node_linker),
        )
        .map_err(InstallError::LinkManifestLinkDeps)?;
    }

    let prior_modules = modules_manifest;
    let now = SystemTime::now();
    let effective_virtual_store_dir = config.effective_virtual_store_dir();
    // Decide "this is the global store" from the resolved paths, not
    // the `enableGlobalVirtualStore` flag alone: the global store is
    // shared across projects, so a config that points `virtualStoreDir`
    // at it must not be pruned even when the flag is off.
    let is_global_virtual_store = crate::prune_virtual_store::same_dir(
        effective_virtual_store_dir,
        &config.global_virtual_store_dir,
    );
    // `did_prune` tracks whether the sweep actually ran (enumerated the
    // store), not just whether the throttle allowed it. It stays false
    // when there is no wanted lockfile to derive the needed set from
    // (e.g. `config.lockfile == false` leaves both `fresh_lockfile` and
    // a loaded `lockfile` absent), when the target is refused as unsafe,
    // or when enumeration failed. `prunedAt` must not advance on a run
    // where nothing was swept, or the next real sweep is throttled off
    // for `modulesCacheMaxAge`.
    let did_prune = if crate::prune_virtual_store::should_prune_virtual_store(
        is_global_virtual_store,
        prior_modules.as_ref().map(|modules| modules.pruned_at.as_str()),
        config.modules_cache_max_age,
        now,
    ) {
        match materialized_current_lockfile.as_ref() {
            // Sweep the canonicalized prune target returned by the
            // containment check, never the raw configured path: deleting
            // from the validated path closes the time-of-check/time-of-use
            // gap a symlink swap would otherwise open.
            Some(wanted) => {
                if let Some(prune_dir) = crate::prune_virtual_store::prune_target_within_modules(
                    effective_virtual_store_dir,
                    &config.modules_dir,
                ) {
                    crate::prune_virtual_store::prune_virtual_store(
                        &prune_dir,
                        wanted.snapshots.iter().flat_map(|snapshots| snapshots.keys()),
                        &install_skipped,
                        config.virtual_store_dir_max_length as usize,
                    )
                    .is_some()
                } else {
                    // A wanted lockfile exists but the store path is unsafe
                    // (escapes node_modules); refuse the destructive sweep.
                    tracing::warn!(
                        virtual_store_dir = %effective_virtual_store_dir.display(),
                        modules_dir = %config.modules_dir.display(),
                        "skipping virtual-store prune: the virtual store is not inside node_modules",
                    );
                    false
                }
            }
            None => false,
        }
    } else {
        false
    };

    // Stamp `prunedAt` only when the sweep ran (or there was no prior
    // `.modules.yaml`); otherwise preserve the recorded timestamp so
    // the throttle keeps counting from the last real prune.
    let pruned_at = match (&prior_modules, did_prune) {
        (Some(prior), false) => prior.pruned_at.clone(),
        _ => httpdate::fmt_http_date(now),
    };

    // The projects whose own install scripts `--ignore-scripts`
    // skipped are owed a build just like the dependencies the build
    // phase deferred, and are recorded by importer id.
    let deferred_projects = config.ignore_scripts.then(|| {
        materialized_project_manifests
            .iter()
            .filter(|(project_dir, manifest)| {
                project_requires_lifecycle_scripts(project_dir, manifest)
            })
            .map(|(project_dir, _)| {
                pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir)
            })
            .collect::<Vec<_>>()
    });
    let previous_pending_builds =
        prior_modules.map_or(&[][..], |modules| modules.pending_builds.as_slice());
    // The build phase settles a dependency only when it actually
    // rebuilt it, so a `pnpm rebuild --pending` that the policy still
    // blocks (`allowBuilds: None`/`false`) leaves the debt in place.
    // Reuse the same policy `BuildModules` ran under; on a rebuild a
    // selected, approved dependency always runs (force-rebuild
    // bypasses the side-effects cache gate), so policy approval is a
    // faithful stand-in for "was rebuilt".
    let rebuild_build_policy =
        rebuild.as_ref().and_then(|_| crate::AllowBuildPolicy::from_config(config).ok());
    let pending_builds = merge_pending_builds(
        previous_pending_builds,
        deferred_projects.into_iter().flatten().chain(deferred_builds),
        materialized_current_lockfile.as_ref(),
        rebuild.as_ref(),
        rebuild_build_policy.as_ref(),
    );

    let mut next_modules = build_modules_manifest(
        config,
        node_linker,
        included,
        hoisted_dependencies,
        hoisted_locations,
        injected_deps,
        &install_skipped,
        &ignored_builds,
        pending_builds,
        pruned_at,
    );
    if filtered_install
        && !matches!(node_linker, NodeLinker::Hoisted)
        && !is_inconsistent
        && let (Some(previous), Some(current), Some(selected)) = (
            previous_modules_metadata.as_ref(),
            materialized_current_lockfile.as_ref(),
            selected_current_lockfile.as_ref(),
        )
    {
        merge_filtered_modules_metadata(&mut next_modules, previous, current, selected);
    }
    write_modules_manifest::<Host>(&config.modules_dir, next_modules)
        .map_err(InstallError::WriteModules)?;

    // Write `<virtual_store_dir>/lock.yaml`. Captures what was
    // actually materialized so the next install can diff each
    // snapshot against it and skip the unchanged
    // slots. Persist *after* `write_modules_manifest` succeeds so
    // a manifest failure can't leave a fresh current-lockfile
    // pointing at incomplete install state — the next frozen
    // reinstall would otherwise diff against a graph that never
    // finished committing (review on <https://github.com/pnpm/pacquet/pull/442>).
    //
    // A filtered isolated/PnP install merges its newly materialized
    // closure into compatible prior current state, while a hoisted
    // install records the full shared graph it materialized. This
    // keeps the file aligned with physical state without discarding
    // unselected slots that remain on disk.
    if let Some(lockfile) = materialized_current_lockfile.as_ref() {
        // Filter the wanted lockfile down to the snapshots that
        // were actually materialized: dep maps the user excluded
        // (`--no-optional`, `--no-dev`) plus snapshots the
        // install-time skip set transiently dropped (a fetch
        // failure, `--no-optional`-only entries). The next install
        // diffs against this filtered shape so dropped snapshots
        // aren't mistaken for already-done work.
        lockfile
            .save_current_to_virtual_store_dir(&config.virtual_store_dir)
            .map_err(InstallError::SaveCurrentLockfile)?;
    }

    // Regenerate `pnpm-lock.yaml` from the synthesized snapshot when
    // the wanted lockfile was reconstructed from
    // `<virtual_store_dir>/lock.yaml`. The no-op short-circuit above
    // handles the common case; this branch covers the rare path where
    // `.modules.yaml` was wiped or inconsistent and the frozen install
    // had to relink.
    if take_frozen_path
        && (lockfile_synthesized_from_current || lockfile_was_fast_updated)
        && config.lockfile
        && let Some(updated) = lockfile
    {
        updated
            .save_to_path(&workspace_root.join(Lockfile::FILE_NAME))
            .map_err(InstallError::SaveWantedLockfile)?;
    }

    // A `pnpm rebuild --pending` is the exception to the mutation
    // gate: it installs no manifest, but the projects it names are
    // exactly the ones whose scripts an earlier `--ignore-scripts`
    // install deferred, and running them is what lets the install
    // drop those entries from `pendingBuilds` (see
    // [`merge_pending_builds`]).
    let projects_to_run: Vec<(std::path::PathBuf, &PackageManifest)> =
        if config.ignore_scripts || config.virtual_store_only {
            Vec::new()
        } else if let Some(rebuild) = rebuild.as_ref() {
            materialized_project_manifests
                .iter()
                .filter(|(project_dir, _)| {
                    let importer_id =
                        pacquet_workspace::importer_id_from_root_dir(&workspace_root, project_dir);
                    rebuild.pending_projects.contains(&importer_id)
                })
                .cloned()
                .collect()
        } else {
            projects_running_own_scripts(&ProjectScriptsInputs {
                mutation,
                workspace_root: &workspace_root,
                active_project_dir: manifest_dir,
                selected_dirs: selection.as_ref().map(|selection| selection.selected_dirs),
                project_manifests: &project_manifests,
                materialized_project_manifests: &materialized_project_manifests,
            })
        };
    if !projects_to_run.is_empty() {
        let project_groups = order_project_lifecycle_groups(
            &projects_to_run,
            selection.as_ref().map(|selection| selection.ordered_groups),
            &workspace_root,
            materialized_current_lockfile.as_ref(),
        )?;
        if !project_groups.is_empty() {
            run_projects_lifecycle_scripts::<Reporter>(
                &project_groups,
                config,
                node_linker,
                &workspace_root,
            )?;
        }
        if let Some(rebuild) = rebuild.as_ref() {
            drain_settled_projects::<Host>(&config.modules_dir, &rebuild.pending_projects)?;
        }
    }

    // Write `node_modules/.pnpm-workspace-state-v1.json`.
    // pnpm's `verifyDepsBeforeRun` gate bails to "outdated" the
    // moment this file is missing, forcing `pnpm install` to rerun.
    // Writing it after both the `.modules.yaml` and the current
    // lockfile succeed keeps the file pointing at a fully committed
    // install.
    update_workspace_state(
        &workspace_root,
        &build_workspace_state(
            &workspace_root,
            config,
            node_linker,
            included,
            supported_architectures.as_ref(),
            &catalogs,
            &project_manifests,
            filtered_install,
        ),
    )
    .map_err(InstallError::WriteWorkspaceState)?;

    // `pnpm:summary` closes the install and lets the reporter render
    // the accumulated `pnpm:root` events as a "+N -M" block. Must
    // come after `importing_done`.
    Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));

    // A global install is exempt from the scaffold below: its root is a
    // throwaway per-group directory, and the approval prompt that
    // follows it records the ignored builds against the stable global
    // packages dir instead.
    let is_global_install = config
        .global_pkg_dir
        .as_deref()
        .is_some_and(|global_pkg_dir| workspace_root.starts_with(global_pkg_dir));
    // Leave the user a line to edit in `pnpm-workspace.yaml` for every
    // build this install blocked, so approving one is an edit rather
    // than recalling the `allowBuilds` shape. Written before the strict
    // failure below, which is the very run whose message it answers.
    if !ignored_builds.is_empty() && !is_global_install {
        let allow_build_keys: BTreeSet<String> = ignored_builds
            .iter()
            .map(|dep_path| crate::allow_build_key_from_ignored_build(dep_path))
            .collect();
        pacquet_workspace_manifest_writer::scaffold_allow_builds(
            config.workspace_dir.as_deref().unwrap_or(&workspace_root),
            allow_build_keys.iter().map(String::as_str),
        )
        .map_err(InstallError::ScaffoldAllowBuilds)?;
    }

    // When `strictDepBuilds` is on (the default), an install that
    // blocked any dependency build script fails with
    // `ERR_PNPM_IGNORED_BUILDS` *after* the artifacts are written, so
    // the package is still added/installed and the user approves the
    // builds and reinstalls.
    if config.strict_dep_builds && !ignored_builds.is_empty() {
        return Err(InstallError::IgnoredBuilds { package_names: ignored_builds });
    }

    Ok(())
}
