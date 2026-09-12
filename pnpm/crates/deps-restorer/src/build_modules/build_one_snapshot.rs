//! Running one package's build scripts.

mod side_effects;
use side_effects::{
    FrozenStoreWrites, SideEffectsUpload, already_built, side_effects_cache_key,
    upload_side_effects_cache,
};

use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    AllowBuildPolicy, BTreeSet, BuildModulesError, HashMap, LogEvent, LogLevel, Mutex,
    NEEDS_BUILD_MARKER, PackageImportMethod, PackageKey, Path, PathBuf, PkgRoots, RebuildOptions,
    Reporter, RunPostinstallHooks, ScriptsPrependNodePath, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalReason, SnapshotEntry,
    allow_build_key_from_ignored_build, apply_patch_to_dir, bin_dirs_in_all_parent_dirs,
    discard_failed_global_virtual_store_slot, get_pkg_id_with_patch_hash,
    parse_name_version_from_key, run_postinstall_hooks, slot_carries_overlay,
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

    build_candidate::<Reporter>(context, snapshot_key, &candidate, cache_key.as_deref(), optional)
}

/// Skipped hoisted snapshots have no package root, just as skipped isolated snapshots have no directory.
fn build_candidate<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    cache_key: Option<&str>,
    optional: bool,
) -> Result<(), BuildModulesError> {
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
            cache_key,
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

// A removed GVS slot may have been imported pristine while its cached build row survived.
fn global_slot_carries_overlay(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    overlay: &HashMap<String, PathBuf>,
) -> bool {
    context.layout.enable_global_virtual_store()
        && context
            .pkg_roots()
            .canonical(snapshot_key)
            .is_some_and(|pkg_dir| slot_carries_overlay(&pkg_dir, overlay))
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
    let result =
        run_candidate_hooks::<Reporter>(context, snapshot_key, pkg_dir, extra_bin_paths, optional);
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

fn run_candidate_hooks<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    pkg_dir: &Path,
    extra_bin_paths: &[PathBuf],
    optional: bool,
) -> Result<bool, pnpm_executor::LifecycleScriptError> {
    run_postinstall_hooks::<Reporter>(&RunPostinstallHooks {
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
    })
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
