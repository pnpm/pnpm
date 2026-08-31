//! Running package build scripts once the tree is materialized.

use super::{
    AllowBuildPolicy, Arc, AtomicU8, BuildModules, BuildModulesError, Config, DependencyGroup,
    Diagnostic, Display, Error, ExecScriptsPrependNodePath, ExtendedPatchInfo, HashMap,
    IgnoredScriptsLog, LinkBinsError, LinkBinsOptions, Lockfile, LogEvent, LogLevel, OsStr,
    PackageKey, PackageMetadata, PatchKeyConflictError, Path, PathBuf, ProjectSnapshot, Reporter,
    ResolvePatchedDependenciesError, SkippedSnapshots, SnapshotEntry, StoreIndexWriter,
    VirtualStoreLayout, direct_dep_names_for_importer, get_patch_info, importer_root_dir,
    link_top_level_bins,
};

#[cfg(test)]
mod tests;

/// Error type of [`run_build_phase`] and [`resolve_snapshot_patches`].
///
/// Each variant is `#[diagnostic(transparent)]` so the surfaced
/// `ERR_PNPM_*` code comes from the wrapped error — the two install
/// paths embed this in their own error enums (also transparently), so
/// a failed build reports identically regardless of which path ran it.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum BuildPhaseError {
    /// `patchedDependencies` couldn't be resolved from
    /// `pnpm-workspace.yaml`.
    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] ResolvePatchedDependenciesError),

    /// Surfaces `ERR_PNPM_PATCH_KEY_CONFLICT` when more
    /// than one configured version range matches a snapshot. Refuses
    /// to silently pick one — the user must add an exact-version
    /// entry to disambiguate.
    #[diagnostic(transparent)]
    PatchKeyConflict(#[error(source)] PatchKeyConflictError),

    /// A lifecycle script (`preinstall` / `install` / `postinstall`)
    /// failed, or the build phase hit an I/O / frozen-store error.
    #[diagnostic(transparent)]
    BuildModules(#[error(source)] BuildModulesError),

    /// Surfaces a failure from the post-`BuildModules` per-importer
    /// top-level bin link. This pass mixes direct + publicly-hoisted
    /// candidates so `pnpm_cmd_shim::pick_winner` (private)'s
    /// [`pnpm_cmd_shim::BinOrigin::Direct`] tier resolves
    /// conflicts in a single call (pnpm/pacquet#342). The failure
    /// surface is the project-tree top-level
    /// `<importer>/node_modules/.bin`.
    #[diagnostic(transparent)]
    TopLevelBinLink(#[error(source)] LinkBinsError),
}

/// Resolve `pnpm-workspace.yaml`'s `patchedDependencies` into a
/// per-snapshot map keyed by the peer-stripped [`PackageKey`].
///
/// Yields `None` when nothing is configured (no yaml, no key, or empty
/// map) or when there are no snapshots; an empty map when patches exist
/// but match nothing in the current install. Computed from pacquet's
/// lockfile-driven flow: the patch hashes are resolved after the
/// lockfile is built/loaded rather than during resolution.
pub fn resolve_snapshot_patches(
    config: &Config,
    pre_resolved: Option<&pnpm_patching::PatchGroupRecord>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> Result<Option<HashMap<PackageKey, ExtendedPatchInfo>>, BuildPhaseError> {
    // Reuse the caller's grouped record when it already resolved it (the
    // fresh-lockfile path builds it to feed the resolver), so the patch
    // files aren't re-hashed; otherwise resolve it once here (frozen path).
    let resolved_owned = match pre_resolved {
        Some(_) => None,
        None => config
            .resolved_patched_dependencies()
            .map_err(BuildPhaseError::ResolvePatchedDependencies)?,
    };
    let patch_groups = pre_resolved.or(resolved_owned.as_ref());
    let patches = match (patch_groups, snapshots) {
        (Some(groups), Some(snaps)) => {
            let mut map = HashMap::new();
            for key in snaps.keys() {
                let metadata_key = key.without_peer();
                let (name, version) = crate::name_version_from_package_key(&metadata_key, packages);
                // Propagate `ERR_PNPM_PATCH_KEY_CONFLICT` rather than
                // silently skipping the snapshot. Failing here makes the
                // user add an exact-version entry to disambiguate.
                if let Some(info) = get_patch_info(Some(groups), &name, &version)
                    .map_err(BuildPhaseError::PatchKeyConflict)?
                {
                    map.insert(metadata_key, info.clone());
                }
            }
            Some(map)
        }
        _ => None,
    };
    Ok(patches)
}

/// Inputs to [`run_build_phase`]. Bundled so both install paths
/// ([`crate::install_frozen_lockfile::InstallFrozenLockfile::run`] and the fresh-lockfile path) can
/// drive the shared lifecycle-script + post-build top-level bin link
/// without a long positional argument list.
pub struct BuildPhaseInputs<'a> {
    pub config: &'static Config,
    /// `lockfileDir` — the project root. Threaded to
    /// `BuildModules` as `lockfile_dir`, where it sets each script's
    /// `INIT_CWD` and the lifecycle log prefix.
    pub workspace_root: &'a Path,
    /// Directory each importer's `node_modules/.bin` is anchored under
    /// in the post-build top-level bin pass. Equals `workspace_root`
    /// in production (and on the frozen path); the fresh path passes
    /// its `symlink_root` (`config.modules_dir.parent()`), which can
    /// differ when a test relocates `modules_dir`.
    pub top_level_bin_root: &'a Path,
    pub layout: &'a VirtualStoreLayout,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub dependency_groups: &'a [DependencyGroup],
    /// `patchedDependencies` already resolved + grouped by the caller, so
    /// the build phase doesn't re-hash the patch files. `None` on the
    /// frozen path, which resolves it inside [`resolve_snapshot_patches`].
    pub patch_groups: Option<&'a pnpm_patching::PatchGroupRecord>,
    pub allow_build_policy: &'a AllowBuildPolicy,
    pub side_effects_maps_by_snapshot: &'a crate::SideEffectsMapsBySnapshot,
    pub requires_build_by_snapshot: &'a crate::RequiresBuildBySnapshot,
    /// Snapshot keys materialized by this install. Under
    /// `ignoreScripts`, only these can add new `pendingBuilds`; the
    /// install orchestrator separately carries forward existing entries.
    pub materialized_snapshots: &'a [PackageKey],
    pub engine_name: Option<&'a str>,
    pub extra_env: &'a HashMap<String, String>,
    pub store_index_writer: &'a Arc<StoreIndexWriter>,
    pub skipped: &'a SkippedSnapshots,
    pub hoisted_pkg_roots_by_key: Option<&'a HashMap<PackageKey, Vec<PathBuf>>>,
    pub is_hoisted: bool,
    /// Publicly-hoisted aliases (with bins) competing for the root
    /// importer's `node_modules/.bin`. Empty under the hoisted linker
    /// and when no public-hoist pattern is set.
    pub publicly_hoisted_for_post_build: &'a [String],
    pub logged_methods: &'a AtomicU8,
    /// Forced-rebuild selection threaded from `pacquet rebuild` /
    /// `approve-builds`; `None` for a normal install. See
    /// [`crate::RebuildOptions`].
    pub rebuild: Option<&'a crate::RebuildOptions>,
    /// [`crate::shim_link_options`] output, for the post-build
    /// top-level bin pass.
    pub link_options: &'a LinkBinsOptions,
}

/// Run dependency lifecycle scripts, report ignored builds, and
/// re-link top-level bins — the shared tail both install paths run
/// after the virtual store is materialized.
///
/// Runs a single `buildModules` + `pnpm:ignored-scripts` emit +
/// `linkBinsOfImporter` sequence. Always emits the `IgnoredScripts`
/// event (with an empty list when nothing was ignored) so the reporter
/// renders a consistent state.
pub fn run_build_phase<Reporter: self::Reporter>(
    inputs: &BuildPhaseInputs,
) -> Result<crate::BuildModulesOutput, BuildPhaseError> {
    // Every field is a `Copy` reference / scalar, so destructuring
    // through the shared borrow copies them out without a move.
    let &BuildPhaseInputs {
        config,
        workspace_root,
        top_level_bin_root,
        layout,
        snapshots,
        packages,
        importers,
        dependency_groups,
        patch_groups,
        allow_build_policy,
        side_effects_maps_by_snapshot,
        requires_build_by_snapshot,
        materialized_snapshots,
        engine_name,
        extra_env,
        store_index_writer,
        skipped,
        hoisted_pkg_roots_by_key,
        is_hoisted,
        publicly_hoisted_for_post_build,
        logged_methods,
        rebuild,
        link_options,
    } = inputs;

    let patches = resolve_snapshot_patches(config, patch_groups, snapshots, packages)?;
    let shared_side_effects_publisher =
        crate::shared_side_effects::shared_side_effects_publisher(config, snapshots);

    // Convert `pnpm-config`'s mirror enum to the executor's
    // canonical type. Config's enum carries the yaml-deserialize impl;
    // the executor's stays free of serde wiring.
    let scripts_prepend_node_path = match config.scripts_prepend_node_path {
        pnpm_config::ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
        pnpm_config::ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
        pnpm_config::ScriptsPrependNodePath::WarnOnly => ExecScriptsPrependNodePath::WarnOnly,
    };

    // BuildModules walks per-snapshot package directories and runs
    // `preinstall` / `install` / `postinstall` lifecycle scripts.
    // Under isolated, the directories live under the virtual-store slot
    // layout; under hoisted, they live at the project-tree paths the
    // walker assigned — threaded in via `pkg_roots_by_key`.
    let can_defer_without_build_modules = config.ignore_scripts
        && rebuild.is_none()
        && patches.as_ref().is_none_or(HashMap::is_empty)
        && (!config.side_effects_cache_read() || side_effects_maps_by_snapshot.is_empty());
    let build_output = if can_defer_without_build_modules {
        let newly_deferred = materialized_snapshots
            .iter()
            .filter(|snapshot_key| !skipped.contains(snapshot_key))
            .filter_map(|snapshot_key| requires_build_by_snapshot.get_key_value(snapshot_key));
        crate::BuildModulesOutput {
            ignored_builds: Vec::new(),
            deferred_builds: crate::build_modules::deferred_builds(newly_deferred, true),
            mutated_slots: false,
        }
    } else {
        BuildModules {
            layout,
            modules_dir: &config.modules_dir,
            lockfile_dir: workspace_root,
            snapshots,
            packages,
            importers,
            allow_build_policy,
            side_effects_maps_by_snapshot: Some(side_effects_maps_by_snapshot),
            requires_build_by_snapshot: Some(requires_build_by_snapshot),
            engine_name,
            side_effects_cache: config.side_effects_cache_read()
                || config.remote_side_effects_cache.is_some(),
            side_effects_cache_write: config.side_effects_cache_write(),
            shared_side_effects_publisher: shared_side_effects_publisher.as_ref(),
            store_dir: Some(&config.store_dir),
            store_index_writer: Some(store_index_writer),
            patches: patches.as_ref(),
            scripts_prepend_node_path,
            script_shell: config.script_shell.as_deref().map(Path::new),
            shell_emulator: config.shell_emulator,
            extra_env,
            user_agent: &config.user_agent,
            unsafe_perm: config.unsafe_perm,
            child_concurrency: config.child_concurrency,
            skipped,
            pkg_roots_by_key: hoisted_pkg_roots_by_key,
            gather_ancestor_bin_paths: is_hoisted,
            frozen_store: config.frozen_store,
            ignore_scripts: config.ignore_scripts,
            import_method: config.package_import_method,
            logged_methods,
            rebuild,
        }
        .run::<Reporter>()
        .map_err(BuildPhaseError::BuildModules)?
    };

    // Always emit the `pnpm:ignored-scripts` event with the package
    // names, unconditionally, so structured / NDJSON consumers always
    // see the list. The event
    // carries `strict_dep_builds` (the final, post-`updateConfig` value
    // the strict-failure check also reads) so the default reporter can
    // suppress the rendered warning box under strict mode — where the
    // install fails with `ERR_PNPM_IGNORED_BUILDS` and the box would only
    // duplicate the error — without a stale reporter-side flag. The
    // display is gated on `!strictDepBuilds`; the strict path throws.
    Reporter::emit(&LogEvent::IgnoredScripts(IgnoredScriptsLog {
        level: LogLevel::Debug,
        package_names: build_output.ignored_builds.clone(),
        strict_dep_builds: config.strict_dep_builds,
    }));

    // `virtual_store_only` links no importer bins, so there is nothing
    // for the pass below to re-resolve. Dependency *build* scripts still
    // ran above — only the importer-facing linking stops, matching
    // `pnpm fetch`.
    if config.virtual_store_only {
        return Ok(build_output);
    }

    // Post-`BuildModules` per-importer top-level bin link
    // (pnpm/pacquet#342). Resolves direct-over-hoisted precedence and
    // shims lifecycle-script-created bins that didn't exist at extract
    // time. Idempotent for unchanged shims. Runs after `buildModules`.
    let modules_dir_basename: &OsStr =
        config.modules_dir.file_name().unwrap_or_else(|| OsStr::new("node_modules"));
    for (importer_id, importer_snapshot) in importers {
        // Public-hoist promotes transitives into the workspace root's
        // `<root>/node_modules/<alias>`, so only the root importer's
        // `.bin` sees `BinOrigin::Hoisted` candidates.
        let hoisted_names: &[String] = if importer_id == Lockfile::ROOT_IMPORTER_KEY {
            publicly_hoisted_for_post_build
        } else {
            &[]
        };
        // When nothing this phase can change actually changed — no
        // script, patch, or side-effects overlay touched a linked slot
        // — the isolated link phase's own bin pass already shimmed
        // exactly this candidate set, so re-resolving it would only
        // re-read every direct dep's manifest per importer. Hoisted
        // installs always relink: this pass is their only importer
        // bin pass.
        if !is_hoisted && !build_output.mutated_slots && hoisted_names.is_empty() {
            continue;
        }
        let project_dir = importer_root_dir(top_level_bin_root, importer_id);
        let modules_dir = project_dir.join(modules_dir_basename);
        // Same filter the symlink phase used so the post-build pass sees
        // the same candidate set (skipping installability-skipped deps
        // avoids dangling shims at a slot that was never extracted).
        let direct_names = direct_dep_names_for_importer(
            importer_snapshot,
            dependency_groups.iter().copied(),
            skipped,
            false,
        );
        link_top_level_bins(&modules_dir, &direct_names, hoisted_names, link_options)
            .map_err(BuildPhaseError::TopLevelBinLink)?;
    }

    Ok(build_output)
}
