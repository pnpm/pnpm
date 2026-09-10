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

    /// [`Self::new`], with the derived suffix map cached on disk.
    ///
    /// The key is a digest of the inputs the suffixes are derived from,
    /// taken from the values in hand rather than from a re-read of the
    /// lockfile file — the caller parsed that file at some earlier
    /// point, and a second read can return a different revision, which
    /// would file this run's suffixes under another one's identity.
    ///
    /// Only the restore path uses it. Nothing here depends on that any
    /// more, but a caller whose lockfile the install is about to
    /// rewrite gains nothing from an entry it will immediately
    /// invalidate.
    ///
    /// The cache module below documents what the key covers and what
    /// the loader refuses to trust.
    #[must_use]
    pub fn new_cached(
        config: &Config,
        engine: Option<&str>,
        snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        allow_build_policy: Option<&AllowBuildPolicy>,
        lockfile_dir: Option<&Path>,
    ) -> Self {
        let Some(snapshots) = snapshots.filter(|_| config.enable_global_virtual_store) else {
            return Self::new(
                config,
                engine,
                snapshots,
                packages,
                allow_build_policy,
                lockfile_dir,
            );
        };
        let mut hasher =
            GvsHasher::new(snapshots, packages, engine, allow_build_policy, lockfile_dir);
        let fingerprint = hasher.fingerprint(snapshots);
        let cache_file = lockfile_dir.map(|lockfile_dir| gvs_layout_cache::CacheFile {
            cache_dir: &config.cache_dir,
            lockfile_dir,
            fingerprint: &fingerprint,
        });
        if let Some(cache_file) = cache_file
            && let Some(gvs_suffixes) = gvs_layout_cache::load(
                cache_file,
                gvs_layout_cache::Expected { snapshots, packages },
            )
        {
            tracing::info!(
                target: "pacquet::install::phase",
                phase = "gvs.layout_cache_hit",
                entries = gvs_suffixes.len(),
                "phase complete",
            );
            return Self::with_cached_suffixes(config, gvs_suffixes, lockfile_dir);
        }
        let gvs_suffixes = hasher.suffixes(snapshots);
        if let Some(cache_file) = cache_file {
            gvs_layout_cache::store(cache_file, &gvs_suffixes);
        }
        Self::with_cached_suffixes(config, gvs_suffixes, lockfile_dir)
    }

    fn with_cached_suffixes(
        config: &Config,
        gvs_suffixes: HashMap<PackageKey, String>,
        lockfile_dir: Option<&Path>,
    ) -> Self {
        VirtualStoreLayout {
            package_store_dir: config.global_virtual_store_dir.clone(),
            gvs_suffixes: Some(gvs_suffixes),
            virtual_store_dir_max_length: config.virtual_store_dir_max_length as usize,
            lockfile_dir: lockfile_dir.map(Path::to_path_buf),
        }
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
        let mut hasher =
            GvsHasher::new(snapshots, packages, engine, allow_build_policy, lockfile_dir);
        VirtualStoreLayout {
            package_store_dir,
            gvs_suffixes: Some(hasher.suffixes(snapshots)),
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
    entries: pnpm_lockfile::LockfileEntries<'_>,
    skipped: &crate::SkippedSnapshots,
    hoisted_locations: Option<&std::collections::BTreeMap<String, Vec<String>>>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let pnpm_lockfile::LockfileEntries { packages, snapshots } = entries;
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
        injected.entry(source.to_string()).or_default().extend(injected_targets(
            layout,
            lockfile_dir,
            key,
            hoisted_locations,
        ));
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

/// Where one injected snapshot's copies live, as lockfile-relative
/// paths with POSIX separators on every platform.
fn injected_targets(
    layout: &VirtualStoreLayout,
    lockfile_dir: &Path,
    key: &PackageKey,
    hoisted_locations: Option<&std::collections::BTreeMap<String, Vec<String>>>,
) -> Vec<String> {
    // Hoisted linker: the walker already recorded every
    // lockfile-relative dir this depPath was placed at.
    if let Some(locations) = hoisted_locations {
        return locations.get(&key.to_string()).cloned().unwrap_or_default();
    }
    // Isolated linker: one virtual-store slot per snapshot. The
    // separator normalization matches the `hoistedLocations` entries the
    // hoisted branch reuses (see `path_relative_to_lockfile_dir`).
    let target = layout.slot_dir(key).join("node_modules").join(key.name.to_string());
    let target = match target.strip_prefix(lockfile_dir) {
        Ok(relative) => relative.to_path_buf(),
        Err(_) => target,
    };
    vec![target.to_string_lossy().replace('\\', "/")]
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
            child_graph_key(alias, dep_ref, lockfile_dir)
        });
        link_target_nodes
            .extend(children.values().filter(|child_key| child_key.starts_with("link:")).cloned());
        graph.insert(
            snapshot_key.to_string(),
            DepsGraphNode { full_pkg_id: full_pkg_id_of(snapshot_key, packages), children },
        );
    }
    for link_target_node in link_target_nodes {
        graph.insert(
            link_target_node.clone(),
            DepsGraphNode { full_pkg_id: link_target_node, children: IndexMap::default() },
        );
    }
    graph
}

