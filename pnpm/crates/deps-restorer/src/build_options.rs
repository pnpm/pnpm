use pnpm_config::PackageImportMethod;
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::{PackageKey, ProjectSnapshot, SnapshotEntry};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Mutex, atomic::AtomicBool},
};

#[derive(Clone, Copy)]
pub struct BuildScriptOptions<'a> {
    pub extra_env: &'a HashMap<String, String>,
    /// Mirrors `config.user_agent`, stamped into each build script's
    /// `npm_config_user_agent`.
    pub user_agent: &'a str,
    /// Mirrors `config.scripts_prepend_node_path`. Threaded through to
    /// [`pnpm_executor::ScriptExecutionOptions::prepend_node_path`] for each
    /// spawned lifecycle script. Default [`ScriptsPrependNodePath::Never`].
    pub prepend_node_path: ScriptsPrependNodePath,
    /// Mirrors `config.script_shell`. Threaded through to
    /// [`pnpm_executor::ScriptExecutionOptions::shell`], so a workspace that
    /// configures a shell gets it for build scripts too, not only for
    /// `pnpm run`. `None` selects the platform default.
    pub shell: Option<&'a Path>,
    /// Mirrors `config.shell_emulator`. Threaded through to
    /// [`pnpm_executor::ScriptExecutionOptions::shell_emulator`], so build scripts run
    /// under the built-in shell wherever `pnpm run` would.
    pub shell_emulator: bool,
    /// Mirrors `config.unsafe_perm`. When `false`, [`pnpm_executor`]
    /// runs each lifecycle script under a per-package TMPDIR set to
    /// `node_modules/.tmp`; when `true`, TMPDIR is left at the
    /// inherited value. Default `true`.
    pub unsafe_perm: bool,

    /// Mirrors `config.ignore_scripts`. When `true`, no lifecycle
    /// script runs and the allow-build gate is bypassed entirely, so a
    /// package not in `allowBuilds` is *not* added to the returned
    /// ignored-builds set. Patches still apply — a patch is applied
    /// even when scripts are suppressed.
    pub ignore: bool,
    /// Mirrors `config.engineStrict`. A patched package's engines are checked
    /// against the patched manifest, after the patch is applied.
    pub engine_strict: bool,
    /// Mirrors `config.nodeVersion`. `None` detects the running Node.js.
    pub node_version: Option<&'a str>,
}

impl<'a> BuildScriptOptions<'a> {
    pub fn from_config(
        config: &'a pnpm_config::Config,
        extra_env: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            extra_env,
            user_agent: &config.user_agent,
            prepend_node_path: crate::build_modules::exec_scripts_prepend_node_path(config),
            shell: config.script_shell.as_deref().map(Path::new),
            shell_emulator: config.shell_emulator,
            unsafe_perm: config.unsafe_perm,
            ignore: config.ignore_scripts,
            engine_strict: config.effective_engine_strict(),
            node_version: config.node_version.as_deref(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct BuildCacheContext<'a> {
    /// Per-snapshot side-effects-cache overlays — passed in from
    /// `CreateVirtualStore`'s prefetch. `None` means the cache is
    /// disabled or no rows were prefetched; the gate falls through
    /// to "rebuild" for every snapshot.
    pub maps_by_snapshot: Option<&'a crate::SideEffectsMapsBySnapshot>,
    /// `<platform>;<arch>;node<major>` — the prefix part of the
    /// dep-state cache key. Computed once at install
    /// start by [`pnpm_graph_hasher::detect_node_major`] +
    /// [`pnpm_graph_hasher::engine_name`]. When `None`, the
    /// gate falls through to "rebuild" (no key to look up).
    pub engine_name: Option<&'a str>,
    /// Mirrors `config.side_effects_cache`. When `false`, the
    /// gate is bypassed entirely and every `requires_build`
    /// snapshot runs its scripts.
    pub read: bool,
    /// Mirrors `config.side_effects_cache_write`. When `true`, a
    /// successful postinstall triggers a re-CAFS of the built package
    /// directory and a queued mutation of the matching
    /// `PackageFilesIndex.sideEffects` row.
    pub write: bool,
    pub publisher: Option<&'a crate::shared_side_effects::SharedSideEffectsPublisher>,
    /// Store-dir handle for the WRITE path's `add_files_from_dir`
    /// call. `None` short-circuits the upload site entirely — used
    /// by unit tests that don't set up a CAFS.
    pub store_dir: Option<&'a pnpm_store_dir::StoreDir>,
    /// Shared batched writer for the side-effects upload's
    /// read-modify-write of the existing `PackageFilesIndex` row.
    /// `None` short-circuits the upload site.
    pub store_index_writer: Option<&'a std::sync::Arc<pnpm_store_dir::StoreIndexWriter>>,

    /// Mirrors `config.frozen_store`. When `true` together with the
    /// global virtual store, a snapshot that would apply a patch or
    /// run an approved lifecycle script is refused with
    /// [`crate::BuildModulesError::FrozenStoreNeedsBuild`] before the write
    /// is attempted — the store is read-only, so the build cannot run.
    /// Has no effect under the isolated linker, whose slot directories
    /// live in the writable project store.
    pub frozen_store: bool,
}

#[derive(Clone, Copy)]
pub struct BuildLayout<'a> {
    /// Install-scoped slot-directory mapping (GVS-aware). The layout
    /// knows the per-snapshot subdirectory shape (legacy flat-name vs
    /// GVS `<scope>/<name>/<version>/<hash>`). See
    /// [`crate::VirtualStoreLayout`].
    pub layout: &'a crate::VirtualStoreLayout,

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
    /// is handled by `PkgRoots`.
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
    pub modules_dir: &'a Path,
    pub lockfile_dir: &'a Path,

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
}

#[derive(Clone, Copy)]
pub struct BuildGraphInputs<'a> {
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
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
    /// Per-snapshot `requiresBuild` values from the warm-cache
    /// prefetch. Missing entries fall back to inspecting the
    /// materialized package directory.
    pub requires_build_by_snapshot: Option<&'a crate::RequiresBuildBySnapshot>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub dependency_groups: Option<&'a [pnpm_package_manifest::DependencyGroup]>,
}

#[derive(Clone, Copy)]
pub struct BuildSnapshotInputs<'a> {
    pub(crate) snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub(crate) packages: Option<&'a HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    pub(crate) patches: Option<&'a HashMap<PackageKey, pnpm_patching::ExtendedPatchInfo>>,
    pub(crate) requires_build_map: &'a HashMap<PackageKey, bool>,
}

#[derive(Clone, Copy)]
pub struct BuildProgress<'a> {
    pub(crate) dep_graph:
        Option<&'a HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>>,
    pub(crate) deps_state_cache: &'a Mutex<pnpm_graph_hasher::DepsStateCache<PackageKey>>,
    pub(crate) ignored_builds: &'a Mutex<BTreeSet<String>>,
    /// Raised before any write that can change a linked slot's contents
    /// (side-effects overlay, patch, lifecycle script) — set pre-attempt, so a
    /// half-applied write still counts. See
    /// [`crate::BuildModulesOutput::mutated_slots`].
    pub(crate) slot_mutations: &'a AtomicBool,
}
