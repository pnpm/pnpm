//! Running one package's build scripts.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    AllowBuildPolicy, BTreeSet, BuildModulesError, HashMap, LogEvent, LogLevel, Mutex,
    NEEDS_BUILD_MARKER, PackageImportMethod, PackageKey, Path, PathBuf, PkgRoots, RebuildOptions,
    Reporter, RunPostinstallHooks, ScriptsPrependNodePath, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalReason, SnapshotEntry,
    allow_build_key_from_ignored_build, apply_patch_to_dir, bin_dirs_in_all_parent_dirs,
    discard_failed_global_virtual_store_slot, get_pkg_id_with_patch_hash, materialize_side_effects,
    parse_name_version_from_key, run_postinstall_hooks, slot_carries_overlay,
    store_index_key_for_resolution,
};

/// Everything one snapshot's build reads: the lockfile shape it belongs to,
/// the policies that gate it, and the environment its scripts run in.
pub(crate) struct BuildOneSnapshot<'a> {
    pub(crate) snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub(crate) packages: Option<&'a HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    pub(crate) patches: Option<&'a HashMap<PackageKey, pnpm_patching::ExtendedPatchInfo>>,
    pub(crate) requires_build_map: &'a HashMap<PackageKey, bool>,
    pub(crate) allow_build_policy: &'a AllowBuildPolicy,
    pub(crate) side_effects_maps_by_snapshot: Option<&'a crate::SideEffectsMapsBySnapshot>,
    pub(crate) engine_name: Option<&'a str>,
    pub(crate) side_effects_cache: bool,
    pub(crate) side_effects_cache_write: bool,
    pub(crate) shared_side_effects_publisher:
        Option<&'a crate::shared_side_effects::SharedSideEffectsPublisher>,
    pub(crate) store_dir: Option<&'a pnpm_store_dir::StoreDir>,
    pub(crate) store_index_writer: Option<&'a std::sync::Arc<pnpm_store_dir::StoreIndexWriter>>,
    pub(crate) dep_graph:
        Option<&'a HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>>,
    pub(crate) deps_state_cache: &'a Mutex<pnpm_graph_hasher::DepsStateCache<PackageKey>>,
    pub(crate) ignored_builds: &'a Mutex<BTreeSet<String>>,
    pub(crate) layout: &'a crate::VirtualStoreLayout,
    pub(crate) pkg_roots_by_key: Option<&'a HashMap<PackageKey, Vec<PathBuf>>>,
    pub(crate) gather_ancestor_bin_paths: bool,
    pub(crate) modules_dir: &'a Path,
    pub(crate) lockfile_dir: &'a Path,
    pub(crate) extra_env: &'a HashMap<String, String>,
    pub(crate) user_agent: &'a str,
    pub(crate) scripts_prepend_node_path: ScriptsPrependNodePath,
    pub(crate) script_shell: Option<&'a Path>,
    pub(crate) shell_emulator: bool,
    pub(crate) unsafe_perm: bool,
    pub(crate) frozen_store: bool,
    pub(crate) ignore_scripts: bool,
    pub(crate) import_method: PackageImportMethod,
    pub(crate) logged_methods: &'a std::sync::atomic::AtomicU8,
    /// Raised before any write that can change a linked slot's contents
    /// (side-effects overlay, patch, lifecycle script) — set pre-attempt, so a
    /// half-applied write still counts. See
    /// [`crate::BuildModulesOutput::mutated_slots`].
    pub(crate) slot_mutations: &'a AtomicBool,
    pub(crate) rebuild: Option<&'a RebuildOptions>,
}

impl<'a> BuildOneSnapshot<'a> {
    fn pkg_roots(&self) -> PkgRoots<'a> {
        PkgRoots { layout: self.layout, by_key: self.pkg_roots_by_key }
    }
}

