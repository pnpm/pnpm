use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, NEEDS_BUILD_MARKER, SkippedSnapshots,
    build_sequence::build_sequence,
    import_indexed_dir, store_index_key_for_resolution,
    version_policy::{VersionPolicyError, expand_package_version_specs},
};
pub(crate) mod allow_build_policy;
pub(crate) mod build_one_snapshot;
pub(crate) mod slots;

pub use allow_build_policy::{
    AllowBuildPolicy, allow_build_key_from_ignored_build, normalize_build_dep_path,
};
pub(crate) use build_one_snapshot::build_one_snapshot;
pub use slots::parse_name_version_from_key;
pub(crate) use slots::{
    bin_dirs_in_all_parent_dirs, discard_failed_global_virtual_store_slot,
    materialize_side_effects, pkg_root_for_key, pkg_roots_for_key, slot_carries_overlay,
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{Config, PackageImportMethod};
use pnpm_deps_path::{get_pkg_id_with_patch_hash, index_of_dep_path_suffix, remove_suffix};
use pnpm_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_postinstall_hooks,
};
use pnpm_lockfile::{PackageKey, ProjectSnapshot, SnapshotEntry};
use pnpm_package_manifest::pkg_requires_build;
use pnpm_patching::{PatchApplyError, apply_patch_to_dir};
use pnpm_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};
use rayon::prelude::*;
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Error from the build-modules step.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum BuildModulesError {
    #[diagnostic(transparent)]
    LifecycleScript(#[error(source)] LifecycleScriptError),

    #[diagnostic(transparent)]
    PatchApply(#[error(source)] PatchApplyError),

    /// `ERR_PNPM_PATCH_FILE_PATH_MISSING` — fired when a snapshot's
    /// resolved patch carries a hash but
    /// no `patch_file_path`. The hash-without-path shape can come
    /// from the lockfile when no live config provides the path, so
    /// the user must add an entry to `patchedDependencies` in
    /// `pnpm-workspace.yaml` to bring the file back into scope.
    #[display("Cannot apply patch for {dep_path}: patch file path is missing")]
    #[diagnostic(
        code(ERR_PNPM_PATCH_FILE_PATH_MISSING),
        help("Ensure the package is listed in patchedDependencies configuration")
    )]
    PatchFilePathMissing { dep_path: String },

    /// `ThreadPoolBuilder::build()` failed — most likely the OS
    /// refused to spawn the requested number of worker threads
    /// (`EAGAIN` / `RLIMIT_NPROC`). Surfaced as a structured error
    /// rather than a panic so the install path can return cleanly.
    #[display("Failed to build the per-install rayon thread pool: {source}")]
    #[diagnostic(
        code(ERR_PNPM_BUILD_THREAD_POOL),
        help(
            "Lower childConcurrency in pnpm-workspace.yaml, or raise the process's RLIMIT_NPROC."
        )
    )]
    ThreadPoolBuild {
        #[error(source)]
        source: rayon::ThreadPoolBuildError,
    },

    /// Under the global virtual store a package's directory lives
    /// inside the store, so applying a patch or running an approved
    /// lifecycle script writes into the store. `frozen_store` promises
    /// the store is complete and read-only, so the build cannot run.
    /// A complete seed never reaches here — patched and built packages
    /// are imported from the side-effects cache and skipped by the
    /// `is_built` gate — so this means the seed is missing build
    /// output, surfaced as `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD`.
    #[display("Cannot build {package} because the store is read-only (frozenStore is enabled)")]
    #[diagnostic(
        code(ERR_PNPM_FROZEN_STORE_NEEDS_BUILD),
        help(
            "This read-only store was not seeded with this package's build output. Rebuild the seed with its scripts enabled so the side-effects cache is populated, or remove it from onlyBuiltDependencies."
        )
    )]
    FrozenStoreNeedsBuild { package: String },

    /// Re-materializing a cached build's side-effects overlay into the
    /// already-linked slot failed. Fired from the `is_built` gate in
    /// `build_one_snapshot` when the warm reinstall has to apply the
    /// stored `added` / `deleted` diff on top of the pristine files.
    #[diagnostic(transparent)]
    MaterializeSideEffects(#[error(source)] ImportIndexedDirError),
}

