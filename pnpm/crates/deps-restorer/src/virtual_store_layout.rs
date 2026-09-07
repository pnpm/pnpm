//! Per-install computed layout of the virtual store.
//!
//! Stage 1 of pnpm/pacquet#432 introduces a path split: when the global
//! virtual store is enabled, packages live at
//! `<store_dir>/links/<scope>/<name>/<version>/<hash>/node_modules/<name>`,
//! not at the project-local
//! `<project>/node_modules/.pnpm/<flat-name>/node_modules/<name>`. The
//! shape of `<flat-name>` versus `<scope>/<name>/<version>/<hash>` is
//! also different — flat name uses [`PkgNameVerPeer::to_virtual_store_name`]
//! while the GVS layout uses
//! [`pnpm_graph_hasher::format_global_virtual_store_path`] over a
//! `calc_graph_node_hash`-computed digest.
//!
//! [`VirtualStoreLayout`] hides that difference behind one
//! [`slot_dir`] lookup so the install pipeline doesn't have to branch
//! on `Config::enable_global_virtual_store` at every site that
//! computes a per-snapshot path.
//!
//! [`slot_dir`]: VirtualStoreLayout::slot_dir
//! [`PkgNameVerPeer::to_virtual_store_name`]: pnpm_lockfile::PkgNameVerPeer::to_virtual_store_name
//! [`pnpm_graph_hasher::format_global_virtual_store_path`]: pnpm_graph_hasher::format_global_virtual_store_path

use crate::{
    AllowBuildPolicy,
    install_frozen_lockfile::{
        find_own_runtime_node_major, find_runtime_node_major, parse_major_from_version,
    },
};
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_deps_path::get_pkg_id_with_patch_hash;
use pnpm_graph_hasher::{
    DepsGraphNode, DepsStateCache, calc_graph_node_hash, detect_node_major, engine_name,
    format_global_virtual_store_path, join_global_virtual_store_path,
};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgIdWithPatchHash, PkgVerPeer, SnapshotEntry,
    VersionPart,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Precomputed mapping from each snapshot key to the directory where
/// its files live on disk. Built once per install in
/// [`InstallFrozenLockfile::run`](crate::InstallFrozenLockfile::run);
/// passed by reference to every helper that needs to know where a
/// particular snapshot is materialised.
///
/// [`Self::slot_dir`] is the only call site every consumer has to
/// touch — it returns an absolute directory whose `node_modules/<name>`
/// subdirectory holds the unpacked package.
pub struct VirtualStoreLayout {
    /// Root containing every per-snapshot subdirectory. Picked from
    /// `Config::global_virtual_store_dir` when GVS is enabled (the
    /// shared `<store_dir>/links` path, or the user's pinned override)
    /// and from `Config::virtual_store_dir` when GVS is disabled (the
    /// project-local `<modules_dir>/.pnpm`). Pacquet keeps the two
    /// fields separate so the legacy non-frozen install path can keep reading
    /// `virtual_store_dir` directly via [`Self::legacy`] without the
    /// frozen-lockfile derivation redirecting it. See
    /// [`Config::apply_global_virtual_store_derivation`] for the
    /// reasoning behind the field split.
    ///
    /// Stored separately from a `&Config` so callers don't have to
    /// thread the full config through the helpers that only need a
    /// path lookup.
    package_store_dir: PathBuf,

    /// `Some` only when the global virtual store is enabled. For each
    /// snapshot, holds the precomputed
    /// `[<scope>/]<name>/<version>/<hash>` suffix that goes after
    /// `package_store_dir`. `None` when GVS is off — callers fall back
    /// to [`PkgNameVerPeer::to_virtual_store_name`] computed on demand
    /// from the snapshot key.
    ///
    /// [`PkgNameVerPeer::to_virtual_store_name`]: pnpm_lockfile::PkgNameVerPeer::to_virtual_store_name
    gvs_suffixes: Option<HashMap<PackageKey, String>>,

