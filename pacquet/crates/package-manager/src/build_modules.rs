use crate::{
    SkippedSnapshots,
    build_sequence::build_sequence,
    version_policy::{VersionPolicyError, expand_package_version_specs},
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_deps_path::remove_suffix;
use pacquet_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_postinstall_hooks,
};
use pacquet_lockfile::{PackageKey, ProjectSnapshot, SnapshotEntry};
use pacquet_package_manifest::pkg_requires_build;
use pacquet_patching::{PatchApplyError, apply_patch_to_dir};
use pacquet_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};
use rayon::prelude::*;
use std::{
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

    /// Mirrors upstream's
    /// [`ERR_PNPM_PATCH_FILE_PATH_MISSING`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L172-L176)
    /// — fired when a snapshot's resolved patch carries a hash but
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
        code(ERR_PACQUET_BUILD_THREAD_POOL),
        help(
            "Lower childConcurrency in pnpm-workspace.yaml, or raise the process's RLIMIT_NPROC."
        )
    )]
    ThreadPoolBuild {
        #[error(source)]
        source: rayon::ThreadPoolBuildError,
    },
}

/// Build policy derived from `allowBuilds` and
/// `dangerouslyAllowAllBuilds` in `pnpm-workspace.yaml`.
///
/// Ports pnpm's `createAllowBuildFunction` from
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/src/index.ts>.
///
/// The internal `expanded_allowed` and `expanded_disallowed` sets
/// contain the result of running each `allowBuilds` key through
/// [`expand_package_version_specs`], so a key like
/// `foo@1.0.0 || 2.0.0` lands as two separate `foo@1.0.0` and
/// `foo@2.0.0` entries that [`AllowBuildPolicy::check`] can match
/// via `HashSet::contains`.
///
/// The tri-state return from [`AllowBuildPolicy::check`]:
/// - `Some(true)`: explicitly allowed, run scripts
/// - `Some(false)`: explicitly denied, silently skip
/// - `None`: not in the list, skip and report as ignored
#[derive(Debug, Default)]
pub struct AllowBuildPolicy {
    expanded_allowed: HashSet<String>,
    expanded_disallowed: HashSet<String>,
    allowed_dep_paths: HashSet<String>,
    disallowed_dep_paths: HashSet<String>,
    dangerously_allow_all: bool,
}

impl AllowBuildPolicy {
    /// Build a policy from already-expanded `allowed` and
    /// `disallowed` sets and `dangerouslyAllowAllBuilds`. Pure
    /// constructor — no IO — so the policy logic is tested
    /// directly with in-memory inputs (mirrors upstream's
    /// `createAllowBuildFunction(opts)` in
    /// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/src/index.ts>).
    #[must_use]
    pub fn new(
        expanded_allowed: HashSet<String>,
        expanded_disallowed: HashSet<String>,
        dangerously_allow_all: bool,
    ) -> Self {
        Self {
            expanded_allowed,
            expanded_disallowed,
            allowed_dep_paths: HashSet::new(),
            disallowed_dep_paths: HashSet::new(),
            dangerously_allow_all,
        }
    }

    #[must_use]
    pub fn new_with_dep_paths(
        expanded_allowed: HashSet<String>,
        expanded_disallowed: HashSet<String>,
        allowed_dep_paths: HashSet<String>,
        disallowed_dep_paths: HashSet<String>,
        dangerously_allow_all: bool,
    ) -> Self {
        Self {
            expanded_allowed,
            expanded_disallowed,
            allowed_dep_paths,
            disallowed_dep_paths,
            dangerously_allow_all,
        }
    }

    /// Build the policy from a resolved [`Config`]. Reads
    /// `allow_builds` and `dangerously_allow_all_builds`, which are
    /// populated by [`pacquet_config::WorkspaceSettings::apply_to`]
    /// from `pnpm-workspace.yaml`. pnpm v11 stopped reading these
    /// from `package.json#pnpm` — see pnpm/pacquet#397 item 5.
    ///
    /// Each `allow_builds` key is partitioned by its boolean value
    /// into `allowed` / `disallowed` sets, then each set is
    /// expanded through [`expand_package_version_specs`] so version
    /// unions like `foo@1.0.0 || 2.0.0` become two literal
    /// `foo@1.0.0` / `foo@2.0.0` entries. Errors from the expansion
    /// (`ERR_PNPM_INVALID_VERSION_UNION` /
    /// `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`) surface to the
    /// caller — mirrors upstream's behavior of throwing from
    /// `expandPackageVersionSpecs` at config-load time.
    pub fn from_config(config: &Config) -> Result<Self, VersionPolicyError> {
        let mut allowed_specs: Vec<&str> = Vec::new();
        let mut disallowed_specs: Vec<&str> = Vec::new();
        let mut allowed_dep_paths = HashSet::new();
        let mut disallowed_dep_paths = HashSet::new();
        for (spec, &value) in &config.allow_builds {
            if is_dep_path_allow_build_key(spec) {
                if value {
                    allowed_dep_paths.insert(normalize_build_dep_path(spec));
                } else {
                    disallowed_dep_paths.insert(normalize_build_dep_path(spec));
                }
            } else {
                if value {
                    allowed_specs.push(spec);
                } else {
                    disallowed_specs.push(spec);
                }
            }
        }
        let expanded_allowed = expand_package_version_specs(allowed_specs)?;
        let expanded_disallowed = expand_package_version_specs(disallowed_specs)?;
        Ok(Self::new_with_dep_paths(
            expanded_allowed,
            expanded_disallowed,
            allowed_dep_paths,
            disallowed_dep_paths,
            config.dangerously_allow_all_builds,
        ))
    }