/// Drives a forced rebuild of already-installed packages. Constructed by
/// `pacquet rebuild` and `pacquet approve-builds`; absent (`None`) for a
/// normal install.
///
/// Effect on [`BuildModules`]: a selected package is built even when the
/// side-effects cache reports it already built (an explicit rebuild always
/// re-runs the scripts). The allow-policy gate is unchanged — a rebuild
/// never builds a disallowed package — and non-selected packages keep
/// their normal install gating so a partial rebuild does not drop the
/// ignored-builds record for the packages it did not touch.
#[derive(Debug, Default, Clone)]
pub struct RebuildOptions {
    /// Allow-build keys (the package name for registry deps, the full
    /// pkgId for git/tarball artifacts — see
    /// [`allow_build_key_from_ignored_build`]) to force past the
    /// side-effects `is_built` gate. `None` forces every build-needing
    /// package (`pnpm rebuild` with no arguments); `Some(keys)` forces
    /// only the matching ones (`pnpm rebuild <pkg>...`). A package matches
    /// when either its name or its allow-build key is in the set, so a
    /// `pnpm rebuild <name>` and an `approve-builds` key both select it.
    pub selected_names: Option<HashSet<String>>,

    /// Importer ids whose own deferred install scripts this rebuild
    /// should run — `pnpm rebuild --pending` reads them out of
    /// `.modules.yaml`'s `pendingBuilds`. A dependency's build is settled
    /// by the rebuild itself; a project's is only settled by running its
    /// scripts, which nothing else in the rebuild path does.
    pub pending_projects: Vec<String>,
}

impl RebuildOptions {
    /// Whether a package named `name` is in the rebuild selection. An
    /// absent selection (`None`) matches every package.
    fn is_selected(&self, name: &str) -> bool {
        self.selected_names.as_ref().is_none_or(|names| names.contains(name))
    }

    /// Whether this rebuild discharges the workspace project recorded
    /// under `importer_id`, which only running its own scripts can do —
    /// dropping one the rebuild never ran would forget the debt rather
    /// than settle it.
    #[must_use]
    pub fn settles_project(&self, importer_id: &str) -> bool {
        self.pending_projects.iter().any(|id| id == importer_id)
    }

    /// Whether this rebuild discharges the dependency recorded under
    /// `dep_path`, which it does by rebuilding it.
    ///
    /// The caller decides which of the two a `.modules.yaml`
    /// `pendingBuilds` entry is — an importer id and a dep path are both
    /// plain strings on disk, and a workspace directory named
    /// `foo@1.0.0` parses as either.
    #[must_use]
    pub fn settles_dependency(&self, dep_path: &str) -> bool {
        let (name, _) = parse_name_version_from_key(remove_suffix(dep_path));
        self.is_selected(&name) || self.is_selected(&allow_build_key_from_ignored_build(dep_path))
    }
}

