use super::{
    BTreeMap, BTreeSet, Catalogs, Config, GlobalLog, HashSet, HoistedDependencies, Host,
    IncludedDependencies, InstallError, InstallWithFreshLockfileError, Lockfile, LogEvent,
    LogLevel, Modules, NodeLinker, PackageManifest, Path, PathBuf, ProjectMutation,
    ProjectScriptsInputs, RebuildOptions, Reporter, SummaryLog, SystemTime,
    WorkspaceInstallSelection, build_modules_manifest, build_workspace_state,
    current_contains_dep_path, drain_settled_projects, merge_filtered_modules_metadata,
    merge_pending_builds, project_lifecycle_graph, project_requires_lifecycle_scripts,
    projects_running_own_scripts, run_projects_lifecycle_scripts, update_workspace_state,
    write_modules_manifest,
};
use crate::{
    optimistic_repeat_install::filesystem_now_ms,
    peer_dependency_issues::report_peer_dependency_issues,
};
use pnpm_store_dir::VerifiedFileIntegrity;
use std::time::Duration;

struct ResolveOnlyCompletionInputs<'a> {
    resolve_only: bool,
    dry_run: bool,
    peer_issues_sink_is_none: bool,
    existing_wanted_lockfile: Option<&'a Lockfile>,
    peer_issue_importer_ids: &'a HashSet<String>,
    fresh_lockfile: Option<&'a Lockfile>,
    prefix: &'a str,
    config: &'static Config,
    catalogs: Option<&'a Catalogs>,
    workspace_root: &'a Path,
    installed_importer_ids: &'a HashSet<String>,
}

fn complete_resolve_only<Reporter: self::Reporter>(
    inputs: &ResolveOnlyCompletionInputs<'_>,
) -> Result<bool, InstallError> {
    if !inputs.resolve_only {
        return Ok(false);
    }

    // A sink-driven dry run is a programmatic query, not a CLI preview.
    if inputs.dry_run && inputs.peer_issues_sink_is_none {
        use std::io::Write as _;
        let report =
            crate::lockfile_diff::render_dry_run_report(&crate::lockfile_diff::diff_lockfiles(
                inputs.existing_wanted_lockfile,
                inputs.fresh_lockfile,
                crate::lockfile_diff::ImporterDiffKey::Specifier,
            ));
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{report}");
        let _ = stdout.flush();
    }
    // A programmatic peer-issue query asks for the issues; it must not
    // also be told about them, nor fail over them.
    if inputs.peer_issues_sink_is_none {
        report_peer_dependency_issues::<Reporter>(
            inputs.fresh_lockfile,
            inputs.peer_issue_importer_ids,
            inputs.installed_importer_ids,
            inputs.workspace_root,
            inputs.config,
            inputs.catalogs,
        )?;
    }
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: inputs.prefix.to_string(),
    }));
    Ok(true)
}

struct SelectMaterializedStateInputs<'a> {
    fresh_lockfile: Option<&'a Lockfile>,
    loaded_wanted_lockfile: Option<&'a Lockfile>,
    requested_importer_ids: Option<&'a HashSet<String>>,
    real_importer_ids: &'a HashSet<String>,
    workspace_root: &'a Path,
    included: IncludedDependencies,
    install_skipped: &'a crate::SkippedSnapshots,
    node_linker: NodeLinker,
    current_lockfile: Option<&'a Lockfile>,
    is_inconsistent: bool,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

struct MaterializedState<'a> {
    wanted_lockfile: Option<&'a Lockfile>,
    selected_current_lockfile: Option<Lockfile>,
    current_lockfile: Option<Lockfile>,
    project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
}