    /// Check whether a package is allowed to run build scripts.
    ///
    /// Returns:
    /// - `Some(true)`: explicitly allowed (or `dangerouslyAllowAllBuilds`)
    /// - `Some(false)`: explicitly denied, silently skip
    /// - `None`: not in the list, skip and report as ignored
    ///
    /// Mirrors upstream's
    /// [`createAllowBuildFunction`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/policy/src/index.ts#L35-L44)
    /// matching: `disallowed` checked first, then `allowed`, both
    /// against `name` and `name@version`. The `HashSet::contains`
    /// lookup means `*` wildcards in specs do NOT match real
    /// package names — see [`expand_package_version_specs`] for
    /// the rationale.
    #[must_use]
    pub fn check(&self, dep_path: &str) -> Option<bool> {
        if self.dangerously_allow_all {
            return Some(true);
        }

        let normalized_dep_path = normalize_build_dep_path(dep_path);
        if self.disallowed_dep_paths.contains(&normalized_dep_path) {
            return Some(false);
        }
        let (name, version) = parse_name_version_from_key(&normalized_dep_path);
        let name_at_version = format!("{name}@{version}");
        if self.expanded_disallowed.contains(&name)
            || self.expanded_disallowed.contains(&name_at_version)
        {
            return Some(false);
        }
        if self.allowed_dep_paths.contains(&normalized_dep_path) {
            return Some(true);
        }
        // Package-name rules require a trusted package identity. A
        // registry-style dep path (`name@semver`) is the trust signal: the
        // lockfile verification gate rejects lockfiles where such a key is
        // backed by a non-registry resolution, so by the time scripts can
        // run, the shape proves the artifact came from a registry.
        if node_semver::Version::parse(&version).is_err() {
            return None;
        }
        if self.expanded_allowed.contains(&name) || self.expanded_allowed.contains(&name_at_version)
        {
            return Some(true);
        }

        None
    }
}

/// Strips the peer suffix (and, matching [`PkgVerPeer::without_peer`]'s
/// lumped suffix handling, the patch hash) so config keys compare equal
/// to the `metadata_key.to_string()` form used at the runtime call sites.
///
/// [`PkgVerPeer::without_peer`]: pacquet_lockfile::PkgVerPeer::without_peer
pub(crate) fn normalize_build_dep_path(dep_path: &str) -> String {
    remove_suffix(dep_path).to_string()
}

fn is_dep_path_allow_build_key(spec: &str) -> bool {
    if normalize_build_dep_path(spec) != spec {
        return true;
    }
    if spec.contains("||") {
        return false;
    }
    let (_, version) = parse_name_version_from_key(spec);
    if version.is_empty() {
        return !spec.starts_with('@') && (spec.contains('/') || spec.contains(':'));
    }
    node_semver::Version::parse(&version).is_err() && is_source_like_dep_path_version(&version)
}

fn is_source_like_dep_path_version(version: &str) -> bool {
    version.contains(':') || version.contains('/') || version.contains('#')
}

