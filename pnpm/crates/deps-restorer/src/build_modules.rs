pub(crate) mod allow_build_policy;
pub(crate) mod build_one_snapshot;
pub(crate) mod slots;
pub use allow_build_policy::{
    AllowBuildPolicy, allow_build_key_from_ignored_build, normalize_build_dep_path,
    parse_allow_build_selector,
};
pub(crate) use build_one_snapshot::build_one_snapshot;
pub(crate) use build_requirements::deferred_builds;
pub use build_requirements::{ScheduledBuilds, ScheduledBuildsInputs};
pub use slots::parse_name_version_from_key;
pub(crate) use slots::{
    PkgRoots, bin_dirs_in_all_parent_dirs, discard_skipped_optional_dependency,
    is_started_build_marker, mark_global_virtual_store_build_started, materialize_side_effects,
    slot_carries_overlay,
};

mod build_requirements;

use build_requirements::{
    RequiresBuildInputs, SideEffectsCacheGate, requires_build_by_key,
    side_effects_cache_gate_active,
};

use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, NEEDS_BUILD_MARKER, SkippedSnapshots,
    build_graph::build_graph,
    find_root_runtime_node_key, import_indexed_dir, store_index_key_for_resolution,
    version_policy::{
        PackageVersionPolicy, PolicyMatch, VersionPolicyError, create_package_version_policy,
        expand_package_version_specs,
    },
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{Config, PackageImportMethod};
use pnpm_deps_path::{get_pkg_id_with_patch_hash, index_of_dep_path_suffix, remove_suffix};
use pnpm_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_postinstall_hooks,
};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_patching::{PatchApplyError, apply_patch_to_dir};
use pnpm_reporter::{
    LogEvent, LogLevel, Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage,
    SkippedOptionalReason,
};
use pnpm_workspace_task_scheduler::{ScheduleGraphOptions, TaskCompletion, schedule_graph};
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

    /// Incompatible engine requirements after applying patches.
    #[diagnostic(transparent)]
    PatchedEngines(#[error(source)] Box<pnpm_package_is_installable::InstallabilityError>),

    /// Failure reading patched manifest.
    #[display("Cannot read the patched package.json of {dep_path}")]
    #[diagnostic(code(ERR_PNPM_PATCHED_MANIFEST_UNREADABLE))]
    PatchedManifestUnreadable { dep_path: String },

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

    /// The OS refused to spawn a scheduler worker, typically because the
    /// process or system thread limit was reached.
    #[display("Failed to build the per-install build scheduler: {source}")]
    #[diagnostic(
        code(ERR_PNPM_BUILD_THREAD_POOL),
        help(
            "Lower childConcurrency in pnpm-workspace.yaml, or raise the process's RLIMIT_NPROC."
        )
    )]
    ThreadPoolBuild {
        #[error(source)]
        source: std::io::Error,
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

    /// A global-virtual-store slot's `.pnpm-needs-build` marker exists but
    /// cannot be read, so whether another install's build left the slot
    /// half-built is unknown.
    #[display("Failed to read the build marker at {}: {source}", path.display())]
    ReadBuildMarker {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    /// An optional dependency's build failed and the package could not be
    /// removed, so it would stay installed half-built.
    #[display("Failed to remove {}, an optional dependency whose build failed: {source}", path.display())]
    RemoveSkippedOptionalDependency {
        path: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },
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
        self.selected_names
            .as_ref()
            .is_none_or(|names| names.contains(name))
    }

    /// Whether this rebuild discharges the workspace project recorded
    /// under `importer_id`, which only running its own scripts can do —
    /// dropping one the rebuild never ran would forget the debt rather
    /// than settle it.
    #[must_use]
    pub fn settles_project(&self, importer_id: &str) -> bool {
        self.pending_projects
            .iter()
            .any(|id| id == importer_id)
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
/// Packages are dispatched as soon as their dependencies finish, bounded by
/// [`BuildModules::child_concurrency`].
pub struct BuildModules<'a> {
    pub cache: crate::BuildCacheContext<'a>,
    pub directories: crate::BuildLayout<'a>,
    pub graph: crate::BuildGraphInputs<'a>,
    pub scripts: crate::BuildScriptOptions<'a>,
    pub allow_build_policy: &'a AllowBuildPolicy,
    /// Mirrors `config.child_concurrency`. Maximum concurrent build-script
    /// spawns. Floored to `1` to guarantee forward progress.
    pub child_concurrency: u32,
    /// Snapshots the installability pass marked optional+incompatible.
    /// Excluded from both `requires_build` computation and the
    /// build graph. Pacquet does not run scripts (or
    /// even check `binding.gyp`) for slots that don't exist on
    /// disk. Skipped snapshots never enter the build graph.
    pub skipped: &'a SkippedSnapshots,

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

    /// Whether any linked slot's contents may have changed during the
    /// build phase — a side-effects overlay was applied, a patch ran,
    /// or a lifecycle script was attempted. `false` on the common warm
    /// install where every candidate was ignored, deferred, or already
    /// built, which lets the post-build importer bin relink skip
    /// importers whose manifests provably match what the link phase
    /// already shimmed.
    pub mutated_slots: bool,
}

impl BuildModules<'_> {
    /// Run the build, reporting the packages that needed one but did
    /// not get it — see [`BuildModulesOutput`].
    pub fn run<Reporter: self::Reporter>(self) -> Result<BuildModulesOutput, BuildModulesError> {
        let Some(snapshots) = self.graph.snapshots else {
            return Ok(BuildModulesOutput::default());
        };

        let requires_build_map = self.requires_build_map(snapshots);
        let dep_states = self.dep_states(snapshots, &requires_build_map);
        let build_graph = build_graph(
            &requires_build_map,
            self.graph.patches,
            snapshots,
            self.graph.importers,
            self.graph.dependency_groups,
            self.skipped,
        );

        // Collect peer-stripped keys so the final list is unique and
        // sorted lexicographically — matches `dedupePackageNamesFromIgnoredBuilds`.
        // `Mutex` for the same parallelism reason as the dep-state cache.
        let ignored_builds: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
        let slot_mutations = std::sync::atomic::AtomicBool::new(false);
        let project_bin_dirs = self.project_bin_dirs(snapshots);
        schedule_builds::<Reporter>(
            &build_graph,
            &self.snapshot_context(
                snapshots,
                &requires_build_map,
                &dep_states,
                &ignored_builds,
                &slot_mutations,
                &project_bin_dirs,
            ),
            self.child_concurrency,
        )?;

        // If a scheduler worker panicked while holding the
        // `ignored_builds` lock, the scheduler will have
        // already propagated the panic (or returned an Err) — so a
        // poisoned mutex here can only mean the protected state is
        // mid-insertion. A `BTreeSet::insert` is one atomic
        // operation from the data-structure's POV (no torn writes),
        // so the canonical poison-recovery pattern is safe.
        let ignored_builds =
            ignored_builds.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(BuildModulesOutput {
            ignored_builds: ignored_builds.into_iter().collect(),
            deferred_builds: deferred_builds(requires_build_map.iter(), self.scripts.ignore),
            mutated_slots: slot_mutations.into_inner(),
        })
    }

    fn requires_build_map(
        &self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) -> HashMap<PackageKey, bool> {
        requires_build_by_key(RequiresBuildInputs {
            snapshots,
            skipped: self.skipped,
            pkg_roots: PkgRoots {
                layout: self.directories.layout,
                by_key: self.directories.pkg_roots_by_key,
            },
            prefetched: self.graph.requires_build_by_snapshot,
            patches: self.graph.patches,
        })
    }

    /// The project directories every dependency build script gets on `PATH`
    /// after the ones walked up from its own package: the root project's
    /// runtime `node`, the privately hoisted `node_modules/.bin`, and the
    /// configured extra bin paths (the workspace root's `node_modules/.bin`).
    /// The runtime comes first so a hoisted package's `node` bin cannot
    /// replace the Node.js whose version keys the slot.
    ///
    /// A global virtual store slot has no `node_modules` ancestor inside the
    /// project, so without these its scripts would miss them. They are given
    /// on purpose although the slot hash records only the runtime's version:
    /// `NODE_PATH` already exposes the root and hoisted `node_modules` to the
    /// same scripts, and builds that run a tool they do not declare would
    /// fail without them.
    fn project_bin_dirs(&self, snapshots: &HashMap<PackageKey, SnapshotEntry>) -> Vec<PathBuf> {
        let hoisted_bin_dir = self.scripts.path.private_hoisting
            .then_some(self.scripts.patched_engines.virtual_store_dir)
            .flatten()
            .map(|virtual_store_dir| virtual_store_dir.join("node_modules").join(".bin"));
        self.runtime_node_bin_dir(snapshots)
            .into_iter()
            .chain(hoisted_bin_dir)
            .chain(self.scripts.path.extra_bin_paths.iter().cloned())
            .collect()
    }

    /// The directory holding the `node` binary of the root project's
    /// `node@runtime:` dependency, the one that keys the engine part of every
    /// built slot's hash. `None` when `--no-runtime` skipped the runtime.
    fn runtime_node_bin_dir(
        &self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) -> Option<PathBuf> {
        let runtime_key = find_root_runtime_node_key(self.graph.importers, snapshots)?;
        if self.skipped.contains(runtime_key) {
            return None;
        }
        let pkg_dir =
            PkgRoots { layout: self.directories.layout, by_key: self.directories.pkg_roots_by_key }
                .canonical(runtime_key)?;
        Some(if cfg!(windows) { pkg_dir } else { pkg_dir.join("bin") })
    }

    fn snapshot_context<'a>(
        &'a self,
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        requires_build_map: &'a HashMap<PackageKey, bool>,
        dep_states: &'a DepStates,
        ignored_builds: &'a Mutex<BTreeSet<String>>,
        slot_mutations: &'a std::sync::atomic::AtomicBool,
        project_bin_dirs: &'a [PathBuf],
    ) -> build_one_snapshot::BuildOneSnapshot<'a> {
        build_one_snapshot::BuildOneSnapshot {
            cache: self.cache,
            directories: self.directories,
            graph: crate::BuildSnapshotInputs {
                snapshots,
                packages: self.graph.packages,
                patches: self.graph.patches,
                requires_build_map,
                importers: self.graph.importers,
            },
            progress: crate::BuildProgress {
                dep_graph: dep_states.graph.as_ref(),
                deps_state_cache: &dep_states.cache,
                ignored_builds,
                slot_mutations,
            },
            scripts: self.scripts,
            project_bin_dirs,

            allow_build_policy: self.allow_build_policy,

            rebuild: self.rebuild,
        }
    }

    /// Build the dep graph + state cache only when the
    /// side-effects-cache gate has a chance of firing — on either the
    /// READ side (prefetch surfaced cache rows) or the WRITE side (the
    /// install will be populating new cache entries after a successful
    /// build).
    ///
    /// The graph is bounded to the *forward closure of `requires_build`
    /// snapshots* via `build_deps_subgraph`. The upload-site and
    /// gate-check loops only ever compute cache keys for
    /// `requires_build` snapshots, and `calc_dep_state` only recurses
    /// into a snapshot's own children, so the closure-bounded graph
    /// produces the exact same cache keys as the full graph for every
    /// root we'll query. A pure-JS install with no `requires_build`
    /// snapshots feeds in an empty root iterator and the function
    /// returns immediately — O(0) walk for that path.
    fn dep_states(
        &self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
        requires_build_map: &HashMap<PackageKey, bool>,
    ) -> DepStates {
        let cache_gate_active = side_effects_cache_gate_active(&SideEffectsCacheGate {
            side_effects_cache: self.cache.read,
            side_effects_cache_write: self.cache.write,
            has_publisher: self.cache.publisher.is_some(),
            frozen_store: self.cache.frozen_store,
            has_engine_name: self.cache.engine_name.is_some(),
            can_write_store: self.cache.store_index_writer.is_some()
                && self.cache.store_dir.is_some(),
            has_packages: self.graph.packages.is_some(),
            has_cache_rows: self.cache.maps_by_snapshot.is_some_and(|map| !map.is_empty()),
        });
        let graph = cache_gate_active.then(|| {
            // Every requires-build snapshot is a root, including the ones
            // the install's dependency-group filter keeps out of the build
            // graph. Narrowing the roots to what will actually build would
            // make a package inside a dependency cycle hash differently
            // under `--prod` than under a full install, because
            // `warm_deps_state_cache` below resolves such a cycle by
            // whichever walk reaches it first.
            let roots = requires_build_map
                .iter()
                .filter(|&(_, &requires_build)| requires_build)
                .map(|(key, _)| key.clone());
            crate::build_deps_subgraph(
                snapshots,
                self.graph.packages.expect("`cache_gate_active` requires packages: Some"),
                roots,
            )
        });
        let cache = Mutex::new(pnpm_graph_hasher::DepsStateCache::new());
        // Prime it in lockfile key order before any build runs. The
        // ready nodes race for the mutex, and a snapshot inside a
        // dependency cycle takes the digest of whichever walk reached
        // it first — so an unprimed cache would hand the same install
        // a different side-effects-cache key on every run, and every
        // repeat install would re-run the build it already has cached.
        if let Some(graph) = &graph {
            let mut cache_guard = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            pnpm_graph_hasher::warm_deps_state_cache(
                graph,
                &mut cache_guard,
                crate::deps_graph::in_lockfile_order(graph).into_iter().map(|(key, _)| key),
            );
        }
        DepStates { graph, cache }
    }
}