/// Per-snapshot build work, called once per ready node by the
/// bounded-parallelism scheduler in
/// [`crate::build_modules::BuildModules::run`].
pub(crate) fn build_one_snapshot<Reporter: self::Reporter>(
    snapshot_key: &PackageKey,
    context: &BuildOneSnapshot<'_>,
) -> Result<(), BuildModulesError> {
    // Ancestors of a build/patch candidate are included in the
    // sequence (so the topo order stays correct) but only run
    // scripts / apply patches when they themselves are candidates.
    let Some(candidate) = BuildCandidate::of(context, snapshot_key) else { return Ok(()) };
    let cache_key = side_effects_cache_key(context, snapshot_key, &candidate);
    if already_built::<Reporter>(context, snapshot_key, &candidate, cache_key.as_deref())? {
        return Ok(());
    }

    let optional = context.snapshots.get(snapshot_key).is_some_and(|entry| entry.optional);
    if reject_frozen_store_build::<Reporter>(
        context,
        snapshot_key,
        (&candidate.name, &candidate.version),
        &FrozenStoreWrites {
            optional,
            has_patch: candidate.patch.is_some(),
            should_run_scripts: candidate.should_run_scripts,
        },
    )? {
        return Ok(());
    }

    // Hoisted snapshots without a recorded `pkgRoot` (the walker
    // dropped them — pre-skipped, optional skip, etc.) take the
    // same exit as the isolated path's `!pkg_dir.exists()` skip.
    let Some(pkg_dir) = context.pkg_roots().canonical(snapshot_key) else {
        return Ok(());
    };
    if !pkg_dir.exists() {
        return Ok(());
    }

    // Per-snapshot `extra_bin_paths`. Isolated leaves it empty;
    // hoisted gathers every ancestor's `node_modules/.bin` up to
    // `lockfile_dir` so a lifecycle script invoked at a nested
    // hoisted location can resolve bins added by parents.
    let extra_bin_paths = if context.gather_ancestor_bin_paths {
        bin_dirs_in_all_parent_dirs(&pkg_dir, context.lockfile_dir)
    } else {
        Vec::new()
    };

    // Apply the patch before running postinstall hooks. A snapshot
    // with a patch entry but no resolved `patch_file_path` is a hard
    // error (`PatchFilePathMissing`).
    // `is_patched` feeds the cache-write gate below
    // (`is_patched || has_side_effects`).
    let is_patched = apply_configured_patch(context, snapshot_key, candidate.patch)?;

    let Some(has_side_effects) = run_snapshot_scripts::<Reporter>(
        context,
        snapshot_key,
        &pkg_dir,
        &extra_bin_paths,
        (candidate.should_run_scripts, optional, &candidate.name, &candidate.version),
    )?
    else {
        return Ok(());
    };

    clear_global_virtual_store_build_markers(
        context,
        snapshot_key,
        candidate.patch.is_some() || candidate.should_run_scripts,
    );

    upload_side_effects_cache(
        context,
        snapshot_key,
        &SideEffectsUpload {
            metadata_key: &candidate.metadata_key,
            pkg_dir: &pkg_dir,
            cache_key: cache_key.as_deref(),
            patch: candidate.patch,
            is_patched,
            has_side_effects,
        },
    );

    Ok(())
}

/// A snapshot whose build scripts or patch this install applies, with
/// what the gates decide from.
struct BuildCandidate<'c> {
    metadata_key: PackageKey,
    /// Looked up against the peer-stripped key because patches are
    /// configured at the (name, version) granularity in
    /// `pnpm-workspace.yaml`, not per peer-resolution variant.
    patch: Option<&'c pnpm_patching::ExtendedPatchInfo>,
    name: String,
    version: String,
    /// An explicit `pacquet rebuild` re-runs the build scripts of the
    /// selected packages even when the side-effects cache reports them
    /// already built; this marks those so they bypass the `is_built`
    /// gate. The selection holds allow-build keys (the package name for
    /// registry deps, the full pkgId for git/tarball artifacts), so
    /// either form matches — a selected non-registry artifact is forced
    /// past the gate too. The allow-policy gate still applies — a
    /// rebuild never builds a disallowed package. Non-selected packages
    /// still run the allow-policy gate (so their `.modules.yaml`
    /// ignored-builds record stays intact), but their scripts are
    /// suppressed by the rebuild-selection gate after it.
    force_rebuild: bool,
    should_run_scripts: bool,
}