/// Run lifecycle scripts for all packages that require a build.
///
/// Ports the core of `buildModules` from
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts>.
///
/// Packages are visited in topological order (children before parents) via
/// [`build_sequence`]. Chunks run sequentially. Members within a chunk
/// run in parallel under a per-install rayon thread pool bounded to
/// [`BuildModules::child_concurrency`] threads — mirrors upstream's
/// [`runGroups(getWorkspaceConcurrency(opts.childConcurrency), groups)`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L124).
pub struct BuildModules<'a> {
    /// Install-scoped slot-directory mapping (GVS-aware). Replaces the
    /// previous `virtual_store_dir: &Path` field — the layout already
    /// knows the per-snapshot subdirectory shape (legacy flat-name vs
    /// GVS `<scope>/<name>/<version>/<hash>`). See
    /// [`crate::VirtualStoreLayout`].
    pub layout: &'a crate::VirtualStoreLayout,
    pub modules_dir: &'a Path,
    pub lockfile_dir: &'a Path,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, pacquet_lockfile::PackageMetadata>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub allow_build_policy: &'a AllowBuildPolicy,
    /// Per-snapshot side-effects-cache overlays — passed in from
    /// `CreateVirtualStore`'s prefetch. `None` means the cache is
    /// disabled or no rows were prefetched; the gate falls through
    /// to "rebuild" for every snapshot.
    pub side_effects_maps_by_snapshot: Option<&'a crate::SideEffectsMapsBySnapshot>,
    /// `<platform>;<arch>;node<major>` — the prefix part of
    /// upstream's dep-state cache key. Computed once at install
    /// start by [`pacquet_graph_hasher::detect_node_major`] +
    /// [`pacquet_graph_hasher::engine_name`]. When `None`, the
    /// gate falls through to "rebuild" (no key to look up).
    pub engine_name: Option<&'a str>,
    /// Mirrors `config.side_effects_cache`. When `false`, the
    /// gate is bypassed entirely and every `requires_build`
    /// snapshot runs its scripts.
    pub side_effects_cache: bool,
    /// Mirrors upstream's `sideEffectsCacheWrite` at
    /// <https://github.com/pnpm/pnpm/blob/7e3145f9fc/config/reader/src/index.ts#L615>.
    /// When `true`, a successful postinstall triggers a re-CAFS of
    /// the built package directory and a queued mutation of the
    /// matching `PackageFilesIndex.sideEffects` row.
    pub side_effects_cache_write: bool,
    /// Store-dir handle for the WRITE path's `add_files_from_dir`
    /// call. `None` short-circuits the upload site entirely — used
    /// by unit tests that don't set up a CAFS.
    pub store_dir: Option<&'a pacquet_store_dir::StoreDir>,
    /// Shared batched writer for the side-effects upload's
    /// read-modify-write of the existing `PackageFilesIndex` row.
    /// `None` short-circuits the upload site.
    pub store_index_writer: Option<&'a std::sync::Arc<pacquet_store_dir::StoreIndexWriter>>,
    /// Per-snapshot resolved patch metadata. Keyed by the snapshot's
    /// peer-stripped `PackageKey`, value is the matching
    /// `ExtendedPatchInfo` (hash + absolute path) computed by
    /// [`pacquet_patching::resolve_and_group`] + per-snapshot
    /// [`pacquet_patching::get_patch_info`]. `None` when no
    /// `patchedDependencies` is configured.
    ///
    /// Drives three things, mirroring upstream's
    /// [`during-install`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts)
    /// flow:
    ///
    /// 1. Build trigger — a snapshot with a patch entry becomes a
    ///    build candidate even when `requires_build` is false.
    /// 2. Side-effects-cache key — `patch_file_hash` carries the
    ///    SHA-256 hex into [`pacquet_graph_hasher::CalcDepStateOptions`].
    /// 3. Patch application — the patch is applied to the extracted
    ///    package dir before postinstall hooks run.
    pub patches: Option<&'a HashMap<PackageKey, pacquet_patching::ExtendedPatchInfo>>,
    /// Mirrors `config.scripts_prepend_node_path`. Threaded through to
    /// [`RunPostinstallHooks::scripts_prepend_node_path`] for each
    /// spawned lifecycle script. Default [`ScriptsPrependNodePath::Never`].
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    /// Mirrors `config.unsafe_perm`. When `false`, [`pacquet_executor`]
    /// runs each lifecycle script under a per-package TMPDIR set to
    /// `node_modules/.tmp`; when `true`, TMPDIR is left at the
    /// inherited value (matches upstream's
    /// [`@pnpm/npm-lifecycle`](https://github.com/pnpm/npm-lifecycle/blob/d2d8e790/index.js#L204-L220)
    /// gate). Default `true`.
    pub unsafe_perm: bool,
    /// Mirrors `config.child_concurrency`. Per-chunk parallelism
    /// for build-script spawns. Chunks remain sequential to preserve
    /// topological ordering; members within a chunk run in parallel
    /// up to this many at a time. Mirrors upstream's
    /// [`runGroups(getWorkspaceConcurrency(opts.childConcurrency), groups)`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L124).
    /// Floored to `1` to guarantee forward progress on
    /// resource-constrained hosts.
    pub child_concurrency: u32,
    /// Snapshots the installability pass marked optional+incompatible.
    /// Excluded from both `requires_build` computation and the
    /// `build_sequence` input — pacquet does not run scripts (or
    /// even check `binding.gyp`) for slots that don't exist on
    /// disk. Mirrors pnpm's `lockfileToDepGraph` flow where skipped
    /// snapshots never enter the build graph.
    pub skipped: &'a SkippedSnapshots,

    /// Per-snapshot `pkgRoot` override, populated by the hoisted
    /// linker with the slice 4 walker's
    /// [`crate::DependenciesGraphNode::dir`] values. When `Some`,
    /// every `pkgRoot` lookup goes through this map instead of the
    /// virtual-store-layout slot computation; a missing entry means
    /// the snapshot didn't make it into the hoisted graph (skipped
    /// optional, etc.) and the build phase silently passes over it.
    /// `None` for the isolated linker — its slot directories are
    /// recovered from [`crate::VirtualStoreLayout::slot_dir`].
    /// Mirrors upstream's two-mode `pkgRoots` selection at
    /// [`building/after-install/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L340-L350).
    pub pkg_root_by_key: Option<&'a HashMap<PackageKey, PathBuf>>,

    /// When `true`, compute per-snapshot `extra_bin_paths` via
    /// `bin_dirs_in_all_parent_dirs` (private helper in this module)
    /// so lifecycle scripts can resolve binaries from every ancestor `node_modules/.bin`
    /// up to [`Self::lockfile_dir`]. Mirrors upstream's hoisted
    /// branch at
    /// [`after-install:357`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L357).
    /// Always `false` under the isolated linker — its bins live in
    /// the slot's own `<slot>/node_modules/.bin`, populated up-
    /// front by [`crate::LinkVirtualStoreBins`], and the script
    /// executor adds that path itself.
    pub gather_ancestor_bin_paths: bool,
}