    /// Threshold passed into
    /// [`PkgNameVerPeer::to_virtual_store_name`] for the legacy flat-
    /// name fallback. Mirrors pnpm's `virtualStoreDirMaxLength`: when
    /// the escaped filename exceeds this many bytes, the tail is
    /// replaced with a 32-char sha256 hash so the directory name fits
    /// within filesystem limits (macOS / ext4 cap component names at
    /// 255 bytes, but pnpm defaults to 60 on Windows and 120 elsewhere
    /// to leave headroom for the `<name>@<version>/` suffix appended
    /// below).
    ///
    /// [`PkgNameVerPeer::to_virtual_store_name`]: pnpm_lockfile::PkgNameVerPeer::to_virtual_store_name
    virtual_store_dir_max_length: usize,

    /// Directory the lockfile's relative paths resolve against.
    /// [`crate::create_symlink_layout()`] needs it to point a `link:`
    /// dependency's symlink at the directory the lockfile names, which
    /// it records relative to this root. `None` when the caller has no
    /// lockfile context, in which case `link:` dependencies inside a
    /// slot are left unlinked exactly as before.
    lockfile_dir: Option<PathBuf>,
}

impl VirtualStoreLayout {
    /// Construct a layout that always uses the legacy
    /// `<root>/<flat-name>` shape, regardless of any
    /// `enable_global_virtual_store` setting on `Config`. Reserved
    /// for callers that must stay on the project-local flat layout
    /// even under GVS — today no production install path uses this
    /// directly. Both `InstallFrozenLockfile` and
    /// `InstallWithoutLockfile` construct via [`Self::new`] so they
    /// honor `Config::enable_global_virtual_store` consistently.
    pub fn legacy(root: impl Into<PathBuf>, virtual_store_dir_max_length: usize) -> Self {
        VirtualStoreLayout {
            package_store_dir: root.into(),
            gvs_suffixes: None,
            virtual_store_dir_max_length,
            lockfile_dir: None,
        }
    }

    /// Directory the lockfile's relative paths resolve against, when
    /// the caller supplied one to [`Self::new`].
    #[must_use]
    pub fn lockfile_dir(&self) -> Option<&Path> {
        self.lockfile_dir.as_deref()
    }

    /// Attach a lockfile directory to a layout built by
    /// [`Self::legacy`], which has no lockfile context of its own.
    #[cfg(test)]
    pub(crate) fn with_lockfile_dir(mut self, lockfile_dir: impl Into<PathBuf>) -> Self {
        self.lockfile_dir = Some(lockfile_dir.into());
        self
    }