impl<'c> BuildCandidate<'c> {
    fn of(context: &BuildOneSnapshot<'c>, snapshot_key: &PackageKey) -> Option<Self> {
        let metadata_key = snapshot_key.without_peer();
        let patch = context.patches.and_then(|patches| patches.get(&metadata_key));
        let requires_build = context.requires_build_map.get(snapshot_key).copied().unwrap_or(false);
        if !is_build_candidate(requires_build, patch.is_some()) {
            return None;
        }
        let dep_path = metadata_key.to_string();
        let (name, version) = parse_name_version_from_key(&dep_path);
        let force_rebuild = rebuild_forces_build(context.rebuild, &name, &dep_path);
        let should_run_scripts = snapshot_runs_scripts(
            context,
            snapshot_key,
            &dep_path,
            (requires_build, force_rebuild),
        );
        Some(Self { metadata_key, patch, name, version, force_rebuild, should_run_scripts })
    }
}

/// The side-effects cache key, computed once per snapshot before the
/// `is_built` gate. The same value is later consumed by the WRITE-path
/// upload after `run_postinstall_hooks` succeeds, so recomputing it
/// there would just duplicate work — `deps_state_cache` makes the second
/// call free anyway, but routing through one value keeps the gate-side
/// and write-side keys provably identical.
///
/// `None` when the cache gate can't fire (no engine, no graph, etc.);
/// both downstream consumers short-circuit on `None`.
///
/// The `deps_state_cache` is shared across all scheduled nodes via
/// `Mutex` because `calc_dep_state` is recursive and memoizes — a
/// per-task cache would defeat the memoization for diamond-shaped
/// subgraphs.
fn side_effects_cache_key(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Option<String> {
    let (graph, engine) = context.dep_graph.zip(context.engine_name)?;
    // Poison-recover: `calc_dep_state` mutates the cache by
    // inserting one entry per recursive walk node, each
    // insert atomic from `HashMap`'s POV. A panic mid-walk
    // leaves the map in a usable state — the worst case is
    // an unfinished sub-walk that the next caller will redo.
    let mut cache_guard =
        context.deps_state_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(pnpm_graph_hasher::calc_dep_state(
        graph,
        &mut cache_guard,
        snapshot_key,
        &pnpm_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            // `None` for unpatched snapshots leaves the
            // `;patch=...` segment off the cache key entirely.
            patch_file_hash: candidate.patch.map(|patch| patch.hash.as_str()),
            // The deps-graph hash is included only when scripts
            // will run. A patched-only snapshot leaves it off so
            // the cache key stays stable across dep-graph changes
            // that don't affect this package's patched output.
            include_dep_graph_hash: candidate.should_run_scripts,
        },
    ))
}

/// Side-effects-cache `is_built` gate. Past the policy gate, this
/// snapshot would otherwise run its scripts — but if the prefetch
/// surfaced a matching side-effects-cache entry, the build is already
/// represented on disk (seeded on a previous install) and can be
/// skipped. An explicit `pacquet rebuild` (`force_rebuild`) always
/// re-runs the scripts, so it bypasses this gate.
fn already_built<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    cache_key: Option<&str>,
) -> Result<bool, BuildModulesError> {
    if !candidate.force_rebuild
        && context.side_effects_cache
        && let Some(maps_by_snapshot) = context.side_effects_maps_by_snapshot
        && let Some(maps) = maps_by_snapshot.get(snapshot_key)
        && let Some(key) = cache_key
        && let Some(overlay) = maps.get(key)
    {
        return satisfy_from_side_effects_cache::<Reporter>(
            context,
            snapshot_key,
            (key, overlay),
            (&candidate.name, &candidate.version),
        );
    }
    Ok(false)
}