impl BuildModules<'_> {
    /// Run the build, returning the sorted set of `name@version` keys whose
    /// scripts were skipped because the package was not in `allowBuilds`.
    ///
    /// The caller is expected to fold the returned set into a single
    /// `pnpm:ignored-scripts` event — mirroring upstream's emit at
    /// <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/index.ts#L414>.
    pub fn run<Reporter: self::Reporter>(self) -> Result<Vec<String>, BuildModulesError> {
        let BuildModules {
            layout,
            modules_dir,
            lockfile_dir,
            snapshots,
            packages,
            importers,
            allow_build_policy,
            side_effects_maps_by_snapshot,
            engine_name,
            side_effects_cache,
            side_effects_cache_write,
            store_dir,
            store_index_writer,
            patches,
            scripts_prepend_node_path,
            unsafe_perm,
            child_concurrency,
            skipped,
            pkg_root_by_key,
            gather_ancestor_bin_paths,
        } = self;

        let Some(snapshots) = snapshots else { return Ok(Vec::new()) };

        let extra_env = HashMap::new();

        // Compute requires_build per snapshot from each extracted package
        // directory. Mirrors upstream where the worker computes
        // `node.requiresBuild` from the package's manifest scripts and the
        // presence of `binding.gyp` / `.hooks/` after extraction
        // (`https://github.com/pnpm/pnpm/blob/80037699fb/building/pkg-requires-build/src/index.ts`).
        // Pacquet does this here rather than in a worker because the worker
        // does not exist yet — it is the same per-package on-disk inspection,
        // moved to the build entry point.
        let requires_build_map: HashMap<PackageKey, bool> = snapshots
            .keys()
            // Skip snapshots that never landed on disk. `pkg_requires_build`
            // would just return `false` for a missing dir, but the
            // walk would still spend a syscall per skipped key — the
            // filter short-circuits that on installs with large
            // optional fan-out.
            .filter(|key| !skipped.contains(key))
            .map(|key| {
                // Hoisted snapshots without a recorded `pkgRoot` (the
                // walker dropped them) get `requires_build = false`
                // so they fall through both the script-runner and the
                // patch-apply gates without a syscall, matching the
                // isolated path's `pkg_dir.exists() == false` skip.
                let requires = pkg_root_for_key(layout, pkg_root_by_key, key)
                    .as_deref()
                    .is_some_and(pkg_requires_build);
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
        // Mirrors upstream's per-install `DepsStateCache` at
        // <https://github.com/pnpm/pnpm/blob/7e3145f9fc/building/during-install/src/index.ts#L74>.
        // The cache memoizes per-node hash across diamond-shaped
        // subgraphs so the recursive walk stays linear in
        // |closure| even when the same dep is reachable through
        // many parents.
        let read_gate_active = side_effects_cache
            && engine_name.is_some()
            && side_effects_maps_by_snapshot.is_some_and(|map| !map.is_empty());
        let write_gate_active = side_effects_cache_write
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
        let deps_state_cache: Mutex<pacquet_graph_hasher::DepsStateCache<PackageKey>> =
            Mutex::new(pacquet_graph_hasher::DepsStateCache::new());

        let chunks = build_sequence(&requires_build_map, patches, snapshots, importers, skipped);

        // Collect peer-stripped keys so the final list is unique and
        // sorted lexicographically — matches `dedupePackageNamesFromIgnoredBuilds`.
        // `Mutex` for the same parallelism reason as `deps_state_cache` above.
        let ignored_builds: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

        // Per-install rayon pool. Bounded to `child_concurrency` so
        // a chunk with many build-needed members doesn't exhaust the
        // process's rayon-global threads (which other crates may
        // depend on). One pool reused across all chunks; chunks
        // themselves run sequentially.
        //
        // Mirrors upstream's
        // [`runGroups(getWorkspaceConcurrency(opts.childConcurrency), groups)`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L124).
        // `ThreadPoolBuilder::build()` is fallible — the OS may
        // refuse the spawn (`EAGAIN` / RLIMIT_NPROC) on a host
        // already near its process-thread limit. Surface that as
        // [`BuildModulesError::ThreadPoolBuild`] so the install
        // returns cleanly with a remediation hint instead of
        // panicking inside the binary.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(child_concurrency.max(1) as usize)
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
                        pkg_root_by_key,
                        gather_ancestor_bin_paths,
                        modules_dir,
                        lockfile_dir,
                        &extra_env,
                        scripts_prepend_node_path,
                        unsafe_perm,
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
        Ok(ignored_builds.into_iter().collect())
    }
}

/// Per-snapshot work extracted out of [`BuildModules::run`]'s inner
/// loop so the bounded-parallelism `par_iter().try_for_each(...)`
/// dispatch can call it once per chunk member. The body is the same
/// as the pre-`#12` sequential loop — `continue`s become `return Ok(())`
/// here.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameters are independent inputs; bundling them into a struct would not improve clarity"
)]
fn build_one_snapshot<Reporter: self::Reporter>(
    snapshot_key: &PackageKey,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: Option<&HashMap<PackageKey, pacquet_lockfile::PackageMetadata>>,
    patches: Option<&HashMap<PackageKey, pacquet_patching::ExtendedPatchInfo>>,
    requires_build_map: &HashMap<PackageKey, bool>,
    allow_build_policy: &AllowBuildPolicy,
    side_effects_maps_by_snapshot: Option<&crate::SideEffectsMapsBySnapshot>,
    engine_name: Option<&str>,
    side_effects_cache: bool,
    side_effects_cache_write: bool,
    store_dir: Option<&pacquet_store_dir::StoreDir>,
    store_index_writer: Option<&std::sync::Arc<pacquet_store_dir::StoreIndexWriter>>,
    dep_graph: Option<&HashMap<PackageKey, pacquet_graph_hasher::DepsGraphNode<PackageKey>>>,
    deps_state_cache: &Mutex<pacquet_graph_hasher::DepsStateCache<PackageKey>>,
    ignored_builds: &Mutex<BTreeSet<String>>,
    layout: &crate::VirtualStoreLayout,
    pkg_root_by_key: Option<&HashMap<PackageKey, PathBuf>>,
    gather_ancestor_bin_paths: bool,
    modules_dir: &Path,
    lockfile_dir: &Path,
    extra_env: &HashMap<String, String>,
    scripts_prepend_node_path: ScriptsPrependNodePath,
    unsafe_perm: bool,
) -> Result<(), BuildModulesError> {
    let metadata_key = snapshot_key.without_peer();
    // Look up against the peer-stripped key because patches are
    // configured at the (name, version) granularity in
    // `pnpm-workspace.yaml`, not per peer-resolution variant.
    let patch = patches.and_then(|map| map.get(&metadata_key));
    let has_patch = patch.is_some();
    let requires_build = requires_build_map.get(snapshot_key).copied().unwrap_or(false);

    // Ancestors of a build/patch candidate are included in the
    // sequence (so the topo order stays correct) but only run
    // scripts / apply patches when they themselves are candidates.
    // Mirrors upstream's chunk filter at
    // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L73-L77>.
    if !requires_build && !has_patch {
        return Ok(());
    }

    let (name, version) = parse_name_version_from_key(&metadata_key.to_string());

    // Mirrors upstream's `if (node.requiresBuild) { allowBuild(...) }`
    // at lines 88-101: the allowBuilds gate only applies when the
    // node has scripts to run. A patched-only package skips this
    // check entirely and proceeds to patch application below.
    //
    // `false` / `None` from the policy set `should_run_scripts =
    // false` (NOT early-return), so the patch still gets applied
    // even when scripts are disallowed. Matches upstream's
    // `ignoreScripts = true; break` pattern.
    let mut should_run_scripts = requires_build;
    if requires_build {
        let dep_path = metadata_key.to_string();
        match allow_build_policy.check(&dep_path) {
            Some(false) => {
                should_run_scripts = false;
            }
            None => {
                // "Not in allowBuilds" — surfaced as
                // `pnpm:ignored-scripts`. Explicit `false` is
                // silently denied (above), matching upstream's
                // switch.
                // Poison-recover: see the equivalent call site at
                // the end of `BuildModules::run` for the safety
                // argument (BTreeSet insertion is atomic from the
                // data-structure's POV).
                ignored_builds
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(dep_path);
                should_run_scripts = false;
            }
            Some(true) => {}
        }
    }

    // Compute the side-effects cache key once per snapshot, before
    // the `is_built` gate. The same value is later consumed by the
    // WRITE-path upload call after `run_postinstall_hooks`
    // succeeds, so recomputing it there would just duplicate work —
    // `deps_state_cache` makes the second call free anyway, but
    // routing through one `let` keeps the gate-side and write-side
    // keys provably identical.
    //
    // `None` when the cache gate can't fire (no engine, no graph,
    // etc.); both downstream consumers short-circuit on `None`.
    //
    // The `deps_state_cache` is shared across all chunk members via
    // `Mutex` because `calc_dep_state` is recursive and memoizes —
    // a per-task cache would defeat the memoization for
    // diamond-shaped subgraphs.
    let cache_key = (dep_graph.zip(engine_name)).map(|(graph, engine)| {
        // Poison-recover: `calc_dep_state` mutates the cache by
        // inserting one entry per recursive walk node, each
        // insert atomic from `HashMap`'s POV. A panic mid-walk
        // leaves the map in a usable state — the worst case is
        // an unfinished sub-walk that the next caller will redo.
        let mut cache_guard =
            deps_state_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        pacquet_graph_hasher::calc_dep_state(
            graph,
            &mut cache_guard,
            snapshot_key,
            &pacquet_graph_hasher::CalcDepStateOptions {
                engine_name: engine,
                // Mirrors upstream's
                // `patchFileHash: depNode.patch?.hash` at
                // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L201>.
                // `None` for unpatched snapshots leaves the
                // `;patch=...` segment off the cache key entirely,
                // matching upstream when `depNode.patch == null`.
                patch_file_hash: patch.map(|patch| patch.hash.as_str()),
                // Mirrors `includeDepGraphHash: hasSideEffects` at
                // upstream line 202. A patched-only snapshot (no
                // scripts will run) leaves the deps-hash off so the
                // cache key stays stable across dep-graph changes
                // that don't affect this package's patched output.
                include_dep_graph_hash: should_run_scripts,
            },
        )
    });

    // Side-effects-cache `is_built` gate. Mirrors upstream's
    // `!node.isBuilt` filter at
    // <https://github.com/pnpm/pnpm/blob/7e3145f9fc/building/during-install/src/index.ts#L73-L77>.
    // We're already past the policy gate, so this snapshot would
    // otherwise run its scripts — but if the prefetch surfaced a
    // matching side-effects-cache entry, the build is already
    // represented on disk (pnpm seeded it on a previous install)
    // and we can skip.
    if side_effects_cache
        && let Some(maps_by_snapshot) = side_effects_maps_by_snapshot
        && let Some(maps) = maps_by_snapshot.get(snapshot_key)
        && let Some(key) = cache_key.as_deref()
        && maps.contains_key(key)
    {
        tracing::debug!(
            target: "pacquet::build",
            ?snapshot_key,
            cache_key = key,
            "side-effects cache hit; skipping build",
        );
        return Ok(());
    }

    // Hoisted snapshots without a recorded `pkgRoot` (the walker
    // dropped them — pre-skipped, optional skip, etc.) take the
    // same exit as the isolated path's `!pkg_dir.exists()` skip.
    let Some(pkg_dir) = pkg_root_for_key(layout, pkg_root_by_key, snapshot_key) else {
        return Ok(());
    };
    if !pkg_dir.exists() {
        return Ok(());
    }

    // Per-snapshot `extra_bin_paths`. Isolated leaves it empty;
    // hoisted gathers every ancestor's `node_modules/.bin` up to
    // `lockfile_dir` so a lifecycle script invoked at a nested
    // hoisted location can resolve bins added by parents.
    let extra_bin_paths: Vec<PathBuf> = if gather_ancestor_bin_paths {
        bin_dirs_in_all_parent_dirs(&pkg_dir, lockfile_dir)
    } else {
        Vec::new()
    };

    let optional = snapshots.get(snapshot_key).is_some_and(|entry| entry.optional);

    // Apply the patch before running postinstall hooks. Mirrors
    // upstream at
    // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L171-L178>:
    // ```
    // if (depNode.patch) {
    //   if (!depNode.patch.patchFilePath) throw PATCH_FILE_PATH_MISSING
    //   isPatched = applyPatchToDir(...)
    // }
    // ```
    // `is_patched` feeds the cache-write gate below
    // (`is_patched || has_side_effects`), matching upstream's
    // line 199 condition.
    let is_patched = if let Some(p) = patch {
        let patch_file_path = p.patch_file_path.as_deref().ok_or_else(|| {
            BuildModulesError::PatchFilePathMissing { dep_path: snapshot_key.to_string() }
        })?;
        apply_patch_to_dir(&pkg_dir, patch_file_path).map_err(BuildModulesError::PatchApply)?;
        true
    } else {
        false
    };

    let has_side_effects = if should_run_scripts {
        let result = run_postinstall_hooks::<Reporter>(&RunPostinstallHooks {
            dep_path: &snapshot_key.to_string(),
            pkg_root: &pkg_dir,
            root_modules_dir: modules_dir,
            init_cwd: lockfile_dir,
            extra_bin_paths: &extra_bin_paths,
            extra_env,
            node_execpath: None,
            npm_execpath: None,
            node_gyp_path: None,
            user_agent: None,
            unsafe_perm,
            node_gyp_bin: None,
            scripts_prepend_node_path,
            script_shell: None,
            optional,
        });

        match result {
            Ok(ran) => ran,
            Err(err) => {
                if optional {
                    // Mirrors
                    // `building/during-install/src/index.ts:226-238`:
                    // a build failure on an optional dep is logged
                    // through the `pnpm:skipped-optional-dependency`
                    // channel and swallowed so the install can
                    // continue. The `package.id` field upstream is
                    // `depNode.dir`; we use the same.
                    Reporter::emit(&LogEvent::SkippedOptionalDependency(
                        SkippedOptionalDependencyLog {
                            level: LogLevel::Debug,
                            details: Some(err.to_string()),
                            package: SkippedOptionalPackage::Installed {
                                id: pkg_dir.to_string_lossy().into_owned(),
                                name,
                                version,
                            },
                            prefix: lockfile_dir.to_string_lossy().into_owned(),
                            reason: SkippedOptionalReason::BuildFailure,
                        },
                    ));
                    return Ok(());
                }
                return Err(BuildModulesError::LifecycleScript(err));
            }
        }
    } else {
        false
    };

    // Side-effects-cache WRITE path. Mirrors upstream at
    // <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/src/index.ts#L198-L216>:
    // after a successful `run_postinstall_hooks` (or a patch
    // application that mutated the dir), re-hash the package
    // directory and queue a
    // `PackageFilesIndex.sideEffects[cache_key] = diff` mutation
    // so a future install can skip the rebuild.
    //
    // Upstream's gate is `(isPatched || hasSideEffects) &&
    // opts.sideEffectsCacheWrite`. Pacquet mirrors that — a
    // patched-only snapshot still uploads its post-patch state so
    // subsequent installs hit the cache.
    //
    // The other preconditions: cache_key composable (engine + graph
    // present), `packages` map available for the integrity lookup,
    // and the metadata row carries an integrity (registry / tarball
    // resolutions — git / directory have no integrity and pnpm
    // doesn't cache those either).
    //
    // All errors are swallowed with a `tracing::warn!`, matching
    // upstream's `try { upload } catch (err) { logger.warn(...) }`
    // at lines 208-215. A failed upload doesn't fail the install:
    // the next install re-runs the build.
    if (is_patched || has_side_effects)
        && side_effects_cache_write
        && let Some(writer) = store_index_writer
        && let Some(store) = store_dir
        && let Some(cache_key) = cache_key.as_deref()
        && let Some(packages) = packages
        && let Some(metadata) = packages.get(&metadata_key)
        && let Some(integrity) = metadata.resolution.integrity()
    {
        let files_index_file =
            pacquet_store_dir::store_index_key(&integrity.to_string(), &metadata_key.to_string());
        if let Err(err) =
            pacquet_store_dir::upload(store, &pkg_dir, &files_index_file, cache_key, writer)
        {
            tracing::warn!(
                target: "pacquet::build",
                ?err,
                dep_path = %snapshot_key,
                "side-effects cache upload failed; build proceeds",
            );
        }
    }

    Ok(())
}