    /// Build the layout for one install. Reads
    /// [`Config::enable_global_virtual_store`] to decide whether to
    /// precompute GVS slot names, then iterates the lockfile's
    /// `snapshots` (the per-peer-context entries) and computes each
    /// snapshot's [`format_global_virtual_store_path`]-shaped suffix
    /// via [`calc_graph_node_hash`].
    ///
    /// Returns a layout that's safe to pass by reference across rayon
    /// workers: every field is `Send + Sync` once constructed (the
    /// internal `HashMap<PackageKey, String>` doesn't mutate after
    /// `new`).
    ///
    /// `engine` is the install-wide fallback `ENGINE_NAME`-style
    /// string that [`pnpm_graph_hasher::engine_name`] produces;
    /// threaded in instead of recomputed inside so the value matches
    /// whatever the rest of the install (notably the side-effects
    /// cache key) uses. Snapshots that themselves pin Node via
    /// `engines.runtime` (carried in the lockfile as
    /// `dependencies.node: runtime:<version>`) override the fallback
    /// per-snapshot through `find_own_runtime_node_major` — the
    /// engine portion of the hash then tracks the Node that the
    /// bin linker would spawn for that pinning package's lifecycle
    /// scripts. The per-snapshot runtime pin takes precedence over
    /// the install-wide fallback.
    ///
    /// `None` propagates straight into
    /// [`calc_graph_node_hash`]'s `engine` parameter — `None` and
    /// `Some("")` produce *different* GVS hashes (the former omits
    /// the `engine` contribution, the latter hashes the empty string),
    /// so the call site must keep the `Option` shape rather than
    /// flattening to `unwrap_or("")`.
    ///
    /// `snapshots` / `packages` are the lockfile fields the caller
    /// already has by the time the install dispatches to a frozen-
    /// lockfile flow — see
    /// [`crate::InstallFrozenLockfile::run`].
    ///
    /// `allow_build_policy` drives engine-agnostic gating. When
    /// `Some`, the constructor walks `snapshots` once to collect
    /// every key whose `(name, version)` passes
    /// [`AllowBuildPolicy::check`] returning `Some(true)`, then expands
    /// that set to every transitive parent. Pure-JS subgraphs hash with
    /// `engine = null` so their GVS directories survive Node.js
    /// upgrades. When `None`, every snapshot keeps the engine in
    /// its hash payload.
    ///
    /// `lockfile_dir` scopes the slots of snapshots resolved from a
    /// local directory to the project that owns them: a directory
    /// resolution is a lockfile-relative path with no integrity, so
    /// `file:dep` otherwise hashes identically in every project that
    /// depends on a directory of that name, and they all link to
    /// whichever project installed first. `None` leaves them on that
    /// shared slot, which is only safe for a lockfile known to have no
    /// directory resolutions.
    pub fn new(
        config: &Config,
        engine: Option<&str>,
        snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        allow_build_policy: Option<&AllowBuildPolicy>,
        lockfile_dir: Option<&Path>,
    ) -> Self {
        // Pacquet keeps `virtual_store_dir` and `global_virtual_store_dir`
        // as two separate fields (see
        // [`Config::apply_global_virtual_store_derivation`] for why).
        // The frozen-lockfile install picks
        // `global_virtual_store_dir` here when GVS is on so the
        // without-lockfile path can stay on the project-local
        // `virtual_store_dir` without colliding.
        let package_store_dir = if config.enable_global_virtual_store {
            config.global_virtual_store_dir.clone()
        } else {
            config.virtual_store_dir.clone()
        };
        let virtual_store_dir_max_length = config.virtual_store_dir_max_length as usize;
        if !config.enable_global_virtual_store {
            return VirtualStoreLayout {
                package_store_dir,
                gvs_suffixes: None,
                virtual_store_dir_max_length,
                lockfile_dir: lockfile_dir.map(Path::to_path_buf),
            };
        }
        Self::global(
            package_store_dir,
            virtual_store_dir_max_length,
            engine,
            snapshots,
            packages,
            allow_build_policy,
            lockfile_dir,
        )
    }

    /// Build a GVS-shaped layout rooted at `package_store_dir`,
    /// regardless of `Config::enable_global_virtual_store`. This is the
    /// body of [`Self::new`]'s GVS branch; the macOS directory-clone
    /// materialization cache
    /// ([`crate::DirCloneCache`](crate::dir_clone_cache::DirCloneCache))
    /// also constructs through it so its canonical slots land on
    /// exactly the paths a GVS-enabled install would use, letting the
    /// two modes share one set of materialized packages under
    /// `<store_dir>/links`.
    pub fn global(
        package_store_dir: PathBuf,
        virtual_store_dir_max_length: usize,
        engine: Option<&str>,
        snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        allow_build_policy: Option<&AllowBuildPolicy>,
        lockfile_dir: Option<&Path>,
    ) -> Self {
        let Some(snapshots) = snapshots else {
            return VirtualStoreLayout {
                package_store_dir,
                gvs_suffixes: Some(HashMap::new()),
                virtual_store_dir_max_length,
                lockfile_dir: lockfile_dir.map(Path::to_path_buf),
            };
        };
        let graph = lockfile_to_dep_graph(snapshots, packages, lockfile_dir);
        // One conversion for the whole lockfile: the same string scopes every
        // local directory snapshot in it.
        //
        // Lossy on purpose: the TypeScript CLI hashes the same slot from a JS
        // string, and Node decodes a path as UTF-8 with replacement, so this is
        // the identical input. Hashing the raw bytes instead would give the two
        // stacks different slots for the same project.
        let project_scope = lockfile_dir.map(|dir| dir.to_string_lossy());
        // Build the engine-agnostic gating set once per install.
        // `None` here disables gating so every snapshot still hashes
        // with its engine string.
        let build_required_dep_paths: Option<HashSet<String>> = allow_build_policy.map(|policy| {
            let built_dep_paths = snapshots
                .keys()
                .filter(|key| policy.check(&key.without_peer().to_string()) == Some(true))
                .map(ToString::to_string)
                .collect();
            pnpm_graph_hasher::build_required_dep_paths(&graph, &built_dep_paths)
        });
        let mut cache: DepsStateCache<String> = HashMap::new();
        let mut gvs_suffixes: HashMap<PackageKey, String> = HashMap::with_capacity(snapshots.len());
        // Lockfile key order, not `HashMap` order: `calc_graph_node_hash`
        // memoizes into `cache`, and for a snapshot inside a dependency
        // cycle the digest that lands there depends on which snapshot the
        // walk reached it from.
        for (snapshot_key, snapshot) in crate::deps_graph::in_lockfile_order(snapshots) {
            // Per-snapshot engine resolution: a snapshot that declares
            // its own `engines.runtime` carries the desugared
            // `dependencies.node: 'runtime:<version>'` pin, which has
            // to drive the engine portion of *its* hash rather than
            // the install-wide fallback. Precedence: own pin first,
            // install-wide fallback second. Default host platform /
            // arch (`None`, `None`) matches whatever the caller used
            // to format the fallback `engine` so the two strings
            // remain comparable across snapshots in one install.
            let own_engine =
                find_own_runtime_node_major(snapshot).map(|major| engine_name(major, None, None));
            let snapshot_engine = own_engine.as_deref().or(engine);
            let metadata_key = snapshot_key.without_peer();
            let metadata = packages.and_then(|map| map.get(&metadata_key));
            let project =
                local_directory_scope(metadata, &metadata_key.suffix, project_scope.as_deref());
            let graph_key = snapshot_key.to_string();
            let hex_digest = calc_graph_node_hash(
                &graph,
                &mut cache,
                &graph_key,
                snapshot_engine,
                build_required_dep_paths.as_ref(),
                project,
            );
            let name = metadata_key.name.to_string();
            let version = gvs_version_segment(metadata, &metadata_key.suffix);
            let suffix = format_global_virtual_store_path(&name, &version, &hex_digest);
            gvs_suffixes.insert(snapshot_key.clone(), suffix);
        }
        VirtualStoreLayout {
            package_store_dir,
            gvs_suffixes: Some(gvs_suffixes),
            virtual_store_dir_max_length,
            lockfile_dir: lockfile_dir.map(Path::to_path_buf),
        }
    }

