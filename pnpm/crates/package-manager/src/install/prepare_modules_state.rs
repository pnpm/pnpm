use super::{
    Arc, Catalogs, Config, HashSet, Host, IncludedDependencies, InstallError, Lockfile, LogEvent,
    LogLevel, Modules, NodeLinker, PackageManifest, Path, PathBuf, PnpmLog, RebuildOptions,
    Reporter, ResolutionVerifier, Stage, StageLog, SummaryLog, SystemTime, build_workspace_state,
    check_modules_settings_diff, frozen_tree_intact, gvs_build_marker_present,
    has_newly_allowed_ignored_builds, has_revoked_allowed_builds, map_frozen_lockfile_error,
    modules_consistent_with, modules_layout_consistent_with, unapproved_recorded_ignored_builds,
    update_workspace_state, verify_lockfile_eagerly,
};
use crate::optimistic_repeat_install::filesystem_now_ms;

pub(super) struct PrepareModulesStateInputs<'a, 'install> {
    pub(super) resolve_only: bool,
    pub(super) take_frozen_path: bool,
    pub(super) config: &'static Config,
    pub(super) filtered_install: bool,
    pub(super) installs_only: bool,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) current_lockfile: Option<&'a Lockfile>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) node_linker: NodeLinker,
    pub(super) disable_optimistic_repeat_install: bool,
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    pub(super) derived_lockfile_path: Option<&'a Path>,
    pub(super) lockfile_verification_override:
        Option<super::LockfileVerificationOverride<'install>>,
    pub(super) lockfile_synthesized_from_current: bool,
    pub(super) lockfile_was_fast_updated: bool,
    pub(super) save_lockfile: bool,
    pub(super) catalogs: &'a Catalogs,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) effective_node_version: Option<&'a str>,
    pub(super) prefix: &'a str,
}

pub(super) struct PreparedModulesState<'install> {
    pub(super) old_modules: Option<pnpm_modules_yaml::ModulesLayout>,
    pub(super) previous_modules_metadata: Option<Modules>,
    pub(super) is_inconsistent: bool,
    pub(super) lockfile_verification_override:
        Option<super::LockfileVerificationOverride<'install>>,
}

pub(super) fn prior_hoisted_dependencies(
    previous_modules_metadata: Option<&Modules>,
) -> Option<&super::HoistedDependencies> {
    previous_modules_metadata.map(|modules| &modules.hoisted_dependencies)
}

pub(super) fn prior_hoisted_locations(
    previous_modules_metadata: Option<&Modules>,
) -> Option<&pnpm_deps_restorer::HoistedLocations> {
    previous_modules_metadata.and_then(|modules| modules.hoisted_locations.as_ref())
}

/// Returns `Ok(None)` after completing an up-to-date install; the caller must return successfully
/// without materializing.
pub(super) async fn prepare_modules_state<'install, Reporter: self::Reporter + 'static>(
    inputs: PrepareModulesStateInputs<'_, 'install>,
) -> Result<Option<PreparedModulesState<'install>>, InstallError> {
    let old_modules =
        read_old_modules(inputs.resolve_only, inputs.take_frozen_path, inputs.config)?;
    let modules_manifest = old_modules.as_ref();
    let previous_modules_metadata = read_previous_modules_metadata(
        inputs.resolve_only,
        inputs.filtered_install,
        inputs.config,
    )?;
    let is_inconsistent =
        modules_layout_drifted(modules_manifest, inputs.config, inputs.node_linker);

    if !inputs.resolve_only && is_inconsistent {
        purge_inconsistent_modules_dir(&InconsistentModulesDir {
            config: inputs.config,
            workspace_root: inputs.workspace_root,
            modules_manifest,
            installs_only: inputs.installs_only,
            filtered_install: inputs.filtered_install,
        })?;
    }

    prune_excluded_direct_deps(&ExcludedGroupPrune {
        resolve_only: inputs.resolve_only,
        is_inconsistent,
        filtered_install: inputs.filtered_install,
        config: inputs.config,
        workspace_root: inputs.workspace_root,
        included: inputs.included,
        modules_manifest,
        current_lockfile: inputs.current_lockfile,
        requested_importer_ids: inputs.requested_importer_ids,
    })?;

    let up_to_date = FrozenTreeUpToDate {
        take_frozen_path: inputs.take_frozen_path,
        filtered_install: inputs.filtered_install,
        disable_optimistic_repeat_install: inputs.disable_optimistic_repeat_install,
        config: inputs.config,
        workspace_root: inputs.workspace_root,
        node_linker: inputs.node_linker,
        included: inputs.included,
        lockfile: inputs.lockfile,
        current_lockfile: inputs.current_lockfile,
        modules_manifest,
        supported_architectures: inputs.supported_architectures,
        rebuild: inputs.rebuild,
        effective_node_version: inputs.effective_node_version,
    };
    if let Some((wanted_lockfile, modules)) = frozen_tree_up_to_date(&up_to_date) {
        report_up_to_date::<Reporter>(UpToDateInstall {
            config: inputs.config,
            workspace_root: inputs.workspace_root,
            node_linker: inputs.node_linker,
            included: inputs.included,
            wanted_lockfile,
            modules,
            supported_architectures: inputs.supported_architectures,
            catalogs: inputs.catalogs,
            project_manifests: inputs.project_manifests,
            filtered_install: inputs.filtered_install,
            prefix: inputs.prefix,
            resolution_verifiers: inputs.resolution_verifiers,
            derived_lockfile_path: inputs.derived_lockfile_path,
            lockfile_verification_override: inputs.lockfile_verification_override,
            lockfile_synthesized_from_current: inputs.lockfile_synthesized_from_current,
            lockfile_was_fast_updated: inputs.lockfile_was_fast_updated,
            save_lockfile: inputs.save_lockfile,
        })
        .await?;
        return Ok(None);
    }

    Ok(Some(PreparedModulesState {
        old_modules,
        previous_modules_metadata,
        is_inconsistent,
        lockfile_verification_override: inputs.lockfile_verification_override,
    }))
}