/// Compute the package directory inside the virtual store for a snapshot key.
///
/// Routes the slot-dir lookup through the install-scoped
/// [`crate::VirtualStoreLayout`], which precomputes
/// `<scope>/<name>/<version>/<hash>` suffixes per *full* snapshot key
/// (with the peer-dependency suffix preserved) when GVS is enabled.
/// Peer-resolved snapshots therefore have to look up by the full key
/// — `slot_dir(key)` — or the GVS lookup misses, falls through to the
/// legacy flat-name path, and points at a directory that
/// [`crate::CreateVirtualDirBySnapshot`] never created.
/// `slot_dir(key.without_peer())` was the pre-[#432] spelling and
/// silently dropped lifecycle scripts for peer-resolved snapshots
/// — never use it here.
///
/// The package-name segment still comes from the peer-stripped key,
/// because the slot's `node_modules/<pkg>` is keyed by the bare
/// package name regardless of peer context.
///
/// [#432]: https://github.com/pnpm/pacquet/issues/432
fn virtual_store_dir_for_key(layout: &crate::VirtualStoreLayout, key: &PackageKey) -> PathBuf {
    let bare_key = key.without_peer();
    let key_str = bare_key.to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);

    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let name = &name_version[..at_idx];

    layout.slot_dir(key).join("node_modules").join(name)
}