    /// Root of the layout — the directory that contains every per-
    /// snapshot subdirectory. Exposed so callers that need to pass a
    /// path to existing helpers (e.g. the
    /// [`pnpm_modules_yaml::Modules`] writer, which still records
    /// the legacy [`Config::virtual_store_dir`] string) have one
    /// source of truth.
    #[must_use]
    pub fn package_store_dir(&self) -> &Path {
        &self.package_store_dir
    }

    /// Whether this install is running in global-virtual-store mode.
    /// Mirrors `config.enable_global_virtual_store` — captured here so
    /// callers can ask the layout itself instead of having to keep a
    /// separate `&Config` reference for the boolean.
    #[must_use]
    pub fn enable_global_virtual_store(&self) -> bool {
        self.gvs_suffixes.is_some()
    }

    /// Absolute directory that holds `node_modules/<name>` for one
    /// snapshot. Falls back to
    /// [`PkgNameVerPeer::to_virtual_store_name`](pnpm_lockfile::PkgNameVerPeer::to_virtual_store_name)
    /// when GVS is off, or when GVS is on but the key isn't in the
    /// precomputed map (which would indicate a bug — every snapshot
    /// the install touches must have been visited in
    /// [`Self::new`]; the fallback is defensive rather than expected
    /// to fire).
    /// Like [`Self::slot_dir`], but only for a snapshot with a
    /// precomputed GVS suffix — `None` instead of the flat-name
    /// fallback. The directory-clone cache requires this: a GVS suffix
    /// is content-addressed through the graph hash's
    /// `full_pkg_id = <pkg_id>:<integrity>` input, while the flat name
    /// is keyed by the snapshot key alone, so a flat-named canonical
    /// slot would keep serving stale content after the same version is
    /// re-published with different integrity.
    #[must_use]
    pub fn hashed_slot_dir(&self, key: &PackageKey) -> Option<PathBuf> {
        let suffix = self.gvs_suffixes.as_ref()?.get(key)?;
        Some(join_global_virtual_store_path(&self.package_store_dir, suffix))
    }