/// Whether any package in the lockfile resolves to a local directory.
///
/// Such a package's source is mutable between installs, so its
/// materialized copy can go stale while every install-state artifact
/// still says the tree is current.
fn has_directory_snapshot(lockfile: &Lockfile) -> bool {
    lockfile.packages.iter().flat_map(|packages| packages.values()).any(|metadata| {
        matches!(metadata.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
    })
}

fn is_safe_modules_purge_target(modules_dir: &Path, workspace_root: &Path) -> bool {
    modules_dir != workspace_root && modules_dir.starts_with(workspace_root)
}

#[cfg(test)]
mod tests;

/// A no-op still refreshes workspace state so `verifyDepsBeforeRun` does not
/// treat the materialized tree as stale.
///
/// An unreadable state file fails the install rather than reading as layout
/// drift: the drift path purges `node_modules` — the entries the user keeps
/// there included — and relinks the whole tree, and it would do so on every
/// run, because the manifest it rewrites is no more readable than the one it
/// replaced. The TypeScript CLI's `readModulesManifest` rethrows everything
/// but `ENOENT` for the same reason.
fn read_old_modules(
    resolve_only: bool,
    take_frozen_path: bool,
    config: &Config,
) -> Result<Option<pnpm_modules_yaml::ModulesLayout>, InstallError> {
    if resolve_only && !take_frozen_path {
        return Ok(None);
    }
    pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir)
        .map_err(InstallError::ReadModules)
}

/// A filtered install rewrites `.modules.yaml` from the selected projects'
/// state merged over the previous file's, so losing the previous contents
/// would drop every unselected project's entries. A file whose layout parses
/// while some later field does not would otherwise merge against `None` and
/// silently prune those entries, so that case fails instead.
fn read_previous_modules_metadata(
    resolve_only: bool,
    filtered_install: bool,
    config: &Config,
) -> Result<Option<Modules>, InstallError> {
    if resolve_only {
        return Ok(None);
    }
    match pnpm_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir) {
        Ok(modules) => Ok(modules),
        // A filtered install merges the unselected importers' entries out of
        // this file, so it cannot proceed without it; an unfiltered install
        // only loses the orphan hoist-link cleanup.
        Err(error) if filtered_install => Err(InstallError::ReadModules(error)),
        Err(error) => {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "failed to fully parse .modules.yaml; skipping orphan hoist-link cleanup",
            );
            Ok(None)
        }
    }
}

/// The purge keys off *layout* drift only, not `included`: an included
/// (`--prod` <-> full) change is handled by relinking, so it must not wipe the
/// user's `node_modules` contents. See [`modules_layout_consistent_with`].
fn modules_layout_drifted(
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
    config: &Config,
    node_linker: NodeLinker,
) -> bool {
    let Some(modules) = modules_manifest else {
        // Treat existence-check errors conservatively as inconsistent.
        return config
            .modules_dir
            .join(pnpm_modules_yaml::MODULES_FILENAME)
            .try_exists()
            .unwrap_or(true);
    };
    !modules_layout_consistent_with(modules, config, node_linker)
}