/// Run lifecycle scripts for all packages that require a build.
///
/// Packages are visited in topological order (children before parents) via
/// [`build_sequence`]. Chunks run sequentially. Members within a chunk
/// run in parallel under a per-install rayon thread pool bounded to
/// [`BuildModules::child_concurrency`] threads.
pub struct BuildModules<'a> {
    /// Install-scoped slot-directory mapping (GVS-aware). The layout
    /// knows the per-snapshot subdirectory shape (legacy flat-name vs
    /// GVS `<scope>/<name>/<version>/<hash>`). See
    /// [`crate::VirtualStoreLayout`].
    pub layout: &'a crate::VirtualStoreLayout,
    pub modules_dir: &'a Path,
    pub lockfile_dir: &'a Path,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub allow_build_policy: &'a AllowBuildPolicy,
    /// Per-snapshot side-effects-cache overlays — passed in from
    /// `CreateVirtualStore`'s prefetch. `None` means the cache is
    /// disabled or no rows were prefetched; the gate falls through
    /// to "rebuild" for every snapshot.
    pub side_effects_maps_by_snapshot: Option<&'a crate::SideEffectsMapsBySnapshot>,
    /// Per-snapshot `requiresBuild` values from the warm-cache
    /// prefetch. Missing entries fall back to inspecting the
    /// materialized package directory.
    pub requires_build_by_snapshot: Option<&'a crate::RequiresBuildBySnapshot>,
    /// `<platform>;<arch>;node<major>` — the prefix part of the
    /// dep-state cache key. Computed once at install
    /// start by [`pnpm_graph_hasher::detect_node_major`] +
    /// [`pnpm_graph_hasher::engine_name`]. When `None`, the
    /// gate falls through to "rebuild" (no key to look up).
    pub engine_name: Option<&'a str>,
    /// Mirrors `config.side_effects_cache`. When `false`, the
    /// gate is bypassed entirely and every `requires_build`
    /// snapshot runs its scripts.
    pub side_effects_cache: bool,
    /// Mirrors `config.side_effects_cache_write`. When `true`, a
    /// successful postinstall triggers a re-CAFS of the built package
    /// directory and a queued mutation of the matching
    /// `PackageFilesIndex.sideEffects` row.
    pub side_effects_cache_write: bool,
    /// Store-dir handle for the WRITE path's `add_files_from_dir`
    /// call. `None` short-circuits the upload site entirely — used
    /// by unit tests that don't set up a CAFS.
    pub store_dir: Option<&'a pnpm_store_dir::StoreDir>,
    /// Shared batched writer for the side-effects upload's
    /// read-modify-write of the existing `PackageFilesIndex` row.
    /// `None` short-circuits the upload site.
    pub store_index_writer: Option<&'a std::sync::Arc<pnpm_store_dir::StoreIndexWriter>>,
    /// Per-snapshot resolved patch metadata. Keyed by the snapshot's
    /// peer-stripped `PackageKey`, value is the matching
    /// `ExtendedPatchInfo` (hash + absolute path) computed by
    /// [`pnpm_patching::resolve_and_group`] + per-snapshot
    /// [`pnpm_patching::get_patch_info`]. `None` when no
    /// `patchedDependencies` is configured.
    ///
    /// Drives three things:
    ///
    /// 1. Build trigger — a snapshot with a patch entry becomes a
    ///    build candidate even when `requires_build` is false.
    /// 2. Side-effects-cache key — `patch_file_hash` carries the
    ///    SHA-256 hex into [`pnpm_graph_hasher::CalcDepStateOptions`].
    /// 3. Patch application — the patch is applied to the extracted
    ///    package dir before postinstall hooks run.
    pub patches: Option<&'a HashMap<PackageKey, pnpm_patching::ExtendedPatchInfo>>,
    /// Mirrors `config.scripts_prepend_node_path`. Threaded through to
    /// [`RunPostinstallHooks::scripts_prepend_node_path`] for each
    /// spawned lifecycle script. Default [`ScriptsPrependNodePath::Never`].
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    /// Mirrors `config.script_shell`. Threaded through to
    /// [`RunPostinstallHooks::script_shell`], so a workspace that
    /// configures a shell gets it for build scripts too, not only for
    /// `pnpm run`. `None` selects the platform default.
    pub script_shell: Option<&'a Path>,
    /// Mirrors `config.shell_emulator`. Threaded through to
    /// [`RunPostinstallHooks::shell_emulator`], so build scripts run
    /// under the built-in shell wherever `pnpm run` would.
    pub shell_emulator: bool,
    pub extra_env: &'a HashMap<String, String>,
    /// Mirrors `config.user_agent`, stamped into each build script's
    /// `npm_config_user_agent`.
    pub user_agent: &'a str,
    /// Mirrors `config.unsafe_perm`. When `false`, [`pnpm_executor`]
    /// runs each lifecycle script under a per-package TMPDIR set to
    /// `node_modules/.tmp`; when `true`, TMPDIR is left at the
    /// inherited value. Default `true`.
    pub unsafe_perm: bool,
    /// Mirrors `config.child_concurrency`. Per-chunk parallelism
    /// for build-script spawns. Chunks remain sequential to preserve
    /// topological ordering; members within a chunk run in parallel
    /// up to this many at a time. Floored to `1` to guarantee forward
    /// progress on resource-constrained hosts.
    pub child_concurrency: u32,
    /// Snapshots the installability pass marked optional+incompatible.
    /// Excluded from both `requires_build` computation and the
    /// `build_sequence` input — pacquet does not run scripts (or
    /// even check `binding.gyp`) for slots that don't exist on
    /// disk. Skipped snapshots never enter the build graph.
    pub skipped: &'a SkippedSnapshots,

    /// Per-snapshot `pkgRoot` override, populated by the hoisted
    /// linker with the slice 4 walker's
    /// [`crate::DependenciesGraphNode::dir`] values. When `Some`,
    /// every `pkgRoot` lookup goes through this map instead of the
    /// virtual-store-layout slot computation; a missing entry means
    /// the snapshot didn't make it into the hoisted graph (skipped
    /// optional, etc.) and the build phase silently passes over it.
    /// `None` for the isolated linker — its slot directories are
    /// recovered from [`crate::VirtualStoreLayout::slot_dir`]. The
    /// two-mode `pkgRoot` selection (override map vs. layout slot)
    /// is handled by `pkg_root_for_key` and `pkg_roots_for_key`.
    ///
    /// One snapshot can occupy several directories: the walker nests a
    /// second copy of a package under a sibling when a version conflict
    /// keeps it out of the root. The first entry is the canonical
    /// `pkgRoot` — scripts run there once and the side-effects cache is
    /// written from it, because the contents are identical everywhere.
    /// Writes that must land in *every* copy (patch application,
    /// re-importing a cached overlay) iterate the whole list.
    pub pkg_roots_by_key: Option<&'a HashMap<PackageKey, Vec<PathBuf>>>,

    /// When `true`, compute per-snapshot `extra_bin_paths` via
    /// `bin_dirs_in_all_parent_dirs` (private helper in this module)
    /// so lifecycle scripts can resolve binaries from every ancestor `node_modules/.bin`
    /// up to [`Self::lockfile_dir`]. Set under the hoisted linker.
    /// Always `false` under the isolated linker — its bins live in
    /// the slot's own `<slot>/node_modules/.bin`, populated up-
    /// front by [`crate::LinkVirtualStoreBins`], and the script
    /// executor adds that path itself.
    pub gather_ancestor_bin_paths: bool,

    /// Mirrors `config.frozen_store`. When `true` together with the
    /// global virtual store, a snapshot that would apply a patch or
    /// run an approved lifecycle script is refused with
    /// [`BuildModulesError::FrozenStoreNeedsBuild`] before the write
    /// is attempted — the store is read-only, so the build cannot run.
    /// Has no effect under the isolated linker, whose slot directories
    /// live in the writable project store.
    pub frozen_store: bool,

    /// Mirrors `config.ignore_scripts`. When `true`, no lifecycle
    /// script runs and the allow-build gate is bypassed entirely, so a
    /// package not in `allowBuilds` is *not* added to the returned
    /// ignored-builds set. Patches still apply — a patch is applied
    /// even when scripts are suppressed.
    pub ignore_scripts: bool,

    /// Mirrors `config.package_import_method`. Used by the
    /// side-effects-cache `is_built` gate to re-materialize a cached
    /// build's output into the already-linked slot — the warm link
    /// only placed the pristine tarball files, so the cached
    /// `added` / `deleted` overlay has to be applied on top before the
    /// build is skipped. See `build_one_snapshot`.
    pub import_method: PackageImportMethod,

    /// Install-scoped dedupe state for the `pnpm:package-import-method`
    /// log, shared with [`crate::CreateVirtualStore`] so the side-effects
    /// re-materialization doesn't re-announce a method the link phase
    /// already reported.
    pub logged_methods: &'a std::sync::atomic::AtomicU8,

    /// Forced-rebuild selection. `None` for a normal install — every
    /// package follows the standard `requires_build` + allow-policy +
    /// side-effects-cache gates. `Some` (a `pacquet rebuild` /
    /// `approve-builds`) restricts the build to the selected names and
    /// forces them past the side-effects `is_built` gate. See
    /// [`RebuildOptions`].
    pub rebuild: Option<&'a RebuildOptions>,
}