/// Whether a side-effects-cache hit already put this snapshot's build output
/// on disk, so the build can be skipped.
///
/// The warm link placed only the pristine tarball files in the project-local
/// slot. The cached build's output (the side-effects `added` / `deleted`
/// overlay) still has to land on disk before the build is skipped, or the
/// package is left in its pre-build state — e.g. a postinstall that downloads
/// a binary leaves nothing behind on the warm reinstall. The side-effects diff
/// is applied at import time.
fn satisfy_from_side_effects_cache<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    cached: (&str, &HashMap<String, PathBuf>),
    named: (&str, &str),
) -> Result<bool, BuildModulesError> {
    let (key, overlay) = cached;
    tracing::debug!(
        target: "pacquet::build",
        ?snapshot_key,
        cache_key = key,
        "side-effects cache hit; skipping build",
    );
    // Under the global virtual store the slot usually *is* the seeded build —
    // it persists inside the store across installs — so the overlay is already
    // on disk and re-linking it would be pure overhead. That only holds while
    // the slot survives, though: a failed build discards it
    // ([`discard_failed_global_virtual_store_slot`]), and a prune or a manual
    // removal can too. The store index keeps the side-effects row either way,
    // so the next install re-imports the slot pristine and still hits the
    // cache. Trusting the hit there would skip the build and leave the package
    // unbuilt, so the slot has to be checked rather than assumed.
    let gvs_slot_already_seeded = context.layout.enable_global_virtual_store()
        && context
            .pkg_roots()
            .canonical(snapshot_key)
            .is_some_and(|pkg_dir| slot_carries_overlay(&pkg_dir, overlay));
    if gvs_slot_already_seeded {
        return Ok(true);
    }
    // The overlay carries the patched / built contents, so it has to reach
    // every hoisted copy for the same reason patch application does.
    context.slot_mutations.store(true, Ordering::Relaxed);
    for pkg_dir in context.pkg_roots().all(snapshot_key) {
        // No slot to materialize into (skipped / never linked) — nothing for
        // the build phase to do either.
        if !pkg_dir.exists() {
            continue;
        }
        match materialize_overlay_into_slot::<Reporter>(context, &pkg_dir, overlay) {
            OverlayOutcome::Materialized => {}
            OverlayOutcome::Rebuild(error) => {
                tracing::warn!(
                    target: "pacquet::build",
                    ?snapshot_key,
                    cache_key = key,
                    %error,
                    "failed to materialize side-effects cache overlay; rebuilding",
                );
                return Ok(false);
            }
            OverlayOutcome::Broken(error) => {
                return report_broken_slot::<Reporter>(
                    context,
                    snapshot_key,
                    &pkg_dir,
                    named,
                    error,
                )
                .map(|()| true);
            }
        }
    }
    Ok(true)
}

/// What materializing a cached overlay into one slot left behind.
enum OverlayOutcome {
    Materialized,
    /// Staging failed with the slot's base files intact, so the normal build
    /// path can re-run the script over them and re-seed the cache.
    ///
    /// Side-effects `added` blobs aren't re-verified (see
    /// [`pnpm_store_dir::build_file_maps_from_index`]), so a CAS blob deleted
    /// out from under the store surfaces here.
    Rebuild(BuildModulesError),
    /// A stage-and-swap failed mid-replace and left the slot without its base
    /// files. Rebuilding against that would run scripts on an incomplete dir
    /// (or skip them when the manifest is gone) and let the install finish
    /// with a broken package.
    Broken(BuildModulesError),
}

fn materialize_overlay_into_slot<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    pkg_dir: &Path,
    overlay: &HashMap<String, PathBuf>,
) -> OverlayOutcome {
    match materialize_side_effects::<Reporter>(
        context.logged_methods,
        context.import_method,
        pkg_dir,
        overlay,
    ) {
        Ok(()) => OverlayOutcome::Materialized,
        Err(error) if pkg_dir.join("package.json").exists() => OverlayOutcome::Rebuild(error),
        Err(error) => OverlayOutcome::Broken(error),
    }
}

/// An optional dependency whose slot the overlay left broken is skipped, as
/// for any optional build failure; anything else is a hard error.
fn report_broken_slot<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    pkg_dir: &Path,
    named: (&str, &str),
    error: BuildModulesError,
) -> Result<(), BuildModulesError> {
    let (name, version) = named;
    if !context.snapshots.get(snapshot_key).is_some_and(|entry| entry.optional) {
        return Err(error);
    }
    Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some(error.to_string()),
        package: SkippedOptionalPackage::Installed {
            id: pkg_dir.to_string_lossy().into_owned(),
            name: name.to_string(),
            version: version.to_string(),
        },
        parents: None,
        prefix: context.lockfile_dir.to_string_lossy().into_owned(),
        reason: SkippedOptionalReason::BuildFailure,
    }));
    Ok(())
}