/// Resolve the on-disk package directory for a snapshot.
///
/// Two-mode lookup mirroring upstream's
/// [`pkgRoots`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L340-L350)
/// computation:
///
/// - **Isolated** (`pkg_root_by_key.is_none()`) — fall through to
///   [`virtual_store_dir_for_key`], which routes through the
///   install-scoped [`crate::VirtualStoreLayout`].
/// - **Hoisted** (`pkg_root_by_key.is_some()`) — look the snapshot up
///   in the per-key map populated from the slice 4 walker's
///   [`crate::DependenciesGraphNode::dir`] values. `None` here means
///   the snapshot is absent from the hoisted graph (pre-skipped, or
///   the walker decided not to record it); the caller should treat
///   that the same as the isolated `pkg_dir.exists() == false` skip.
fn pkg_root_for_key(
    layout: &crate::VirtualStoreLayout,
    pkg_root_by_key: Option<&HashMap<PackageKey, PathBuf>>,
    key: &PackageKey,
) -> Option<PathBuf> {
    match pkg_root_by_key {
        Some(map) => map.get(key).cloned(),
        None => Some(virtual_store_dir_for_key(layout, key)),
    }
}

/// Mirrors upstream's
/// [`binDirsInAllParentDirs`](https://github.com/pnpm/pnpm/blob/94240bc046/building/after-install/src/index.ts#L476-L487)
/// — walk every ancestor `node_modules/.bin` from `pkg_root` up to
/// (and including) `lockfile_dir`. Used as the per-snapshot
/// `extra_bin_paths` under `nodeLinker: hoisted` so a lifecycle
/// script invoked at a nested location can resolve bins added by
/// any ancestor's `node_modules/.bin` — npm-style ancestor-chain
/// resolution that the isolated layout doesn't need (every slot's
/// children sit in its own `node_modules`, and bin-link writes are
/// per-slot).
///
/// The `path.dirname(dir)[0] === '@'` check upstream skips
/// pushing when `dir`'s parent path string starts with `@` — a
/// guard for relative-path code paths in the upstream codebase.
/// Mirrored here against the parent's path-string first character
/// for byte-for-byte parity with upstream's output.
///
/// Non-existent ancestor `.bin` directories are harmless: they
/// just don't contribute anything to lifecycle-script PATH lookup.
fn bin_dirs_in_all_parent_dirs(pkg_root: &Path, lockfile_dir: &Path) -> Vec<PathBuf> {
    let mut bin_dirs: Vec<PathBuf> = Vec::new();
    let mut dir: PathBuf = pkg_root.to_path_buf();
    loop {
        let parent = dir.parent().unwrap_or_else(|| Path::new(""));
        let parent_starts_with_at =
            parent.to_str().and_then(|text| text.chars().next()).is_some_and(|ch| ch == '@');
        if !parent_starts_with_at {
            bin_dirs.push(dir.join("node_modules").join(".bin"));
        }
        dir = parent.to_path_buf();
        if dir == *lockfile_dir || dir.as_os_str().is_empty() {
            break;
        }
    }
    bin_dirs.push(lockfile_dir.join("node_modules").join(".bin"));
    bin_dirs
}

/// Parse `name` and `version` from a lockfile snapshot key like
/// `/@pnpm.e2e/install-script-example@1.0.0`.
pub(crate) fn parse_name_version_from_key(key: &str) -> (String, String) {
    let stripped = key.strip_prefix('/').unwrap_or(key);
    match stripped.rfind('@') {
        Some(idx) if idx > 0 => (stripped[..idx].to_string(), stripped[idx + 1..].to_string()),
        _ => (stripped.to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests;