    #[must_use]
    pub fn slot_dir(&self, key: &PackageKey) -> PathBuf {
        let suffix = match &self.gvs_suffixes {
            Some(map) => map
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.to_virtual_store_name(self.virtual_store_dir_max_length)),
            None => key.to_virtual_store_name(self.virtual_store_dir_max_length),
        };
        // The flat non-GVS `to_virtual_store_name` fallback carries no
        // `/`, so it too can route through the GVS join (which then just
        // pushes it as a single component).
        join_global_virtual_store_path(&self.package_store_dir, &suffix)
    }
}

/// Build a lockfile's layout using the runtime pin, effective Node version, then host.
#[must_use]
pub fn virtual_store_layout_for_lockfile(
    config: &Config,
    effective_node_version: Option<&str>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    allow_build_policy: Option<&AllowBuildPolicy>,
    lockfile_dir: Option<&Path>,
) -> VirtualStoreLayout {
    let engine = if config.enable_global_virtual_store {
        find_runtime_node_major(snapshots)
            .or_else(|| effective_node_version.and_then(parse_major_from_version))
            .or_else(detect_node_major)
            .map(|major| engine_name(major, None, None))
    } else {
        None
    };
    VirtualStoreLayout::new(
        config,
        engine.as_deref(),
        snapshots,
        packages,
        allow_build_policy,
        lockfile_dir,
    )
}

/// Return the GVS directory containing every graph-hash slot for one snapshot.
/// This derives only the `<scope>/<name>/<version>` prefix and does not build or
/// hash the dependency graph. Returns `None` when lockfile data cannot form the
/// exact package-name and version components in that prefix.
#[must_use]
pub fn global_virtual_store_version_dir(
    package_store_dir: &Path,
    snapshot_key: &PackageKey,
    metadata: Option<&PackageMetadata>,
) -> Option<PathBuf> {
    let name = snapshot_key.name.to_string();
    let version = gvs_version_segment(metadata, &snapshot_key.suffix);
    if !pnpm_resolving_deps_resolver::is_valid_dependency_alias(&name)
        || !is_single_gvs_path_component(&version)
    {
        return None;
    }
    let candidate_slot = join_global_virtual_store_path(
        package_store_dir,
        &format_global_virtual_store_path(&name, &version, "candidate"),
    );
    if !pnpm_fs::is_subdir(package_store_dir, &candidate_slot) {
        return None;
    }
    candidate_slot.parent().map(Path::to_path_buf)
}

/// Map each injected `file:` project to the virtual-store package
/// directories its copies were materialized at.
///
/// Key: the `file:` path from the snapshot key — lockfile-relative,
/// which for an injected workspace project is exactly its importer id
/// (`comp2`, `packages/foo`, ...). Value: every peer-variant slot's
/// package directory (`node_modules/.pnpm/<slot>/node_modules/<name>`),
/// relative to `lockfile_dir` when the slot lives under it (a
/// global-virtual-store slot outside the project stays absolute).
///
/// This is the `.modules.yaml` `injectedDeps` payload pnpm v11 wrote
/// from `projectsWithTargetDirs`. Consumers use it to propagate
/// post-install changes of an injected project into every materialized
/// copy — Bit's build task, for one, hard-links compiled artifacts
/// into each copy via this map and silently links nothing when the
/// field is missing.
///
/// Skipped snapshots have no slot on disk, so they are left out. Only
/// **directory**-resolution `file:` snapshots participate: v11's map
/// came from `projectsWithTargetDirs` (injected workspace projects),
/// so a `file:` *tarball* dep — whose extracted copy is not a mirror
/// of any source directory — is excluded to keep the field's meaning
/// identical.
///
/// Under `nodeLinker: hoisted` there is no virtual store — the
/// injected copies live wherever the hoisted walker placed them.
/// Pass the walker's `hoisted_locations` (depPath → lockfile-relative
/// package dirs) as `hoisted_locations` and the targets are read from
/// it instead of the slot layout.
#[must_use]
pub fn collect_injected_deps(
    layout: &VirtualStoreLayout,
    lockfile_dir: &Path,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    skipped: &crate::SkippedSnapshots,
    hoisted_locations: Option<&std::collections::BTreeMap<String, Vec<String>>>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut injected: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let Some(snapshots) = snapshots else { return injected };
    for key in snapshots.keys() {
        let VersionPart::File(path) = key.suffix.version() else { continue };
        if skipped.contains(key) {
            continue;
        }
        // `packages:` keys are peer-stripped; require a directory
        // resolution (an injected project copy, not a file: tarball).
        let is_directory = packages
            .and_then(|packages| packages.get(&key.without_peer()))
            .is_some_and(|meta| matches!(meta.resolution, LockfileResolution::Directory(_)));
        if !is_directory {
            continue;
        }
        let source = path.strip_prefix("./").unwrap_or(path);
        let targets = injected.entry(source.to_string()).or_default();
        if let Some(locations) = hoisted_locations {
            // Hoisted linker: the walker already recorded every
            // lockfile-relative dir this depPath was placed at.
            if let Some(dirs) = locations.get(&key.to_string()) {
                targets.extend(dirs.iter().cloned());
            }
        } else {
            // Isolated linker: one virtual-store slot per snapshot.
            let target = layout.slot_dir(key).join("node_modules").join(key.name.to_string());
            let target = match target.strip_prefix(lockfile_dir) {
                Ok(relative) => relative.to_path_buf(),
                Err(_) => target,
            };
            // POSIX separators on every platform, matching the
            // `hoistedLocations` entries the hoisted branch reuses
            // (see `path_relative_to_lockfile_dir`).
            targets.push(target.to_string_lossy().replace('\\', "/"));
        }
    }
    // A source project whose every snapshot contributed no target
    // (e.g. hoisted entries the walker never placed) would round-trip
    // as an empty list; drop those.
    injected.retain(|_, targets| !targets.is_empty());
    // Sort each target list — `snapshots` is a `HashMap`, so insertion
    // order is nondeterministic and the manifest must be byte-stable
    // across installs.
    for targets in injected.values_mut() {
        targets.sort_unstable();
    }
    injected
}