/// Frozen-store backstop. Under the global virtual store the slot directory
/// lives inside the read-only store, so applying a patch or running an
/// approved lifecycle script would fail with a raw `EROFS`; this refuses up
/// front with guidance. The caller is past the `is_built` gate, so a cached
/// build has already returned — reaching here means the seed is genuinely
/// missing this package's build output. Bin-linking (the other write) reuses
/// existing symlinks write-free on a complete seed, so only patch/script
/// writes gate.
///
/// `true` means the snapshot is skipped rather than built.
fn reject_frozen_store_build<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    named: (&str, &str),
    writes: &FrozenStoreWrites,
) -> Result<bool, BuildModulesError> {
    let (name, version) = named;
    let &FrozenStoreWrites { optional, has_patch, should_run_scripts } = writes;
    if !context.frozen_store
        || !context.layout.enable_global_virtual_store()
        || !(has_patch || should_run_scripts)
    {
        return Ok(false);
    }
    if !optional {
        return Err(BuildModulesError::FrozenStoreNeedsBuild {
            package: format!("{name}@{version}"),
        });
    }
    // A build/patch failure on an optional dependency is non-fatal (see the
    // lifecycle-script arm), so a seed missing an optional package's build
    // output skips that build instead of blocking the install.
    Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
        level: LogLevel::Debug,
        details: Some(format!(
            "The read-only store (frozenStore) is missing the build output of {name}@{version}.",
        )),
        package: SkippedOptionalPackage::Installed {
            id: context
                .pkg_roots()
                .canonical(snapshot_key)
                .map_or_else(|| snapshot_key.to_string(), |dir| dir.to_string_lossy().into_owned()),
            name: name.to_string(),
            version: version.to_string(),
        },
        parents: None,
        prefix: context.lockfile_dir.to_string_lossy().into_owned(),
        reason: SkippedOptionalReason::BuildFailure,
    }));
    Ok(true)
}

/// Apply the configured patch before the postinstall hooks run, reporting
/// whether one was applied (which feeds the cache-write gate). A snapshot with
/// a patch entry but no resolved `patch_file_path` is a hard error.
///
/// Every copy is patched, not just the primary slot. Under the hoisted linker
/// a version conflict nests further copies under their consumers; leaving
/// those unpatched would silently run the very code the patch replaces.
fn apply_configured_patch(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    patch: Option<&pnpm_patching::ExtendedPatchInfo>,
) -> Result<bool, BuildModulesError> {
    let Some(patch) = patch else { return Ok(false) };
    let patch_file_path = patch.patch_file_path.as_deref().ok_or_else(|| {
        BuildModulesError::PatchFilePathMissing { dep_path: snapshot_key.to_string() }
    })?;
    context.slot_mutations.store(true, Ordering::Relaxed);
    for patched_dir in context.pkg_roots().all(snapshot_key) {
        if !patched_dir.exists() {
            continue;
        }
        apply_patch_to_dir(&patched_dir, patch_file_path)
            .inspect_err(|_| {
                discard_failed_global_virtual_store_slot(context.layout, snapshot_key);
            })
            .map_err(BuildModulesError::PatchApply)?;
    }
    Ok(true)
}