fn select_materialized_state<'a>(
    inputs: &SelectMaterializedStateInputs<'a>,
) -> MaterializedState<'a> {
    let wanted_lockfile = inputs.fresh_lockfile.or(inputs.loaded_wanted_lockfile);
    let selected_current_lockfile = wanted_lockfile.and_then(|wanted| {
        inputs.requested_importer_ids.map(|requested| {
            crate::materialization_closure(
                wanted,
                inputs.workspace_root,
                requested,
                inputs.included,
                inputs.install_skipped,
            )
            .lockfile
        })
    });
    let current_lockfile = wanted_lockfile.map(|wanted| {
        if inputs.requested_importer_ids.is_some()
            && matches!(inputs.node_linker, NodeLinker::Hoisted)
        {
            crate::filter_lockfile_for_current(wanted, inputs.included, inputs.install_skipped)
        } else if let Some(requested_importer_ids) = inputs.requested_importer_ids {
            crate::merge_filtered_current_lockfile(
                (!inputs.is_inconsistent).then_some(inputs.current_lockfile).flatten(),
                wanted,
                requested_importer_ids,
                inputs.included,
                inputs.install_skipped,
                inputs.workspace_root,
            )
        } else {
            crate::filter_lockfile_for_current(wanted, inputs.included, inputs.install_skipped)
        }
    });
    let project_anchor_importer_ids = match inputs.requested_importer_ids {
        Some(requested) if matches!(inputs.node_linker, NodeLinker::Hoisted) => requested.clone(),
        Some(requested) => wanted_lockfile.map_or_else(
            || requested.clone(),
            |wanted| {
                crate::materialization_closure(
                    wanted,
                    inputs.workspace_root,
                    requested,
                    inputs.included,
                    inputs.install_skipped,
                )
                .importer_ids
            },
        ),
        None => inputs.real_importer_ids.clone(),
    };
    let project_manifests = inputs
        .project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            let importer_id =
                pnpm_workspace::importer_id_from_root_dir(inputs.workspace_root, project_dir);
            project_anchor_importer_ids.contains(&importer_id)
        })
        .cloned()
        .collect();

    MaterializedState {
        wanted_lockfile,
        selected_current_lockfile,
        current_lockfile,
        project_manifests,
    }
}

struct LinkMaterializedProjectsInputs<'a> {
    filtered_install: bool,
    node_linker: NodeLinker,
    config: &'static Config,
    current_lockfile: Option<&'a Lockfile>,
    wanted_lockfile: Option<&'a Lockfile>,
    workspace_root: &'a Path,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    materialized_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

async fn link_materialized_projects<Reporter: self::Reporter + 'static>(
    inputs: LinkMaterializedProjectsInputs<'_>,
) -> Result<(), InstallError> {
    let LinkMaterializedProjectsInputs {
        filtered_install,
        node_linker,
        config,
        current_lockfile: materialized_current_lockfile,
        wanted_lockfile,
        workspace_root,
        project_manifests,
        materialized_project_manifests,
    } = inputs;
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
            Some(major) => Some(pnpm_graph_hasher::engine_name(major, None, None)),
            None if config.enable_global_virtual_store => tokio::task::spawn_blocking(|| {
                pnpm_graph_hasher::detect_node_major()
                    .map(|major| pnpm_graph_hasher::engine_name(major, None, None))
            })
            .await
            .ok()
            .flatten(),
            None => None,
        };
        let allow_build_policy = crate::AllowBuildPolicy::from_config(config)
            .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)
            .map_err(InstallError::WithFreshLockfile)?;
        let layout = crate::VirtualStoreLayout::new(
            config,
            engine_name.as_deref(),
            current.snapshots.as_ref(),
            current.packages.as_ref(),
            Some(&allow_build_policy),
            Some(workspace_root),
        );
        crate::package_map::write_package_map(
            current,
            &crate::package_map::PackageMapOptions {
                lockfile_dir: workspace_root,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout: &layout,
                project_manifests,
            },
        )
        .map_err(InstallError::WritePackageMap)?;
    }

    // Materialize `link:` direct deps straight from the in-memory
    // project manifests. `excludeLinksFromLockfile` keeps them out
    // of the lockfile importers, so the lockfile-driven symlink
    // passes cannot see them. Aliases the wanted lockfile *does*
    // track are skipped — those belong to the lockfile passes (and
    // their dedupe decisions). See [`crate::link_manifest_link_deps`].
    // These are importer symlinks like any other, so
    // `virtualStoreOnly` skips them too.
    if !config.virtual_store_only {
        crate::link_manifest_link_deps::<Reporter>(
            workspace_root,
            materialized_project_manifests,
            wanted_lockfile.and_then(|lockfile| {
                (!lockfile.importers.is_empty()).then_some(&lockfile.importers)
            }),
            // Honor a `modulesDir` override the same way the
            // lockfile-driven symlink pass does.
            config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules")),
            &crate::shim_link_options(config, node_linker),
        )
        .map_err(InstallError::LinkManifestLinkDeps)?;
    }

    Ok(())
}