/// Version segment of a snapshot's global-virtual-store path. Derives
/// the version as `pkgSnapshot.version ?? pkgInfo.version`, then feeds
/// it into the global-virtual-store path format.
///
/// A snapshot resolved from a local directory gets the fixed
/// [`LOCAL_DIRECTORY_SEGMENT`] instead: the lockfile records no version
/// for one, and the install path that resolves from manifests does know
/// the version, so anchoring the segment is what keeps a re-install on
/// the slot the first install created. Emitting the raw `file:<path>`
/// here would put a `:` (and embedded `/`) into the slot path —
/// rejected on Windows with `ERROR_INVALID_NAME`.
fn gvs_version_segment(metadata: Option<&PackageMetadata>, suffix: &PkgVerPeer) -> String {
    if is_local_directory(metadata, suffix) {
        return LOCAL_DIRECTORY_SEGMENT.to_string();
    }
    match metadata.and_then(|meta| meta.version.as_deref()) {
        Some(version) => version.to_string(),
        None => suffix.version().to_string(),
    }
}

fn is_single_gvs_path_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    !value.contains(['/', '\\'])
        && matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

/// Stands in for the version of a snapshot resolved from a local
/// directory — see [`gvs_version_segment`].
const LOCAL_DIRECTORY_SEGMENT: &str = "directory";

/// Whether the snapshot is a package taken from a local directory — a
/// `file:` directory dependency or an injected workspace package.
///
/// Both the version segment and the project scope below are derived
/// from this one answer. Deciding it twice is what lets a snapshot take
/// the anchored segment while missing the scope, which is the collision
/// the scope exists to prevent.
///
/// Without a `packages:` entry the resolution is unavailable and the
/// snapshot key's `file:` version part is the only signal left. Reading
/// it as a directory is the safe way to be wrong: the slot stays scoped,
/// and the segment stays a legal path component (the raw `file:<path>`
/// carries a `:` and a `/`, which Windows rejects with
/// `ERROR_INVALID_NAME`).
fn is_local_directory(metadata: Option<&PackageMetadata>, suffix: &PkgVerPeer) -> bool {
    if let Some(metadata) = metadata {
        if matches!(metadata.resolution, LockfileResolution::Directory(_)) {
            return true;
        }
        if metadata.version.is_some() {
            return false;
        }
    }
    matches!(suffix.version(), VersionPart::File(_))
}