/// Whether the lifecycle scripts left side effects behind. `None` means an
/// optional dependency's build failed and the snapshot is skipped.
fn run_snapshot_scripts<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    pkg_dir: &Path,
    extra_bin_paths: &[PathBuf],
    run: (bool, bool, &str, &str),
) -> Result<Option<bool>, BuildModulesError> {
    let (should_run_scripts, optional, name, version) = run;
    if !should_run_scripts {
        return Ok(Some(false));
    }
    context.slot_mutations.store(true, Ordering::Relaxed);
    let result = run_postinstall_hooks::<Reporter>(&RunPostinstallHooks {
        dep_path: &snapshot_key.to_string(),
        pkg_root: pkg_dir,
        root_modules_dir: context.modules_dir,
        init_cwd: context.lockfile_dir,
        extra_bin_paths,
        extra_env: context.extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: Some(context.user_agent),
        unsafe_perm: context.unsafe_perm,
        node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
        scripts_prepend_node_path: context.scripts_prepend_node_path,
        script_shell: context.script_shell,
        shell_emulator: context.shell_emulator,
        optional,
    });
    match result {
        Ok(ran) => Ok(Some(ran)),
        Err(err) => {
            // Before the optional-skip return, so a failed optional build
            // leaves no half-built slot behind either.
            discard_failed_global_virtual_store_slot(context.layout, snapshot_key);
            if !optional {
                return Err(BuildModulesError::LifecycleScript(err));
            }
            Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
                level: LogLevel::Debug,
                details: Some(err.to_string()),
                package: SkippedOptionalPackage::Installed {
                    id: pkg_dir.to_string_lossy().into_owned(),
                    name: name.to_string(),
                    version: version.to_string(),
                },
                parents: None,
                prefix: context.lockfile_dir.to_string_lossy().into_owned(),
                reason: SkippedOptionalReason::BuildFailure,
            }));
            Ok(None)
        }
    }
}

/// A slot the isolated global virtual store just built no longer needs its
/// `.pnpm-needs-build` marker.
fn clear_global_virtual_store_build_markers(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    built: bool,
) {
    if !built || !context.layout.enable_global_virtual_store() || context.pkg_roots_by_key.is_some()
    {
        return;
    }
    for built_dir in context.pkg_roots().all(snapshot_key) {
        if let Err(error) = std::fs::remove_file(built_dir.join(NEEDS_BUILD_MARKER))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                target: "pacquet::build",
                ?error,
                dep_path = %snapshot_key,
                "failed to remove the global virtual store build marker",
            );
        }
    }
}

fn is_build_candidate(requires_build: bool, has_patch: bool) -> bool {
    requires_build || has_patch
}

/// An explicit `pacquet rebuild` re-runs the build scripts of the selected
/// packages even when the side-effects cache reports them already built. The
/// selection holds allow-build keys (the package name for registry deps, the
/// full pkgId for git/tarball artifacts), so either form matches — a selected
/// non-registry artifact is forced past the `is_built` gate too.
fn rebuild_forces_build(rebuild: Option<&RebuildOptions>, name: &str, dep_path: &str) -> bool {
    rebuild.is_some_and(|rebuild| {
        rebuild.is_selected(name)
            || rebuild.is_selected(&allow_build_key_from_ignored_build(dep_path))
    })
}

/// Whether this snapshot's lifecycle scripts run at all.
///
/// The allow-policy gate still applies — a rebuild never builds a disallowed
/// package. And a `pacquet rebuild <pkg>` runs scripts only for the selected
/// packages: non-selected ones are still evaluated by the policy gate (so
/// their `.modules.yaml` ignored-builds record stays intact), but their
/// scripts are suppressed here. The side-effects `is_built` gate is only an
/// optimization and is disabled by default, so this gate — not that
/// short-circuit — is what bounds script execution to the selection.
fn snapshot_runs_scripts(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    dep_path: &str,
    build: (bool, bool),
) -> bool {
    let (requires_build, force_rebuild) = build;
    if !requires_build || context.ignore_scripts {
        return false;
    }
    if context.rebuild.is_some() && !force_rebuild {
        return false;
    }
    scripts_are_allowed(context, snapshot_key, dep_path)
}

/// The `allowBuilds` gate, which only applies when the node has scripts to
/// run. A patched-only package skips this check entirely and proceeds to patch
/// application; a refusal returns `false` rather than failing, so the patch
/// still gets applied even when scripts are disallowed.
fn scripts_are_allowed(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    dep_path: &str,
) -> bool {
    if let Some(allowed) = context.allow_build_policy.check(dep_path) {
        allowed
    } else {
        {
            // Poison-recover: see the equivalent call site at the end of
            // `BuildModules::run` for the safety argument (BTreeSet insertion
            // is atomic from the data-structure's POV).
            //
            // The patch hash is kept: two copies of a package that differ only
            // by an applied patch are different builds to approve, and pnpm's
            // `dedupePackageNamesFromIgnoredBuilds` reports them apart for the
            // same reason. `dep_path` has already lost it, so it is re-derived
            // from the full key.
            let ignored_key = get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string();
            context
                .ignored_builds
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(ignored_key);
            false
        }
    }
}