struct CommitModulesStateInputs<'a> {
    prior_modules: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    config: &'static Config,
    workspace_root: &'a Path,
    materialized_current_lockfile: Option<&'a Lockfile>,
    selected_current_lockfile: Option<&'a Lockfile>,
    materialized_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    included: IncludedDependencies,
    install_skipped: &'a crate::SkippedSnapshots,
    node_linker: NodeLinker,
    filtered_install: bool,
    is_inconsistent: bool,
    previous_modules_metadata: Option<&'a Modules>,
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    injected_deps: BTreeMap<String, Vec<String>>,
    ignored_builds: &'a [String],
    deferred_builds: Vec<String>,
    rebuild: Option<&'a RebuildOptions>,
    take_frozen_path: bool,
    lockfile_synthesized_from_current: bool,
    lockfile_was_fast_updated: bool,
    save_lockfile: bool,
    loaded_wanted_lockfile: Option<&'a Lockfile>,
}

fn commit_modules_state(inputs: CommitModulesStateInputs<'_>) -> Result<(), InstallError> {
    let CommitModulesStateInputs {
        prior_modules,
        config,
        workspace_root,
        materialized_current_lockfile,
        selected_current_lockfile,
        materialized_project_manifests,
        included,
        install_skipped,
        node_linker,
        filtered_install,
        is_inconsistent,
        previous_modules_metadata,
        hoisted_dependencies,
        hoisted_locations,
        injected_deps,
        ignored_builds,
        deferred_builds,
        rebuild,
        take_frozen_path,
        lockfile_synthesized_from_current,
        lockfile_was_fast_updated,
        save_lockfile,
        loaded_wanted_lockfile,
    } = inputs;
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
                        install_skipped,
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
                pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir)
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
    let allow_build_policy = (rebuild.is_some()
        || (prior_modules.is_some() && materialized_current_lockfile.is_some()))
    .then(|| crate::AllowBuildPolicy::from_config(config))
    .transpose()
    .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)
    .map_err(InstallError::WithFreshLockfile)?;
    let pending_builds = merge_pending_builds(
        previous_pending_builds,
        deferred_projects.into_iter().flatten().chain(deferred_builds),
        materialized_current_lockfile,
        rebuild,
        allow_build_policy.as_ref(),
    );

    // Rebuild reads hoisted locations from `.modules.yaml` and reports
    // `MISSING_HOISTED_LOCATIONS` if an install fails to persist them here.
    let mut next_modules = build_modules_manifest(
        config,
        node_linker,
        included,
        hoisted_dependencies,
        hoisted_locations,
        injected_deps,
        install_skipped,
        ignored_builds,
        pending_builds,
        pruned_at,
    );
    if let (Some(previous), Some(current), Some(policy)) =
        (prior_modules, materialized_current_lockfile, allow_build_policy.as_ref())
    {
        retain_current_ignored_builds(&mut next_modules, previous, current, policy);
    }
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
        && (lockfile_synthesized_from_current
            || lockfile_was_fast_updated
            || config.merge_git_branch_lockfiles)
        && config.lockfile
        && save_lockfile
        && let Some(updated) = loaded_wanted_lockfile
    {
        updated
            .save_to_path(&workspace_root.join(config.wanted_lockfile_name()))
            .map_err(InstallError::SaveWantedLockfile)?;
    }

    Ok(())
}

fn retain_current_ignored_builds(
    next: &mut Modules,
    previous: &pnpm_modules_yaml::ModulesLayout,
    current: &Lockfile,
    allow_build_policy: &crate::AllowBuildPolicy,
) {
    let Some(previous_ignored) = previous.ignored_builds.as_ref() else { return };
    for dep_path in previous_ignored {
        if current_contains_dep_path(current, dep_path.as_str())
            && allow_build_policy.check(dep_path.as_str()).is_none()
        {
            next.ignored_builds.get_or_insert_default().insert(dep_path.clone());
        }
    }
}

#[derive(Clone, Copy)]
struct MaterializedProjectScriptsInputs<'a, 'selection> {
    config: &'static Config,
    node_linker: NodeLinker,
    workspace_root: &'a Path,
    rebuild: Option<&'a RebuildOptions>,
    mutation: ProjectMutation,
    manifest_dir: &'a Path,
    selection: Option<&'a WorkspaceInstallSelection<'selection>>,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    materialized_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    materialized_current_lockfile: Option<&'a Lockfile>,
}