/// The modules directory a drifted layout has to be rebuilt from.
struct InconsistentModulesDir<'a> {
    config: &'static Config,
    workspace_root: &'a Path,
    modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    installs_only: bool,
    filtered_install: bool,
}

fn purge_inconsistent_modules_dir(
    context: &InconsistentModulesDir<'_>,
) -> Result<(), InstallError> {
    // A plain install may recreate the drifted modules dir; `add` / `remove`
    // must surface the drift instead (upstream `validateModules` with
    // `forceNewModules = installsOnly`).
    if !context.installs_only
        && let Some(modules) = context.modules_manifest
    {
        check_modules_settings_diff(modules, context.config)?;
    }
    let (is_safe, target_dir) = purge_target(context.config, context.workspace_root);
    if !is_safe {
        if context.filtered_install {
            return Err(InstallError::UnsafeFilteredModulesDir {
                modules_dir: context.config.modules_dir.clone(),
                workspace_root: context.workspace_root.to_path_buf(),
            });
        }
        tracing::warn!(
            ?context.config.modules_dir,
            "refusing to remove inconsistent modules directory outside the project root",
        );
        return Ok(());
    }
    let Some(target) = target_dir else { return Ok(()) };
    purge_modules_dir_entries(&target, context.config, context.modules_manifest)
}

/// The canonicalized directory the purge may sweep, and whether sweeping it is
/// safe at all. Deleting from the validated path closes the
/// time-of-check/time-of-use gap a symlink swap would otherwise open.
fn purge_target(config: &Config, workspace_root: &Path) -> (bool, Option<PathBuf>) {
    if !config.modules_dir.exists() {
        return (true, None);
    }
    match (std::fs::canonicalize(&config.modules_dir), std::fs::canonicalize(workspace_root)) {
        (Ok(modules_canon), Ok(root_canon)) => {
            (is_safe_modules_purge_target(&modules_canon, &root_canon), Some(modules_canon))
        }
        _ => (false, None),
    }
}

fn purge_modules_dir_entries(
    target: &Path,
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> Result<(), InstallError> {
    let read_error = |error| InstallError::ReadModulesDir { path: target.to_path_buf(), error };
    let entries = match std::fs::read_dir(target) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(read_error(error)),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(read_error(error)),
        };
        purge_modules_entry(&entry, config, modules_manifest)?;
    }
    Ok(())
}

/// A hidden entry pnpm does not own is the user's, and is left alone.
fn purge_modules_entry(
    entry: &std::fs::DirEntry,
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> Result<(), InstallError> {
    let file_name = entry.file_name();
    let file_name_str = file_name.to_string_lossy();
    if file_name_str.starts_with('.')
        && !is_pnpm_owned_entry(&file_name_str, config, modules_manifest)
    {
        return Ok(());
    }
    let entry_path = entry.path();
    if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
        return remove_modules_dir(&entry_path);
    }
    remove_modules_file(&entry_path)
}