/// What a [`BuildModules`] run decided about the packages it visited
/// but did not build.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BuildModulesOutput {
    /// Sorted, peer-stripped `name@version` keys whose scripts were
    /// skipped because the package was not in `allowBuilds`. The caller
    /// folds these into a single `pnpm:ignored-scripts` event.
    pub ignored_builds: Vec<String>,

    /// Sorted dep paths of the snapshots that need a build which
    /// `--ignore-scripts` deferred. Empty when scripts were not
    /// ignored. These become `.modules.yaml`'s `pendingBuilds`, which
    /// `pnpm rebuild --pending` later drains.
    ///
    /// Peers are kept here — unlike `ignored_builds`, whose keys are an
    /// `allowBuilds` lookup, these address a materialized slot.
    pub deferred_builds: Vec<String>,
}

impl BuildModules<'_> {
    /// Run the build, reporting the packages that needed one but did
    /// not get it — see [`BuildModulesOutput`].
    pub fn run<Reporter: self::Reporter>(self) -> Result<BuildModulesOutput, BuildModulesError> {
        let BuildModules {
            layout,
            modules_dir,
            lockfile_dir,
            snapshots,
            packages,
            importers,
            allow_build_policy,
            side_effects_maps_by_snapshot,
            requires_build_by_snapshot,
            engine_name,
            side_effects_cache,
            side_effects_cache_write,
            store_dir,
            store_index_writer,
            patches,
            scripts_prepend_node_path,
            script_shell,
            shell_emulator,
            extra_env,
            user_agent,
            unsafe_perm,
            child_concurrency,
            skipped,
            pkg_roots_by_key,
            gather_ancestor_bin_paths,
            frozen_store,
            ignore_scripts,
            import_method,
            logged_methods,
            rebuild,
        } = self;

        let Some(snapshots) = snapshots else { return Ok(BuildModulesOutput::default()) };

        // Compute `requiresBuild` per snapshot. Warm store-index rows
        // already carry a precomputed answer, so only misses need to
        // inspect the materialized package directory.
        let requires_build_map: HashMap<PackageKey, bool> = snapshots
            .keys()
            // Skip snapshots that never landed on disk. `pkg_requires_build`
            // would just return `false` for a missing dir, but the
            // walk would still spend a syscall per skipped key — the
            // filter short-circuits that on installs with large
            // optional fan-out.
            .filter(|key| !skipped.contains(key))
            .map(|key| {
                let pkg_root = pkg_root_for_key(layout, pkg_roots_by_key, key);
                let requires = match (
                    pkg_root.as_deref(),
                    requires_build_by_snapshot.and_then(|map| map.get(key).copied()),
                ) {
                    (None, _) => false,
                    (_, Some(requires)) => requires,
                    (Some(pkg_root), None) => pkg_requires_build(pkg_root),
                };
                (key.clone(), requires)
            })
            .collect();

        // Build the dep graph + state cache only when the
        // side-effects-cache gate has a chance of firing — on
        // either the READ side (prefetch surfaced cache rows) or
        // the WRITE side (the install will be populating new
        // cache entries after a successful build).
        //
        // The graph is bounded to the *forward closure of
        // `requires_build` snapshots* via `build_deps_subgraph`.
        // The upload-site and gate-check loops only ever compute
        // cache keys for `requires_build` snapshots (the
        // `continue` at the top of the chunk loop), and
        // `calc_dep_state` only recurses into a snapshot's own
        // children, so the closure-bounded graph produces the
        // exact same cache keys as the full graph for every
        // root we'll query. A pure-JS install with no
        // `requires_build` snapshots feeds in an empty root
        // iterator and the function returns immediately —
        // O(0) walk for that path.
        //
        // The per-install dep-state cache memoizes per-node hash
        // across diamond-shaped subgraphs so the recursive walk stays
        // linear in |closure| even when the same dep is reachable
        // through many parents.
        let read_gate_active = side_effects_cache
            && engine_name.is_some()
            && side_effects_maps_by_snapshot.is_some_and(|map| !map.is_empty());
        let write_gate_active = side_effects_cache_write
            && !frozen_store
            && engine_name.is_some()
            && store_index_writer.is_some()
            && store_dir.is_some();
        let cache_gate_active = (read_gate_active || write_gate_active) && packages.is_some();
        let dep_graph = cache_gate_active.then(|| {
            let roots = requires_build_map
                .iter()
                .filter(|&(_, &requires_build)| requires_build)
                .map(|(key, _)| key.clone());
            crate::build_deps_subgraph(
                snapshots,
                packages.expect("`cache_gate_active` requires packages: Some"),
                roots,
            )
        });
        // `deps_state_cache` memoizes per-snapshot hashes across the
        // recursive walk in `calc_dep_state`. Shared across all
        // chunks so diamond-shaped subgraphs hit the memo from
        // earlier chunks too. Wrapped in `Mutex` because chunks now
        // dispatch their members concurrently — `calc_dep_state`
        // mutates the cache through `&mut`, and rayon would
        // otherwise need each task to own a private cache, defeating
        // the point of memoization.
        let deps_state_cache: Mutex<pnpm_graph_hasher::DepsStateCache<PackageKey>> =
            Mutex::new(pnpm_graph_hasher::DepsStateCache::new());
        // Prime it in lockfile key order before any chunk runs. The
        // chunk members race for the mutex, and a snapshot inside a
        // dependency cycle takes the digest of whichever walk reached
        // it first — so an unprimed cache would hand the same install
        // a different side-effects-cache key on every run, and every
        // repeat install would re-run the build it already has cached.
        if let Some(graph) = &dep_graph {
            let mut cache_guard =
                deps_state_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            pnpm_graph_hasher::warm_deps_state_cache(
                graph,
                &mut cache_guard,
                crate::deps_graph::in_lockfile_order(graph).into_iter().map(|(key, _)| key),
            );
        }

        let chunks = build_sequence(&requires_build_map, patches, snapshots, importers, skipped);

        // Collect peer-stripped keys so the final list is unique and
        // sorted lexicographically — matches `dedupePackageNamesFromIgnoredBuilds`.
        // `Mutex` for the same parallelism reason as `deps_state_cache` above.
        let ignored_builds: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

        // Per-install rayon pool. Bounded by `child_concurrency`, the
        // largest chunk, and the package-manager safety cap so a
        // repository cannot request an excessive number of native
        // threads. One pool is reused across all sequential chunks.
        //
        // `ThreadPoolBuilder::build()` is fallible — the OS may
        // refuse the spawn (`EAGAIN` / RLIMIT_NPROC) on a host
        // already near its process-thread limit. Surface that as
        // [`BuildModulesError::ThreadPoolBuild`] so the install
        // returns cleanly with a remediation hint instead of
        // panicking inside the binary.
        let max_chunk_size = chunks.iter().map(Vec::len).max().unwrap_or(0);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(crate::script_thread_count(child_concurrency, max_chunk_size))
            .build()
            .map_err(|source| BuildModulesError::ThreadPoolBuild { source })?;

        for chunk in chunks {
            // The closure runs once per chunk; `try_for_each`
            // short-circuits on the first error. The only mutable
            // state shared across tasks is the two `Mutex`-wrapped
            // collections above and `deps_state_cache`.
            pool.install(|| -> Result<(), BuildModulesError> {
                chunk.par_iter().try_for_each(|snapshot_key| {
                    build_one_snapshot::<Reporter>(
                        snapshot_key,
                        snapshots,
                        packages,
                        patches,
                        &requires_build_map,
                        allow_build_policy,
                        side_effects_maps_by_snapshot,
                        engine_name,
                        side_effects_cache,
                        side_effects_cache_write,
                        store_dir,
                        store_index_writer,
                        dep_graph.as_ref(),
                        &deps_state_cache,
                        &ignored_builds,
                        layout,
                        pkg_roots_by_key,
                        gather_ancestor_bin_paths,
                        modules_dir,
                        lockfile_dir,
                        extra_env,
                        user_agent,
                        scripts_prepend_node_path,
                        script_shell,
                        shell_emulator,
                        unsafe_perm,
                        frozen_store,
                        ignore_scripts,
                        import_method,
                        logged_methods,
                        rebuild,
                    )
                })
            })?;
        }

        // If a chunk worker panicked while holding the
        // `ignored_builds` lock, rayon's `try_for_each` will have
        // already propagated the panic (or returned an Err) — so a
        // poisoned mutex here can only mean the protected state is
        // mid-insertion. A `BTreeSet::insert` is one atomic
        // operation from the data-structure's POV (no torn writes),
        // so the canonical poison-recovery pattern is safe.
        let ignored_builds =
            ignored_builds.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(BuildModulesOutput {
            ignored_builds: ignored_builds.into_iter().collect(),
            deferred_builds: deferred_builds(requires_build_map.iter(), ignore_scripts),
        })
    }
}

/// The snapshots `--ignore-scripts` kept from building, sorted for a
/// stable `.modules.yaml`.
///
/// The caller supplies the snapshots in scope. Normal build-module runs
/// pass every snapshot they inspected; the `ignoreScripts`
/// fast path passes only entries newly materialized by this install.
pub(crate) fn deferred_builds<'a>(
    requires_build: impl IntoIterator<Item = (&'a PackageKey, &'a bool)>,
    ignore_scripts: bool,
) -> Vec<String> {
    if !ignore_scripts {
        return Vec::new();
    }
    let mut deferred: Vec<String> = requires_build
        .into_iter()
        .filter(|&(_, &requires_build)| requires_build)
        .map(|(key, _)| key.to_string())
        .collect();
    deferred.sort();
    deferred
}

#[cfg(test)]
mod tests;