fn run_materialized_project_scripts<Reporter: self::Reporter>(
    inputs: MaterializedProjectScriptsInputs<'_, '_>,
) -> Result<(), InstallError> {
    let MaterializedProjectScriptsInputs {
        config,
        node_linker,
        workspace_root,
        rebuild,
        mutation,
        manifest_dir,
        selection,
        project_manifests,
        materialized_project_manifests,
        materialized_current_lockfile,
    } = inputs;
    // A `pnpm rebuild --pending` is the exception to the mutation
    // gate: it installs no manifest, but the projects it names are
    // exactly the ones whose scripts an earlier `--ignore-scripts`
    // install deferred, and running them is what lets the install
    // drop those entries from `pendingBuilds` (see
    // [`merge_pending_builds`]).
    let projects_to_run: Vec<(std::path::PathBuf, &PackageManifest)> =
        if config.ignore_scripts || config.virtual_store_only {
            Vec::new()
        } else if let Some(rebuild) = rebuild {
            materialized_project_manifests
                .iter()
                .filter(|(project_dir, _)| {
                    let importer_id =
                        pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir);
                    rebuild.pending_projects.contains(&importer_id)
                })
                .cloned()
                .collect()
        } else {
            projects_running_own_scripts(&ProjectScriptsInputs {
                mutation,
                workspace_root,
                active_project_dir: manifest_dir,
                selected_dirs: selection.map(|selection| selection.selected_dirs),
                project_manifests,
                materialized_project_manifests,
            })
        };
    if !projects_to_run.is_empty() {
        let project_graph = project_lifecycle_graph(
            &projects_to_run,
            selection.map(|selection| selection.project_dependencies),
            workspace_root,
            materialized_current_lockfile,
        )?;
        if !project_graph.dependencies.is_empty() {
            run_projects_lifecycle_scripts::<Reporter>(
                &project_graph,
                config,
                node_linker,
                workspace_root,
            )?;
        }
        if let Some(rebuild) = rebuild {
            drain_settled_projects::<Host>(&config.modules_dir, &rebuild.pending_projects)?;
        }
    }

    Ok(())
}

struct ReportInstallCompletionInputs<'a> {
    config: &'static Config,
    catalogs: Option<&'a Catalogs>,
    workspace_root: &'a Path,
    workspace_manifest_dir: &'a Path,
    prefix: String,
    ignored_builds: Vec<String>,
    verified_file_integrity_baseline: VerifiedFileIntegrity,
    /// The lockfile this install resolved, or `None` when it skipped
    /// resolution — which is what decides whether peer-dependency
    /// issues are reported at all.
    resolved_lockfile: Option<&'a Lockfile>,
    peer_issue_importer_ids: &'a HashSet<String>,
    installed_importer_ids: &'a HashSet<String>,
}

fn report_install_completion<Reporter: self::Reporter>(
    inputs: ReportInstallCompletionInputs<'_>,
) -> Result<(), InstallError> {
    let ReportInstallCompletionInputs {
        config,
        catalogs,
        workspace_root,
        workspace_manifest_dir,
        prefix,
        ignored_builds,
        verified_file_integrity_baseline,
        resolved_lockfile,
        peer_issue_importer_ids,
        installed_importer_ids,
    } = inputs;
    // Reported before the summary and before the ignored-builds
    // failure below, matching where pnpm places the verdict: last in
    // the install, first among the ways it can still fail.
    report_peer_dependency_issues::<Reporter>(
        resolved_lockfile,
        peer_issue_importer_ids,
        installed_importer_ids,
        workspace_root,
        config,
        catalogs,
    )?;
    // `pnpm:summary` closes the install and lets the reporter render
    // the accumulated `pnpm:root` events as a "+N -M" block. Must
    // come after `importing_done`.
    Reporter::emit(&LogEvent::Summary(SummaryLog { level: LogLevel::Debug, prefix }));

    report_verified_file_integrity::<Reporter>(
        VerifiedFileIntegrity::snapshot().since(verified_file_integrity_baseline),
    );

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
    // `--ignore-workspace` opts out: the run disowned the workspace
    // manifest, so it must not write to one either.
    if !ignored_builds.is_empty() && !is_global_install && !config.ignore_workspace {
        let allow_build_keys: BTreeSet<String> = ignored_builds
            .iter()
            .map(|dep_path| crate::allow_build_key_from_ignored_build(dep_path))
            .collect();
        pnpm_workspace_manifest_writer::scaffold_allow_builds(
            workspace_manifest_dir,
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

/// Spending this long re-hashing store files is well past what a
/// healthy store needs, so the install owns up to the time.
const VERIFIED_FILE_INTEGRITY_SLOW: Duration = Duration::from_secs(1);

/// Re-hashing this many files says something keeps invalidating the
/// store even when the hashing itself was quick — worth telling the
/// user about before the store grows and the same churn does cost them
/// time.
const VERIFIED_FILE_INTEGRITY_MANY: u64 = 1000;

/// Tell the user when store verification re-hashed files: how much time
/// it cost, or failing that, that it happened at all on a scale a
/// healthy store never reaches. The two are separate claims, so they
/// are separate messages, and the timed one wins when both hold — it
/// carries the file count anyway.
///
/// `verified` covers this install alone, and its `duration` is summed
/// across the threads that did the hashing — see
/// [`pnpm_store_dir::VerifiedFileIntegrity`].
///
/// The seconds are rounded to tenths in integer arithmetic rather than
/// by float formatting: pnpm renders the same messages from the same
/// figures and the two have to agree character for character, but
/// Rust's `{:.1}` rounds a tie to even where JavaScript's `toFixed`
/// rounds it up.
pub(super) fn report_verified_file_integrity<Reporter: self::Reporter>(
    verified: VerifiedFileIntegrity,
) {
    let files = verified.files;
    let message = if verified.duration > VERIFIED_FILE_INTEGRITY_SLOW {
        let tenths = (verified.duration.as_millis() + 50) / 100;
        format!("The integrity of {files} files was checked in {}.{}s.", tenths / 10, tenths % 10)
    } else if files > VERIFIED_FILE_INTEGRITY_MANY {
        format!(
            "The integrity of {files} files was checked, because their timestamps changed since the store recorded them. A backup tool, an antivirus scan, or a copied store can cause this.",
        )
    } else {
        return;
    };
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Info, message }));
}