/// Extra hash input that keeps a snapshot taken from a local directory
/// on a slot of its own. `None` for every other snapshot, which then
/// hashes exactly as it did before.
///
/// A directory resolution is the one resolution with no integrity: it
/// is a path relative to the lockfile, so `file:dep` hashes identically
/// in every project that happens to depend on a directory of that name.
/// Sharing the slot would hand one project the files of whichever
/// project installed first, and because the source directory is mutable
/// the install re-imports it every time — so the projects would go on
/// overwriting each other's dependency.
fn local_directory_scope<'a>(
    metadata: Option<&PackageMetadata>,
    suffix: &PkgVerPeer,
    lockfile_dir: Option<&'a str>,
) -> Option<&'a str> {
    is_local_directory(metadata, suffix).then_some(lockfile_dir).flatten()
}

/// Build the dependency graph from the lockfile's `snapshots` /
/// `packages` sections. Every entry in `snapshots` becomes a node whose
/// `full_pkg_id` is `<pkg_id_with_patch_hash>:<integrity>` (for tarball
/// / registry resolutions) and whose `children` are the
/// alias→snapshot-key edges pulled from the snapshot's combined
/// `dependencies` + `optionalDependencies`.
///
/// Resolved `link:` targets become leaf nodes whose identity is the
/// absolute target path. Modeling them as children makes the target
/// participate in every ancestor's recursive hash while keeping slots
/// shared between projects that resolve the link to the same directory.
///
/// Packages whose metadata is missing or whose resolution has no
/// `integrity` (directory / git) are emitted with the bare
/// `pkg_id_with_patch_hash` as their `full_pkg_id`. The frozen-
/// lockfile install path rejects those resolutions before reaching the
/// linker, so a stub `full_pkg_id` here is safe — the GVS hash for an
/// install that contains one of those snapshots is irrelevant because
/// the install will error out before consulting it.
fn lockfile_to_dep_graph(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    lockfile_dir: Option<&Path>,
) -> HashMap<String, DepsGraphNode<String>> {
    let mut graph = HashMap::with_capacity(snapshots.len());
    let mut link_target_nodes = HashSet::new();
    for (snapshot_key, snapshot) in snapshots {
        let children = crate::deps_graph::build_children_with(snapshot, |alias, dep_ref| {
            if let Some(snapshot_key) = dep_ref.resolve(alias) {
                return Some(snapshot_key.to_string());
            }
            let link_target = dep_ref.as_link_target()?;
            let lockfile_dir = lockfile_dir?;
            let resolved = pnpm_fs::lexical_normalize(&lockfile_dir.join(link_target));
            Some(format!("link:{}", resolved.to_string_lossy()))
        });
        link_target_nodes
            .extend(children.values().filter(|child_key| child_key.starts_with("link:")).cloned());
        let metadata_key = snapshot_key.without_peer();
        let pkg_id_with_patch_hash = PkgIdWithPatchHash::from(
            get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string(),
        );
        let resolution =
            packages.and_then(|map| map.get(&metadata_key)).map(|meta| &meta.resolution);
        let full_pkg_id = create_full_pkg_id(&pkg_id_with_patch_hash, resolution);
        graph.insert(snapshot_key.to_string(), DepsGraphNode { full_pkg_id, children });
    }
    for link_target_node in link_target_nodes {
        graph.insert(
            link_target_node.clone(),
            DepsGraphNode { full_pkg_id: link_target_node, children: IndexMap::default() },
        );
    }
    graph
}

/// `variations` (cross-platform variant) resolutions don't exist in
/// pacquet's lockfile model yet — when they're added, this helper
/// will need a `selectPlatformVariant` branch to pick the right
/// integrity.
fn create_full_pkg_id(
    pkg_id_with_patch_hash: &PkgIdWithPatchHash,
    resolution: Option<&LockfileResolution>,
) -> String {
    match resolution.and_then(LockfileResolution::integrity) {
        Some(integrity) => format!("{pkg_id_with_patch_hash}:{integrity}"),
        // Directory / git / missing-metadata fall through to the bare
        // id. The install path rejects these resolutions before the
        // hash is consulted (see
        // [`crate::InstallPackageBySnapshotError::UnsupportedResolution`]),
        // so the value never actually drives a slot path on disk.
        None => pkg_id_with_patch_hash.to_string(),
    }
}

#[cfg(test)]
mod tests;