/// The writes a frozen store would have to make into its read-only slot.
struct FrozenStoreWrites {
    optional: bool,
    has_patch: bool,
    should_run_scripts: bool,
}

/// What the side-effects-cache write path uploads.
struct SideEffectsUpload<'a> {
    metadata_key: &'a PackageKey,
    pkg_dir: &'a Path,
    cache_key: Option<&'a str>,
    patch: Option<&'a pnpm_patching::ExtendedPatchInfo>,
    is_patched: bool,
    has_side_effects: bool,
}

/// Side-effects-cache WRITE path. After a successful `run_postinstall_hooks`
/// (or a patch application that mutated the dir), re-hash the package
/// directory and queue a `PackageFilesIndex.sideEffects[cache_key] = diff`
/// mutation so a future install can skip the rebuild.
///
/// A frozen store short-circuits before `upload`: its disabled index writer
/// drops queued rows, but `upload` writes CAFS files before queuing them.
/// Otherwise a patched-only snapshot still uploads its post-patch state so
/// subsequent installs hit the cache.
///
/// The other preconditions: cache key composable (engine + graph present),
/// `packages` map available for store-index key selection, and the resolution
/// has a store-backed key.
///
/// All errors are swallowed with a `tracing::warn!`. A failed upload doesn't
/// fail the install: the next install re-runs the build.
fn upload_side_effects_cache(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    upload: &SideEffectsUpload<'_>,
) {
    if (!upload.is_patched && !upload.has_side_effects) || context.frozen_store {
        return;
    }
    let (Some(writer), Some(store), Some(cache_key), Some(packages)) =
        (context.store_index_writer, context.store_dir, upload.cache_key, context.packages)
    else {
        return;
    };
    let Some(metadata) = packages.get(upload.metadata_key) else { return };
    let publishes_remotely = upload.has_side_effects
        && context
            .shared_side_effects_publisher
            .is_some_and(|publisher| publisher.can_publish(upload.metadata_key, metadata));
    if !context.side_effects_cache_write && !publishes_remotely {
        return;
    }
    let Some(files_index_file) = store_index_key_for_resolution(
        &metadata.resolution,
        &upload.metadata_key.pkg_id(),
        !context.ignore_scripts,
    ) else {
        return;
    };
    let uploaded = upload_and_publish(
        context,
        snapshot_key,
        (store, writer, &files_index_file, cache_key),
        (upload, metadata),
    );
    if let Err(err) = uploaded {
        tracing::warn!(
            target: "pacquet::build",
            ?err,
            dep_path = %snapshot_key,
            "side-effects cache upload failed; build proceeds",
        );
    }
}

fn upload_and_publish(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    store: (
        &pnpm_store_dir::StoreDir,
        &std::sync::Arc<pnpm_store_dir::StoreIndexWriter>,
        &str,
        &str,
    ),
    uploaded: (&SideEffectsUpload<'_>, &pnpm_lockfile::PackageMetadata),
) -> Result<(), pnpm_store_dir::UploadError> {
    let (store, writer, files_index_file, cache_key) = store;
    let (upload, metadata) = uploaded;
    let Some(publisher) = context.shared_side_effects_publisher else {
        return pnpm_store_dir::upload(store, upload.pkg_dir, files_index_file, cache_key, writer);
    };
    let diff = pnpm_store_dir::upload_with_diff(
        store,
        upload.pkg_dir,
        files_index_file,
        cache_key,
        writer,
    )?;
    if upload.has_side_effects
        && let Some(diff) = diff
        && let Some(graph) = context.dep_graph
        && let Err(error) = publisher.publish(
            snapshot_key,
            metadata,
            graph,
            upload.patch.map(|patch| patch.hash.as_str()),
            diff,
            store,
        )
    {
        tracing::warn!(
            target: "pacquet::build",
            dep_path = %snapshot_key,
            %error,
            "remote side-effects publication failed; build proceeds",
        );
    }
    Ok(())
}