pub(super) struct ApplyMaterializationInputs<'a, 'selection> {
    pub(super) resolve_only: bool,
    pub(super) dry_run: bool,
    pub(super) peer_issues_sink_is_none: bool,
    pub(super) existing_wanted_lockfile: Option<&'a Lockfile>,
    pub(super) peer_issue_importer_ids: HashSet<String>,
    pub(super) fresh_lockfile: Option<Lockfile>,
    pub(super) prefix: String,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) requested_importer_ids: Option<HashSet<String>>,
    pub(super) workspace_root: PathBuf,
    pub(super) workspace_manifest_dir: PathBuf,
    pub(super) included: IncludedDependencies,
    pub(super) install_skipped: crate::SkippedSnapshots,
    pub(super) node_linker: NodeLinker,
    pub(super) current_lockfile: Option<Lockfile>,
    pub(super) real_importer_ids: HashSet<String>,
    pub(super) project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
    pub(super) filtered_install: bool,
    pub(super) is_inconsistent: bool,
    pub(super) previous_modules_metadata: Option<Modules>,
    pub(super) config: &'static Config,
    pub(super) hoisted_dependencies: HoistedDependencies,
    pub(super) hoisted_locations: BTreeMap<String, Vec<String>>,
    pub(super) injected_deps: BTreeMap<String, Vec<String>>,
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) modules_manifest: Option<pnpm_modules_yaml::ModulesLayout>,
    pub(super) rebuild: Option<RebuildOptions>,
    pub(super) take_frozen_path: bool,
    pub(super) lockfile_synthesized_from_current: bool,
    pub(super) lockfile_was_fast_updated: bool,
    pub(super) save_lockfile: bool,
    pub(super) mutation: ProjectMutation,
    pub(super) manifest_dir: &'a Path,
    pub(super) selection: Option<WorkspaceInstallSelection<'selection>>,
    pub(super) supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) catalogs: Catalogs,
    pub(super) catalog_context_present: bool,
    pub(super) verified_file_integrity_baseline: VerifiedFileIntegrity,
}

