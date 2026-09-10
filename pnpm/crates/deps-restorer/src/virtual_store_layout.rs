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

mod graph_hash;
use graph_hash::GvsHasher;

use crate::{
    AllowBuildPolicy,
    install_frozen_lockfile::{find_runtime_node_major, parse_major_from_version},
};
use pnpm_config::Config;
use pnpm_graph_hasher::{
    detect_node_major, engine_name, format_global_virtual_store_path,
    join_global_virtual_store_path,
};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgVerPeer, SnapshotEntry, VersionPart,
};
use std::{
    collections::HashMap,
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
    /// via [`calc_graph_node_hash`](pnpm_graph_hasher::calc_graph_node_hash).
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
    /// [`calc_graph_node_hash`](pnpm_graph_hasher::calc_graph_node_hash)'s `engine` parameter — `None` and
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
    if !pnpm_package_name::is_valid_dependency_alias(&name)
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
mod gvs_layout_cache;
