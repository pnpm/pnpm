use crate::{
    AllowBuildPolicy,
    CasPathsByPkgId,
    PackageManifests,
    VirtualStoreLayout,
};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::Config;
use pnpm_lockfile::{
    Lockfile,
    PackageKey,
    PackageMetadata,
    ProjectSnapshot,
    SnapshotEntry,
};
use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
use pnpm_store_dir::StoreIndexWriter;
use std::{
    collections::HashMap,
    path::{
        Path,
        PathBuf,
    },
    sync::{
        Arc,
        atomic::AtomicU8,
    },
};

#[derive(Clone, Copy)]
pub struct LinkProjects<'a> {
    pub manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub package_map_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub dependency_groups: &'a [DependencyGroup],
    /// Anchor for each importer's `node_modules`. The frozen path uses
    /// `workspace_root`; the fresh path uses `modules_dir.parent()`,
    /// because its tests relocate `modules_dir` away from the manifest.
    pub symlink_root: &'a Path,
    /// Importer ids allowed to live outside the lockfile dir (Bit's
    /// capsule installs).
    pub trusted_importer_ids: &'a std::collections::HashSet<String>,
    /// Importers declaring `installConfig.hoistingLimits: "workspaces"`.
    pub root_component_importers: &'a std::collections::HashSet<String>,
}

#[derive(Clone, Copy)]
pub struct LinkLockfiles<'a> {
    /// The lockfile this phase links. Its `importers`, `packages` and
    /// `snapshots` are read straight off it, so a caller cannot pair one
    /// lockfile's entries with another's maps.
    pub lockfile: &'a Lockfile,
    pub current_lockfile: Option<&'a Lockfile>,
    /// Restricts per-slot bin linking to this install's materialized
    /// snapshots. `None` keeps rebuild's all-slot behavior.
    pub materialized_snapshots: Option<&'a [PackageKey]>,
    /// The lockfile the module-resolution sidecars describe. The frozen
    /// path filters to the current install first; the fresh path already
    /// holds a materialization closure.
    pub sidecar_lockfile: &'a Lockfile,
}

pub struct LinkPackageData<'a> {
    pub package_manifests: &'a PackageManifests,
    /// Per-snapshot `requiresBuild` flags from the store-index prefetch,
    /// gating the importer bin pass's use of `package_manifests` — see
    /// [`crate::link_direct_dep_bins_prefetched`]. `None` when the
    /// caller has no prefetch (the fresh-lockfile installer).
    pub requires_build_by_snapshot: Option<&'a crate::RequiresBuildBySnapshot>,
    pub cas_paths_by_pkg_id: Option<CasPathsByPkgId>,
}

#[derive(Clone, Copy)]
pub struct PriorLinkState<'a> {
    pub prune_orphans: bool,
    pub hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// `hoistedLocations` recorded by the previous install's
    /// `.modules.yaml`; lets the hoisted linker leave packages that are
    /// already in place alone. `None` on a first install.
    pub hoisted_locations: Option<&'a crate::HoistedLocations>,
    /// See [`crate::PriorHoistedState::build_present_packages`].
    pub build_present_packages: bool,
    /// See [`crate::PriorHoistedState::unbuilt_builds`].
    pub unbuilt_builds: &'a crate::UnbuiltBuilds,
    /// The snapshots the previous install's `.modules.yaml` recorded as
    /// skipped. A lockfile entry says what an install resolved rather than
    /// what it put on disk, so telling the two apart for the previous
    /// install takes this set (pnpm/pnpm#15161).
    pub previously_skipped: &'a crate::SkippedSnapshots,
}

impl<'a> PriorLinkState<'a> {
    #[must_use]
    pub fn hoisted_state(self, current_lockfile: Option<&'a Lockfile>) -> PriorHoistedState<'a> {
        PriorHoistedState {
            current_lockfile,
            current_hoisted_locations: self.hoisted_locations,
            previously_skipped: self.previously_skipped,
            unbuilt_builds: self.unbuilt_builds,
            build_present_packages: self.build_present_packages,
        }
    }
}

#[derive(Clone, Copy)]
pub struct HoistedProjects<'a> {
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub dependency_groups: &'a [DependencyGroup],
    /// Selected project anchors whose direct dependencies and workspace
    /// links are written by this filtered run.
    pub manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    /// Every real importer manifest represented in the full hoisted graph.
    /// The shared package map needs all project names for self-reference
    /// entries even though direct links are limited to selected anchors.
    pub package_map_manifests: &'a [(PathBuf, &'a pnpm_package_manifest::PackageManifest)],
    /// Lockfile root the walker resolves hoisted directories against.
    pub walker_lockfile_dir: &'a Path,
    /// Anchor for [`crate::SymlinkDirectDependencies`]'s per-importer
    /// `node_modules` lookup. Equals `walker_lockfile_dir` on the
    /// frozen path; the fresh path passes `config.modules_dir.parent()`
    /// so relocated `modules_dir` test configs land symlinks where the
    /// rest of the install writes.
    pub symlink_workspace_root: &'a Path,
}