fn is_pnpm_owned_entry(
    file_name: &str,
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> bool {
    file_name == ".bin"
        || file_name == ".modules.yaml"
        || config.virtual_store_dir.file_name().is_some_and(|name| name == file_name)
        || modules_manifest.is_some_and(|manifest| {
            recorded_virtual_store_name(manifest, config).is_some_and(|name| name == file_name)
        })
}

/// The `node_modules`-relative name the recorded virtual store occupies, when
/// it is inside the modules directory at all.
fn recorded_virtual_store_name(
    manifest: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Option<std::ffi::OsString> {
    let mut recorded = PathBuf::from(&manifest.virtual_store_dir);
    if recorded.is_relative() {
        recorded = config.modules_dir.join(recorded);
    }
    if !recorded.starts_with(&config.modules_dir) {
        return None;
    }
    recorded.file_name().map(std::ffi::OsStr::to_os_string)
}

fn remove_modules_dir(entry_path: &Path) -> Result<(), InstallError> {
    #[cfg(windows)]
    let is_removed = pnpm_fs::remove_symlink_dir(entry_path).is_ok();
    #[cfg(not(windows))]
    let is_removed = false;

    if !is_removed
        && let Err(error) = std::fs::remove_dir_all(entry_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(InstallError::RemoveModulesDir { path: entry_path.to_path_buf(), error });
    }
    Ok(())
}

fn remove_modules_file(entry_path: &Path) -> Result<(), InstallError> {
    if let Err(error) = std::fs::remove_file(entry_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(InstallError::RemoveModulesDir { path: entry_path.to_path_buf(), error });
    }
    Ok(())
}

/// What decides whether direct links excluded by this run have to be pruned.
struct ExcludedGroupPrune<'a> {
    resolve_only: bool,
    is_inconsistent: bool,
    filtered_install: bool,
    config: &'static Config,
    workspace_root: &'a Path,
    included: IncludedDependencies,
    modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    current_lockfile: Option<&'a Lockfile>,
    requested_importer_ids: Option<&'a HashSet<String>>,
}

/// Remove direct links from dependency groups excluded by this run.
/// Unfiltered installs can use the global `included` value recorded in
/// `.modules.yaml`; filtered installs may retain importers materialized with
/// different group sets, so they conservatively prune every excluded group
/// from only the selected workspace-link closure.
fn prune_excluded_direct_deps(context: &ExcludedGroupPrune<'_>) -> Result<(), InstallError> {
    if context.resolve_only || context.is_inconsistent {
        return Ok(());
    }
    let Some(modules) = context.modules_manifest else { return Ok(()) };
    let Some(current) = context.current_lockfile else { return Ok(()) };
    if !context.filtered_install && modules.included == context.included {
        return Ok(());
    }
    let selected_prune_importer_ids = context.requested_importer_ids.map(|requested| {
        crate::materialization_closure(
            current,
            context.workspace_root,
            requested,
            context.included,
            &crate::SkippedSnapshots::new(),
        )
        .importer_ids
    });
    let previously_included = if context.filtered_install {
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
        context.included,
        context.workspace_root,
        context.config,
        selected_prune_importer_ids.as_ref(),
    )
    .map_err(InstallError::PruneDirectDeps)
}

/// Everything the "nothing to do" verdict rests on.
struct FrozenTreeUpToDate<'a> {
    take_frozen_path: bool,
    filtered_install: bool,
    disable_optimistic_repeat_install: bool,
    config: &'static Config,
    workspace_root: &'a Path,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    lockfile: Option<&'a Lockfile>,
    current_lockfile: Option<&'a Lockfile>,
    modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    rebuild: Option<&'a RebuildOptions>,
    effective_node_version: Option<&'a str>,
}