fn child_graph_key(
    alias: &pnpm_lockfile::PkgName,
    dep_ref: &pnpm_lockfile::SnapshotDepRef,
    lockfile_dir: Option<&Path>,
) -> Option<String> {
    if let Some(snapshot_key) = dep_ref.resolve(alias) {
        return Some(snapshot_key.to_string());
    }
    let link_target = dep_ref.as_link_target()?;
    let resolved = pnpm_fs::lexical_normalize(&lockfile_dir?.join(link_target));
    Some(format!("link:{}", resolved.to_string_lossy()))
}

fn full_pkg_id_of(
    snapshot_key: &PackageKey,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> String {
    let pkg_id_with_patch_hash =
        PkgIdWithPatchHash::from(get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string());
    let resolution = packages
        .and_then(|packages| packages.get(&snapshot_key.without_peer()))
        .map(|meta| &meta.resolution);
    create_full_pkg_id(&pkg_id_with_patch_hash, resolution)
}

/// Length-prefixed so two adjacent fields cannot be confused with one
/// longer field carrying the same bytes.
fn write_field(hasher: &mut sha2::Sha256, value: &str) {
    use sha2::Digest as _;
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

/// Hashes every snapshot's global-virtual-store slot suffix over one dep
/// graph, gating set and memo for the whole lockfile.
struct GvsHasher<'h> {
    graph: HashMap<String, DepsGraphNode<String>>,
    /// The engine-agnostic gating set. `None` disables gating so every
    /// snapshot still hashes with its engine string.
    build_required_dep_paths: Option<HashSet<String>>,
    cache: DepsStateCache<String>,
    /// One conversion for the whole lockfile: the same string scopes
    /// every local directory snapshot in it.
    ///
    /// Lossy on purpose: the TypeScript CLI hashes the same slot from a
    /// JS string, and Node decodes a path as UTF-8 with replacement, so
    /// this is the identical input. Hashing the raw bytes instead would
    /// give the two stacks different slots for the same project.
    project_scope: Option<std::borrow::Cow<'h, str>>,
    engine: Option<&'h str>,
    packages: Option<&'h HashMap<PackageKey, PackageMetadata>>,
}

impl<'h> GvsHasher<'h> {
    fn new(
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
        packages: Option<&'h HashMap<PackageKey, PackageMetadata>>,
        engine: Option<&'h str>,
        allow_build_policy: Option<&AllowBuildPolicy>,
        lockfile_dir: Option<&'h Path>,
    ) -> Self {
        let graph = lockfile_to_dep_graph(snapshots, packages, lockfile_dir);
        let build_required_dep_paths =
            allow_build_policy.map(|policy| engine_gating_dep_paths(policy, snapshots, &graph));
        Self {
            graph,
            build_required_dep_paths,
            cache: HashMap::new(),
            project_scope: lockfile_dir.map(|dir| dir.to_string_lossy()),
            engine,
            packages,
        }
    }

    /// Per-snapshot engine resolution: a snapshot that declares its own
    /// `engines.runtime` carries the desugared
    /// `dependencies.node: 'runtime:<version>'` pin, which has to drive
    /// the engine portion of *its* hash rather than the install-wide
    /// fallback. Precedence: own pin first, install-wide fallback
    /// second. Default host platform / arch (`None`, `None`) matches
    /// whatever the caller used to format the fallback `engine` so the
    /// two strings remain comparable across snapshots in one install.
    /// Every snapshot's suffix, walked in lockfile key order rather
    /// than `HashMap` order: [`calc_graph_node_hash`] memoizes into
    /// `cache`, and for a snapshot inside a dependency cycle the digest
    /// that lands there depends on which snapshot the walk reached it
    /// from.
    fn suffixes(
        &mut self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) -> HashMap<PackageKey, String> {
        let mut gvs_suffixes = HashMap::with_capacity(snapshots.len());
        for (snapshot_key, snapshot) in crate::deps_graph::in_lockfile_order(snapshots) {
            gvs_suffixes.insert(snapshot_key.clone(), self.suffix(snapshot_key, snapshot));
        }
        gvs_suffixes
    }

    /// Digest of everything [`Self::suffixes`] would read, so a cached
    /// map can be filed under it.
    ///
    /// Taken from the dep graph this hasher already built, plus the
    /// per-snapshot values `suffix` reads that the graph does not
    /// carry. Hashing the graph rather than the lockfile it came from
    /// is what keeps this honest: the suffixes and the key are then two
    /// functions of the same values, and no re-read can put them out of
    /// step.
    ///
    /// Cheap next to what it guards. On a 1355-node lockfile the graph
    /// build is ~2 ms and the suffix loop it lets us skip is ~8 ms.
    fn fingerprint(&self, snapshots: &HashMap<PackageKey, SnapshotEntry>) -> String {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        write_field(&mut hasher, gvs_layout_cache::CACHE_FORMAT_VERSION);
        // `None` and `Some("")` are different payloads downstream, so
        // they must not collapse here.
        match self.engine {
            Some(engine) => {
                hasher.update([1_u8]);
                write_field(&mut hasher, engine);
            }
            None => hasher.update([0_u8]),
        }
        write_field(&mut hasher, self.project_scope.as_deref().unwrap_or(""));
        self.write_gating_set(&mut hasher);
        self.write_graph(&mut hasher);
        self.write_snapshot_extras(&mut hasher, snapshots);
        format!("{:x}", hasher.finalize())
    }

    /// The set that decides which snapshots carry the engine string.
    ///
    /// Tagged, because `None` is not an empty set: `calc_graph_node_hash`
    /// reads `None` as "gating off" and puts the engine in every
    /// snapshot's hash, while an empty set puts it in none.
    fn write_gating_set(&self, hasher: &mut sha2::Sha256) {
        use sha2::Digest as _;
        let Some(paths) = self.build_required_dep_paths.as_ref() else {
            return hasher.update([0_u8]);
        };
        hasher.update([1_u8]);
        let mut sorted: Vec<&str> = paths.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        write_field(hasher, &sorted.join("\u{0}"));
    }

    /// The dep graph, in an order a `HashMap` cannot vary.
    fn write_graph(&self, hasher: &mut sha2::Sha256) {
        use sha2::Digest as _;
        let mut node_keys: Vec<&String> = self.graph.keys().collect();
        node_keys.sort_unstable();
        hasher.update((node_keys.len() as u64).to_le_bytes());
        for node_key in node_keys {
            let node = &self.graph[node_key];
            write_field(hasher, node_key);
            write_field(hasher, &node.full_pkg_id);
            hasher.update((node.children.len() as u64).to_le_bytes());
            for (alias, child_key) in &node.children {
                write_field(hasher, alias);
                write_field(hasher, child_key);
            }
        }
    }

    /// What [`Self::suffix`] reads that the graph does not carry: a
    /// snapshot's own `engines.runtime` pin, and the version segment
    /// its metadata contributes.
    fn write_snapshot_extras(
        &self,
        hasher: &mut sha2::Sha256,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) {
        for (snapshot_key, snapshot) in crate::deps_graph::in_lockfile_order(snapshots) {
            let metadata_key = snapshot_key.without_peer();
            let metadata = self.packages.and_then(|packages| packages.get(&metadata_key));
            write_field(hasher, &snapshot_key.to_string());
            write_field(
                hasher,
                &find_own_runtime_node_major(snapshot)
                    .map(|major| engine_name(major, None, None))
                    .unwrap_or_default(),
            );
            write_field(hasher, &gvs_version_segment(metadata, &metadata_key.suffix));
        }
    }

    fn suffix(&mut self, snapshot_key: &PackageKey, snapshot: &SnapshotEntry) -> String {
        let own_engine =
            find_own_runtime_node_major(snapshot).map(|major| engine_name(major, None, None));
        let metadata_key = snapshot_key.without_peer();
        let metadata = self.packages.and_then(|packages| packages.get(&metadata_key));
        let hex_digest = calc_graph_node_hash(
            &self.graph,
            &mut self.cache,
            &snapshot_key.to_string(),
            own_engine.as_deref().or(self.engine),
            self.build_required_dep_paths.as_ref(),
            local_directory_scope(metadata, &metadata_key.suffix, self.project_scope.as_deref()),
        );
        format_global_virtual_store_path(
            &metadata_key.name.to_string(),
            &gvs_version_segment(metadata, &metadata_key.suffix),
            &hex_digest,
        )
    }
}

fn engine_gating_dep_paths(
    policy: &AllowBuildPolicy,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    graph: &HashMap<String, DepsGraphNode<String>>,
) -> HashSet<String> {
    let built_dep_paths = snapshots
        .keys()
        .filter(|key| policy.check(&key.without_peer().to_string()) == Some(true))
        .map(ToString::to_string)
        .collect();
    pnpm_graph_hasher::build_required_dep_paths(graph, &built_dep_paths)
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

/// On-disk cache for the derived global-virtual-store suffix map.
///
/// The map — every snapshot's `<scope>/<name>/<version>/<hash>` slot
/// suffix — is a pure function of its inputs, so a run whose inputs are
/// unchanged loads it instead of deriving it. What that skips is the
/// recursive hash per snapshot; the dep graph is built either way,
/// because the key is derived from it.
///
/// Modelled on the lockfile-verification cache
/// (`<cache_dir>/lockfile-verified.jsonl`) and it lives next to it, in
/// `cache_dir`: derived state a run may always recompute, never
/// something an install depends on being there.
///
/// The key is [`GvsHasher::fingerprint`] — a digest of the dep graph
/// the suffixes are derived from, plus the engine string, the
/// allow-build gating set, the project scope and a format version.
/// Deriving the key and the suffixes from the same in-hand values is
/// what keeps them in step; a key taken from a re-read of the lockfile
/// could describe a revision the suffixes did not come from.
mod gvs_layout_cache {
    use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
    use sha2::Digest as _;
    use std::{
        collections::HashMap,
        io::{Read as _, Write},
        path::{Path, PathBuf},
    };

    /// Bumped whenever the derived suffixes or this file's encoding
    /// change, so entries written by an older pnpm are never read.
    pub(super) const CACHE_FORMAT_VERSION: &str = "1";

    /// Generous per-snapshot ceiling on a cache file, so a preseeded one
    /// cannot make an install read an arbitrary amount before it has
    /// validated anything. A real entry is a package key plus a slot
    /// suffix and two length prefixes; both strings are bounded in
    /// practice by npm's 214-character name limit plus a version and a
    /// 64-character digest.
    const MAX_ENTRY_BYTES: u64 = 4096;

    /// One file per lockfile directory, not one per fingerprint.
    ///
    /// The fingerprint changes with every lockfile edit, engine change
    /// and build-policy change, so filing entries under it would leave
    /// a snapshot-sized file behind in the user's cache for each — and
    /// nothing prunes them. Naming the file after the project instead
    /// and keeping the fingerprint *inside* it means the newest entry
    /// replaces the previous one, at the cost of not being able to
    /// switch between two lockfiles without re-deriving.
    fn cache_path(cache_dir: &Path, lockfile_dir: &Path) -> PathBuf {
        let mut hasher = sha2::Sha256::new();
        hasher.update(CACHE_FORMAT_VERSION.as_bytes());
        hasher.update(lockfile_dir.to_string_lossy().as_bytes());
        cache_dir.join("gvs-layout").join(format!("{:x}.bin", hasher.finalize()))
    }

    /// Where one project's entry lives and what it must have been
    /// derived from.
    #[derive(Clone, Copy)]
    pub(super) struct CacheFile<'a> {
        pub cache_dir: &'a Path,
        pub lockfile_dir: &'a Path,
        pub fingerprint: &'a str,
    }

    /// Read a cached map back, or `None` to derive it instead.
    ///
    /// Length-prefixed pairs — `u32 key_len | key | u32 val_len | value`
    /// — rather than JSON: the map runs to thousands of entries and the
    /// whole point is to beat the derivation it replaces.
    ///
    /// What comes back off disk is checked against `expected` before it
    /// is trusted, because a suffix decides where a package is linked
    /// from. Nothing here re-derives a hash — that is the work being
    /// avoided — but two things are cheap and rule out the ways a wrong
    /// map does damage:
    ///
    ///   * every snapshot the caller holds must be present. A file
    ///     truncated between pairs otherwise parses as a shorter map,
    ///     and each absent snapshot silently takes
    ///     [`super::VirtualStoreLayout::slot_dir`]'s flat-name fallback,
    ///     landing outside the global virtual store;
    ///   * every suffix must name the package it is filed under.
    ///     `cacheDir` is settable from a repository's own
    ///     `pnpm-workspace.yaml`, so a hostile repository can commit a
    ///     cache entry; this is what stops one from pointing a package
    ///     at a slot holding some *other* package. What it cannot rule
    ///     out is a different dependency-set variant of the same
    ///     `name@version`, whose slot holds that package's own
    ///     published files either way.
    pub(super) fn load(
        file: CacheFile<'_>,
        expected: Expected<'_>,
    ) -> Option<HashMap<PackageKey, String>> {
        // A hostile checkout can choose `cacheDir` and so write this
        // file, and it has to be read before any of it can be checked.
        // The read is therefore bounded by what a legitimate map for
        // *these* snapshots could need. Nothing observable changes when
        // the bound is hit — an over-long file fails validation below
        // either way — so this only caps the memory a preseeded one can
        // make an install allocate.
        let path = cache_path(file.cache_dir, file.lockfile_dir);
        // A hostile checkout can point `cacheDir` at a directory it
        // ships, so this path may be anything it likes. Opening a FIFO
        // blocks until someone writes to it, which would hang the
        // install before it has read a single package: require a
        // regular file, and one that is not reached through a symlink,
        // before opening.
        if !std::fs::symlink_metadata(&path).ok()?.is_file() {
            return None;
        }
        let handle = std::fs::File::open(&path).ok()?;
        if !handle.metadata().ok()?.is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        let ceiling = (expected.snapshots.len() as u64 + 1).saturating_mul(MAX_ENTRY_BYTES);
        handle.take(ceiling).read_to_end(&mut bytes).ok()?;
        let (stored_fingerprint, mut cursor) = read_field(&bytes, 0)?;
        if stored_fingerprint != file.fingerprint {
            return None;
        }
        let mut suffixes = HashMap::with_capacity(expected.snapshots.len());
        while cursor < bytes.len() {
            let (package_key, next) = read_field(&bytes, cursor)?;
            let (suffix, next) = read_field(&bytes, next)?;
            cursor = next;
            let package_key = package_key.parse::<PackageKey>().ok()?;
            let digest = suffix.strip_prefix(&expected.slot_prefix(&package_key)?)?;
            if !is_graph_node_digest(digest) {
                return None;
            }
            suffixes.insert(package_key, suffix.to_owned());
        }
        expected.snapshots.keys().all(|key| suffixes.contains_key(key)).then_some(suffixes)
    }

    /// Whether `digest` is the hex `calc_graph_node_hash` produces, and
    /// so a single path component.
    ///
    /// The prefix check above says a suffix names the right package;
    /// this says the rest of it is a name and not a route. Without it a
    /// suffix ending `1.2.3/../../../..` passes the prefix check and
    /// then [`super::join_global_virtual_store_path`] walks it right
    /// back out of the store, because that function's job is to split a
    /// suffix into components rather than to judge them.
    fn is_graph_node_digest(digest: &str) -> bool {
        digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    /// The snapshots a cached map has to describe, and the metadata
    /// that says how each one's slot path begins.
    #[derive(Clone, Copy)]
    pub(super) struct Expected<'a> {
        pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    }

    impl Expected<'_> {
        /// `<scope>/<name>/<version>/` — the part of a slot suffix that
        /// follows from the snapshot key alone, leaving only the graph
        /// hash unverified. `None` for a key the caller does not hold,
        /// which fails the entry.
        fn slot_prefix(&self, package_key: &PackageKey) -> Option<String> {
            let metadata_key = package_key.without_peer();
            self.snapshots.get(package_key)?;
            let metadata = self.packages.and_then(|packages| packages.get(&metadata_key));
            let name = metadata_key.name.to_string();
            let version = super::gvs_version_segment(metadata, &metadata_key.suffix);
            // The empty digest leaves exactly the fixed part of a
            // suffix: `<scope>/<name>/<version>/`.
            Some(super::format_global_virtual_store_path(&name, &version, ""))
        }
    }

    fn read_field(bytes: &[u8], cursor: usize) -> Option<(&str, usize)> {
        let len_end = cursor.checked_add(4)?;
        let len = u32::from_le_bytes(bytes.get(cursor..len_end)?.try_into().ok()?) as usize;
        let field_end = len_end.checked_add(len)?;
        let field = std::str::from_utf8(bytes.get(len_end..field_end)?).ok()?;
        Some((field, field_end))
    }

    /// Best-effort write: a cache that cannot be written is a slower
    /// install, never a failed one.
    pub(super) fn store(file: CacheFile<'_>, suffixes: &HashMap<PackageKey, String>) {
        let path = cache_path(file.cache_dir, file.lockfile_dir);
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let mut bytes = Vec::with_capacity(suffixes.len() * 192);
        write_field(&mut bytes, file.fingerprint);
        for (package_key, suffix) in suffixes {
            write_field(&mut bytes, &package_key.to_string());
            write_field(&mut bytes, suffix);
        }
        // Staged under a name only this writer knows, then renamed, so
        // a concurrent reader never observes a half-written map and
        // two concurrent writers never share a staging file. A
        // predictable one would also let anything that can write the
        // cache directory redirect the write through a symlink.
        let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) else { return };
        // No `sync_all`: this runs on the miss path, after the map has
        // already been derived, and an fsync there is latency spent on
        // the install this cache exists to speed up. A crash mid-write
        // costs a torn entry, which `load` refuses and re-derives.
        if file.write_all(&bytes).is_ok() {
            let _ = file.persist(&path);
        }
    }

    fn write_field(bytes: &mut Vec<u8>, field: &str) {
        bytes.extend_from_slice(&(field.len() as u32).to_le_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
}