#[derive(Clone, Copy)]
pub struct PriorHoistedState<'a> {
    /// Previous install's `<virtual_store_dir>/lock.yaml`. The walker
    /// diffs orphans against it and compares the resolution it records
    /// for a directory against the wanted one. Both install paths pass
    /// it; `None` when the file is absent, which is a first install.
    pub current_lockfile: Option<&'a Lockfile>,
    /// `hoistedLocations` from the previous install's `.modules.yaml`,
    /// so the walker can mark packages that are already on disk. `None`
    /// on a first install.
    pub current_hoisted_locations: Option<&'a crate::HoistedLocations>,
    /// See [`crate::PriorLinkState::previously_skipped`].
    pub previously_skipped: &'a crate::SkippedSnapshots,
    /// Packages the previous install's `.modules.yaml` recorded as not
    /// built. A present one among them still reaches the build phase.
    pub unbuilt_builds: &'a crate::UnbuiltBuilds,
    /// `true` when every package's directory must reach the build
    /// phase, present or not: the user asked for a rebuild
    /// (`pnpm rebuild`, `approve-builds`), or `allowBuilds` changed since
    /// the previous install, so a build it ignored may now run, or one it
    /// ran must be judged again. Otherwise a present package is not
    /// rebuilt, as pnpm marks an unfetched node `isBuilt`.
    pub build_present_packages: bool,
}

pub struct HoistedLinkGraph<'a> {
    /// Lockfile the walker reads `snapshots:` / `packages:` /
    /// `importers:` from. `&built_lockfile` on the fresh path,
    /// the loaded wanted lockfile on the frozen path.
    pub lockfile: &'a Lockfile,
    pub layout: &'a VirtualStoreLayout,
    /// Per-package CAS index produced by [`crate::CreateVirtualStore`]
    /// under `node_linker == Hoisted`. The linker imports files from
    /// these paths into the on-disk hoisted tree.
    pub cas_paths_by_pkg_id: Option<crate::CasPathsByPkgId>,
}

#[derive(Clone, Copy)]
pub struct BuildPhaseGraph<'a> {
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub dependency_groups: &'a [DependencyGroup],
    /// Snapshot keys materialized by this install. Under
    /// `ignoreScripts`, only these can add new `pendingBuilds`; the
    /// install orchestrator separately carries forward existing entries.
    pub materialized_snapshots: &'a [PackageKey],
}

#[derive(Clone, Copy)]
pub struct BuildPhasePolicy<'a> {
    pub config: &'static Config,
    /// `patchedDependencies` already resolved + grouped by the caller, so
    /// the build phase doesn't re-hash the patch files. `None` on the
    /// frozen path, which resolves it inside [`crate::resolve_snapshot_patches`].
    pub patch_groups: Option<&'a pnpm_patching::PatchGroupRecord>,
    pub allow_build_policy: &'a AllowBuildPolicy,
    /// Forced-rebuild selection threaded from `pacquet rebuild` /
    /// `approve-builds`; `None` for a normal install. See
    /// [`crate::RebuildOptions`].
    pub rebuild: Option<&'a crate::RebuildOptions>,
}

#[derive(Clone, Copy)]
pub struct BuildPhaseCache<'a> {
    pub maps_by_snapshot: &'a crate::SideEffectsMapsBySnapshot,
    pub requires_build_by_snapshot: &'a crate::RequiresBuildBySnapshot,
    pub engine_name: Option<&'a str>,
    pub store_index_writer: &'a Arc<StoreIndexWriter>,
}

#[derive(Clone, Copy)]
pub struct BuildPhaseDirectories<'a> {
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
    pub hoisted_pkg_roots_by_key: Option<&'a HashMap<PackageKey, Vec<PathBuf>>>,
    pub is_hoisted: bool,
    /// Publicly-hoisted aliases (with bins) competing for the root
    /// importer's `node_modules/.bin`. Empty under the hoisted linker
    /// and when no public-hoist pattern is set.
    pub publicly_hoisted_for_post_build: &'a [String],
    pub logged_methods: &'a AtomicU8,
    /// [`crate::shim_link_options`] output, for the post-build
    /// top-level bin pass.
    pub link_options: &'a LinkBinsOptions,
}