/// The lockfile and modules manifest of a tree nothing has to be done to, or
/// `None` when the install has to materialize.
fn frozen_tree_up_to_date<'a>(
    context: &FrozenTreeUpToDate<'a>,
) -> Option<(&'a Lockfile, &'a pnpm_modules_yaml::ModulesLayout)> {
    let config = context.config;
    if context.take_frozen_path
        && !context.filtered_install
        && !context.disable_optimistic_repeat_install
        // `--force` reinstalls everything, so an up-to-date tree
        // must not short-circuit the materialization.
        && !config.force
        && let Some(wanted_lockfile) = context.lockfile
        && let Some(current) = context.current_lockfile
        && wanted_lockfile == current
        // A `file:` dependency resolves to a directory whose
        // contents can change with nothing in the lockfile or
        // `.modules.yaml` moving, so an equal-lockfile tree is not
        // evidence that its slot is current. pnpm's `file:` is a
        // copy taken at install time, not a symlink, so the copy
        // has to be retaken; the TypeScript CLI has no gate at this
        // level at all and instead forces every directory dep
        // through materialization in `lockfileToDepGraph`.
        && !has_directory_snapshot(wanted_lockfile)
        && let Some(modules) = context.modules_manifest
        && modules_consistent_with(modules, config, context.node_linker, context.included)
        // A `supportedArchitectures` change alters the skip set
        // without touching the lockfile or `.modules.yaml`, so the
        // unchanged-layout premise doesn't hold and the platform
        // packages must be re-evaluated.
        && crate::optimistic_repeat_install::recorded_supported_architectures_match(
            context.workspace_root,
            context.supported_architectures,
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
        && !gvs_build_marker_present(
            wanted_lockfile,
            config,
            context.workspace_root,
            context.effective_node_version,
        )
        // An explicit `pacquet rebuild` always re-runs the build phase,
        // so it never short-circuits here.
        && context.rebuild.is_none()
        && !modules_cache_prune_due(config, context.modules_manifest)
        && frozen_tree_intact(
            wanted_lockfile,
            modules,
            config,
            context.workspace_root,
            context.node_linker,
        )
    {
        return Some((wanted_lockfile, modules));
    }
    None
}

fn modules_cache_prune_due(
    config: &Config,
    modules_manifest: Option<&pnpm_modules_yaml::ModulesLayout>,
) -> bool {
    modules_manifest.is_some_and(|modules| {
        crate::prune_virtual_store::should_prune_virtual_store(
            crate::prune_virtual_store::same_dir(
                config.effective_virtual_store_dir(),
                &config.global_virtual_store_dir,
            ),
            Some(modules.pruned_at.as_str()),
            config.modules_cache_max_age,
            SystemTime::now(),
        )
    })
}

/// What the up-to-date early return still has to write and report.
struct UpToDateInstall<'a, 'install> {
    config: &'static Config,
    workspace_root: &'a Path,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    wanted_lockfile: &'a Lockfile,
    modules: &'a pnpm_modules_yaml::ModulesLayout,
    supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    catalogs: &'a Catalogs,
    project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    filtered_install: bool,
    prefix: &'a str,
    resolution_verifiers: &'a [Arc<dyn ResolutionVerifier>],
    derived_lockfile_path: Option<&'a Path>,
    lockfile_verification_override: Option<super::LockfileVerificationOverride<'install>>,
    lockfile_synthesized_from_current: bool,
    lockfile_was_fast_updated: bool,
    save_lockfile: bool,
}

async fn report_up_to_date<Reporter: self::Reporter + 'static>(
    context: UpToDateInstall<'_, '_>,
) -> Result<(), InstallError> {
    // The full frozen path runs the offline structural
    // name gate before any materialization; the up-to-date
    // early return must not skip it (the resolution-verifier
    // fan-out below is policy-gated and can be empty).
    pnpm_lockfile_verification::verify_lockfile_dependency_names(context.wanted_lockfile)
        .map_err(InstallError::LockfileVerification)?;
    // Nothing to materialize means no fetch to overlap; verify
    // eagerly before the up-to-date early return.
    verify_up_to_date_lockfile::<Reporter>(
        context.wanted_lockfile,
        context.lockfile_verification_override,
        context.resolution_verifiers,
        (context.derived_lockfile_path, &context.config.cache_dir),
    )
    .await?;
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
    if context.config.strict_dep_builds
        && let Ok(Some(package_names)) =
            unapproved_recorded_ignored_builds(context.modules, context.config)
    {
        return Err(InstallError::IgnoredBuilds { package_names });
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: "Lockfile is up to date, resolution step is skipped".to_string(),
        prefix: context.prefix.to_string(),
    }));
    Reporter::emit(&LogEvent::Stage(StageLog {
        level: LogLevel::Debug,
        prefix: context.prefix.to_string(),
        stage: Stage::ImportingDone,
    }));
    save_merged_wanted_lockfile(
        context.wanted_lockfile,
        context.config,
        context.workspace_root,
        (
            context.lockfile_synthesized_from_current,
            context.lockfile_was_fast_updated,
            context.save_lockfile,
        ),
    )?;
    update_workspace_state(
        context.workspace_root,
        &build_workspace_state::<Host>(
            context.workspace_root,
            context.config,
            context.node_linker,
            context.included,
            context.supported_architectures,
            context.catalogs,
            context.project_manifests,
            context.filtered_install,
            filesystem_now_ms(context.workspace_root),
        ),
    )
    .map_err(InstallError::WriteWorkspaceState)?;
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: context.prefix.to_string(),
    }));
    Ok(())
}

async fn verify_up_to_date_lockfile<Reporter: self::Reporter + 'static>(
    wanted_lockfile: &Lockfile,
    lockfile_verification_override: Option<super::LockfileVerificationOverride<'_>>,
    resolution_verifiers: &[Arc<dyn ResolutionVerifier>],
    paths: (Option<&Path>, &Path),
) -> Result<(), InstallError> {
    let (derived_lockfile_path, cache_dir) = paths;
    let Some(lockfile_verification_override) = lockfile_verification_override else {
        return verify_lockfile_eagerly::<Reporter>(
            wanted_lockfile,
            resolution_verifiers,
            derived_lockfile_path,
            cache_dir,
        )
        .await;
    };
    lockfile_verification_override.await.map_err(map_frozen_lockfile_error)
}

/// A merge produced a lockfile that no file on disk holds, so it has to be
/// written back even when nothing else changed.
fn save_merged_wanted_lockfile(
    wanted_lockfile: &Lockfile,
    config: &Config,
    workspace_root: &Path,
    merge: (bool, bool, bool),
) -> Result<(), InstallError> {
    let (lockfile_synthesized_from_current, lockfile_was_fast_updated, save_lockfile) = merge;
    if (lockfile_synthesized_from_current
        || lockfile_was_fast_updated
        || config.merge_git_branch_lockfiles)
        && config.lockfile
        && save_lockfile
    {
        wanted_lockfile
            .save_to_path(&workspace_root.join(config.wanted_lockfile_name()))
            .map_err(InstallError::SaveWantedLockfile)?;
    }
    Ok(())
}