/// What the side-effects-cache gate hashes over.
struct DepStates {
    graph: Option<HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>>,
    /// Memoizes per-snapshot hashes across the recursive walk in
    /// `calc_dep_state`. Shared across all scheduled nodes so
    /// diamond-shaped subgraphs hit the memo from earlier builds too.
    /// Wrapped in `Mutex` because the scheduler dispatches ready nodes
    /// concurrently. `calc_dep_state` mutates the cache through `&mut`,
    /// and worker threads would otherwise need each task to own a
    /// private cache, defeating the point of memoization.
    cache: Mutex<pnpm_graph_hasher::DepsStateCache<PackageKey>>,
}

/// Run every build in dependency order, bailing on the first failure.
fn schedule_builds<Reporter: self::Reporter>(
    build_graph: &indexmap::IndexMap<PackageKey, Vec<PackageKey>>,
    context: &build_one_snapshot::BuildOneSnapshot<'_>,
    child_concurrency: u32,
) -> Result<(), BuildModulesError> {
    let first_error: Mutex<Option<BuildModulesError>> = Mutex::new(None);
    let on_node_skipped: fn(&PackageKey) = |_| {};
    let run_node =
        |snapshot_key: PackageKey| match build_one_snapshot::<Reporter>(&snapshot_key, context) {
            Ok(()) => TaskCompletion::Passed,
            Err(error) => {
                first_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_or_insert(error);
                TaskCompletion::Failed
            }
        };
    schedule_graph(
        build_graph,
        &ScheduleGraphOptions {
            concurrency: crate::script_thread_count(child_concurrency, build_graph.len()),
            bail: true,
            continue_on_failure: false,
            run_node: &run_node,
            on_node_skipped: &on_node_skipped,
        },
    )
    .map_err(|source| BuildModulesError::ThreadPoolBuild { source })?;
    match first_error.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;

/// The executor's canonical `scriptsPrependNodePath`. Config's mirror
/// enum carries the yaml-deserialize impl; the executor's stays free of
/// serde wiring.
pub fn exec_scripts_prepend_node_path(config: &Config) -> ScriptsPrependNodePath {
    match config.scripts_prepend_node_path {
        pnpm_config::ScriptsPrependNodePath::Always => ScriptsPrependNodePath::Always,
        pnpm_config::ScriptsPrependNodePath::Never => ScriptsPrependNodePath::Never,
        pnpm_config::ScriptsPrependNodePath::WarnOnly => ScriptsPrependNodePath::WarnOnly,
    }
}