pub(super) async fn apply_materialization_result<Reporter: self::Reporter + 'static>(
    inputs: ApplyMaterializationInputs<'_, '_>,
) -> Result<(), InstallError> {
    let ApplyMaterializationInputs {
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
        modules_manifest,
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
    } = inputs;
    let peer_catalogs = catalog_context_present.then_some(&catalogs);
    let modules_manifest = modules_manifest.as_ref();
    // What this run installed: a `--filter`ed install acts only on its
    // selection, every other one on the whole workspace. The lockfile
    // can hold more — importers a filtered run left alone, or ones
    // `pruneLockfileImporters` has yet to drop.
    let installed_importer_ids = requested_importer_ids.as_ref().unwrap_or(&real_importer_ids);
    tracing::info!(target: "pacquet::install", "Complete all");

    if complete_resolve_only::<Reporter>(&ResolveOnlyCompletionInputs {
        resolve_only,
        dry_run,
        peer_issues_sink_is_none,
        existing_wanted_lockfile,
        peer_issue_importer_ids: &peer_issue_importer_ids,
        fresh_lockfile: fresh_lockfile.as_ref(),
        prefix: &prefix,
        config,
        catalogs: peer_catalogs,
        workspace_root: &workspace_root,
        installed_importer_ids,
    })? {
        return Ok(());
    }

    let MaterializedState {
        wanted_lockfile: materialized_wanted_lockfile,
        selected_current_lockfile,
        current_lockfile: materialized_current_lockfile,
        project_manifests: materialized_project_manifests,
    } = select_materialized_state(&SelectMaterializedStateInputs {
        fresh_lockfile: fresh_lockfile.as_ref(),
        loaded_wanted_lockfile: lockfile,
        requested_importer_ids: requested_importer_ids.as_ref(),
        real_importer_ids: &real_importer_ids,
        workspace_root: &workspace_root,
        included,
        install_skipped: &install_skipped,
        node_linker,
        current_lockfile: current_lockfile.as_ref(),
        is_inconsistent,
        project_manifests: &project_manifests,
    });

    link_materialized_projects::<Reporter>(LinkMaterializedProjectsInputs {
        filtered_install,
        node_linker,
        config,
        current_lockfile: materialized_current_lockfile.as_ref(),
        wanted_lockfile: materialized_wanted_lockfile,
        workspace_root: &workspace_root,
        project_manifests: &project_manifests,
        materialized_project_manifests: &materialized_project_manifests,
    })
    .await?;

    commit_modules_state(CommitModulesStateInputs {
        prior_modules: modules_manifest,
        config,
        workspace_root: &workspace_root,
        materialized_current_lockfile: materialized_current_lockfile.as_ref(),
        selected_current_lockfile: selected_current_lockfile.as_ref(),
        materialized_project_manifests: &materialized_project_manifests,
        included,
        install_skipped: &install_skipped,
        node_linker,
        filtered_install,
        is_inconsistent,
        previous_modules_metadata: previous_modules_metadata.as_ref(),
        hoisted_dependencies,
        hoisted_locations,
        injected_deps,
        ignored_builds: &ignored_builds,
        deferred_builds,
        rebuild: rebuild.as_ref(),
        take_frozen_path,
        lockfile_synthesized_from_current,
        lockfile_was_fast_updated,
        save_lockfile,
        loaded_wanted_lockfile: lockfile,
    })?;

    run_materialized_project_scripts::<Reporter>(MaterializedProjectScriptsInputs {
        config,
        node_linker,
        workspace_root: &workspace_root,
        rebuild: rebuild.as_ref(),
        mutation,
        manifest_dir,
        selection: selection.as_ref(),
        project_manifests: &project_manifests,
        materialized_project_manifests: &materialized_project_manifests,
        materialized_current_lockfile: materialized_current_lockfile.as_ref(),
    })?;

    // Nothing below reads the materialized lockfiles, and each holds a
    // workspace-scale importer map.
    pnpm_fs::background_drop((
        selected_current_lockfile,
        materialized_current_lockfile,
        current_lockfile,
    ));

    // Write `node_modules/.pnpm-workspace-state-v1.json`.
    // pnpm's `verifyDepsBeforeRun` gate bails to "outdated" the
    // moment this file is missing, forcing `pnpm install` to rerun.
    // Writing it after both the `.modules.yaml` and the current
    // lockfile succeed keeps the file pointing at a fully committed
    // install.
    update_workspace_state(
        &workspace_root,
        &build_workspace_state::<Host>(
            &workspace_root,
            config,
            node_linker,
            included,
            supported_architectures.as_ref(),
            &catalogs,
            &project_manifests,
            filtered_install,
            filesystem_now_ms(&workspace_root),
        ),
    )
    .map_err(InstallError::WriteWorkspaceState)?;

    let completion = report_install_completion::<Reporter>(ReportInstallCompletionInputs {
        config,
        catalogs: peer_catalogs,
        workspace_root: &workspace_root,
        workspace_manifest_dir: &workspace_manifest_dir,
        prefix,
        ignored_builds,
        verified_file_integrity_baseline,
        resolved_lockfile: fresh_lockfile.as_ref(),
        peer_issue_importer_ids: &peer_issue_importer_ids,
        installed_importer_ids,
    });
    pnpm_fs::background_drop(fresh_lockfile);
    completion
}
