//! Locating and repairing the virtual-store slot a build runs in.

use super::{
    BuildModulesError, HashMap, ImportIndexedDirOpts, NEEDS_BUILD_MARKER, PackageImportMethod,
    PackageKey, Path, PathBuf, Reporter, import_indexed_dir,
};

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
/// The package-name segment is `key.name`, which carries no
/// peer context: the slot's `node_modules/<pkg>` is keyed by the bare
/// package name whatever the peers resolved to.
///
/// [#432]: https://github.com/pnpm/pacquet/issues/432
pub(crate) fn virtual_store_dir_for_key(
    layout: &crate::VirtualStoreLayout,
    key: &PackageKey,
) -> PathBuf {
    let name = key.name.to_string();

    #[cfg(windows)]
    let name = pnpm_fs::to_native_separators(Path::new(&name));

    layout
        .slot_dir(key)
        .join("node_modules")
        .join(name)
}

/// Whether `pkg_dir` already holds every file of a side-effects-cache
/// overlay — i.e. the cached build is on disk rather than merely recorded
/// in the store index.
///
/// The overlay is the resolved post-build file set, so a slot still
/// carrying only the pristine tarball is missing whatever the build
/// added and fails the check. The file set alone cannot see a build
/// that *only deleted* files, which is why [`NEEDS_BUILD_MARKER`] is
/// checked first: a pristine re-import of a package that needs building
/// carries the marker, so it reports unseeded no matter what the overlay
/// looks like.
///
/// Only reached for packages that both pass the build-allow policy and
/// have a cache entry — a handful per install, not the whole tree.
pub(crate) fn slot_carries_overlay(pkg_dir: &Path, overlay: &HashMap<String, PathBuf>) -> bool {
    !pkg_dir.join(NEEDS_BUILD_MARKER).exists()
        && pkg_dir.is_dir()
        && overlay
            .keys()
            .all(|relative| pkg_dir.join(relative).exists())
}

pub(crate) const FAILED_BUILD_MARKER: &str = "failed";

pub(crate) fn is_failed_build_marker(marker: &Path) -> bool {
    std::fs::read_to_string(marker).is_ok_and(|content| content.trim() == FAILED_BUILD_MARKER)
}

/// Mark a snapshot's global-virtual-store slot as still needing its build
/// after its patch application or build script failed.
///
/// The slot stays in place: other projects whose dependency graph hashes
/// to it may already link it. The marker makes the next install that
/// reaches the slot re-import its pristine files and build it again.
///
/// No-op outside the isolated global virtual store: the next install
/// rebuilds a project-local slot from scratch anyway. A failed write is
/// logged and swallowed; the build error the caller is already returning
/// is the one worth surfacing.
pub(crate) fn mark_failed_global_virtual_store_build(pkg_roots: PkgRoots<'_>, key: &PackageKey) {
    if !pkg_roots.layout.enable_global_virtual_store() || pkg_roots.by_key.is_some() {
        return;
    }
    let marker = virtual_store_dir_for_key(pkg_roots.layout, key).join(NEEDS_BUILD_MARKER);
    if let Err(error) = std::fs::write(&marker, FAILED_BUILD_MARKER)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            target: "pacquet::build",
            ?error,
            dep_path = %key,
            marker = %marker.display(),
            "failed to mark the global virtual store slot of a failed build",
        );
    }
}

/// Where each snapshot's package sits on disk, under either linker.
///
/// `by_key` is what distinguishes the two: the isolated linker leaves it
/// `None` and every lookup derives the one virtual-store slot from
/// `layout`, while the hoisted walker fills it with the paths it chose,
/// which may be several for one snapshot.
#[derive(Clone, Copy)]
pub(crate) struct PkgRoots<'a> {
    pub layout: &'a crate::VirtualStoreLayout,
    pub by_key: Option<&'a HashMap<PackageKey, Vec<PathBuf>>>,
}

impl PkgRoots<'_> {
    /// The canonical on-disk package directory for a snapshot — the one
    /// whose lifecycle scripts run and whose contents seed the
    /// side-effects cache.
    ///
    /// Under the hoisted linker this is the first directory the walker
    /// recorded. `None` there means the snapshot is absent from the
    /// hoisted graph (pre-skipped, or the walker decided not to record
    /// it); the caller should treat that the same as the isolated
    /// `pkg_dir.exists() == false` skip.
    ///
    /// Use [`Self::all`] instead for a write that has to reach every copy
    /// of the package.
    pub(crate) fn canonical(self, key: &PackageKey) -> Option<PathBuf> {
        match self.by_key {
            Some(map) => map
                .get(key)
                .and_then(|dirs| dirs.first())
                .cloned(),
            None => Some(virtual_store_dir_for_key(self.layout, key)),
        }
    }

    /// Every on-disk directory holding a snapshot's package.
    ///
    /// The isolated linker gives each snapshot exactly one virtual-store
    /// slot, so this is [`Self::canonical`] in a one-element list. The
    /// hoisted linker can place the same snapshot at several paths — a
    /// version conflict keeps a package out of the root and the walker
    /// nests a copy under each consumer that needs it.
    pub(crate) fn all(self, key: &PackageKey) -> Vec<PathBuf> {
        match self.by_key {
            Some(map) => map
                .get(key)
                .cloned()
                .unwrap_or_default(),
            None => vec![virtual_store_dir_for_key(self.layout, key)],
        }
    }
}

/// Re-import a snapshot's package directory from the side-effects cache
/// overlay (the `base - deleted + added` file set already resolved to
/// CAS paths by [`pnpm_store_dir::build_file_maps_from_index`]).
///
/// The warm-link phase materializes only the pristine tarball files, so
/// a cached build whose `is_built` gate fires would otherwise leave the
/// slot in its pre-build state. A forced re-import rebuilds the directory
/// to match the overlay exactly (adding the build output and dropping any
/// files the build deleted) while preserving the slot's nested
/// `node_modules/` symlinks.
///
/// The import always runs on a cache hit (non-GVS). Skipping it when the
/// slot "looks" materialized is unsound by filename alone — a slot left
/// from a different cache key can carry the same filenames with stale
/// bytes — and a content check would read every file, costing as much as
/// the hardlink-based re-import it would replace. A cheap *and* sound skip
/// needs a link-phase "this slot was re-linked pristine-only this install"
/// signal threaded from the link phase, which is left as a follow-up.
pub(crate) fn materialize_side_effects<Reporter: self::Reporter>(
    logged_methods: &std::sync::atomic::AtomicU8,
    import_method: PackageImportMethod,
    pkg_dir: &Path,
    overlay: &HashMap<String, PathBuf>,
) -> Result<(), BuildModulesError> {
    import_indexed_dir::<Reporter>(
        logged_methods,
        import_method,
        pkg_dir,
        overlay,
        ImportIndexedDirOpts {
            force: true,
            keep_modules_dir: true,
            ..ImportIndexedDirOpts::default()
        },
    )
    .map_err(BuildModulesError::MaterializeSideEffects)
}

/// Walk every ancestor `node_modules/.bin` from `pkg_root` up to
/// (and including) `lockfile_dir`. Used as the per-snapshot
/// `extra_bin_paths` under `nodeLinker: hoisted` so a lifecycle
/// script invoked at a nested location can resolve bins added by
/// any ancestor's `node_modules/.bin` — npm-style ancestor-chain
/// resolution that the isolated layout doesn't need (every slot's
/// children sit in its own `node_modules`, and bin-link writes are
/// per-slot).
///
/// A step is skipped when `dir`'s parent path string starts with
/// `@` — a guard for relative-path code paths. The check is against
/// the parent's path-string first character.
///
/// Non-existent ancestor `.bin` directories are harmless: they
/// just don't contribute anything to lifecycle-script PATH lookup.
pub(crate) fn bin_dirs_in_all_parent_dirs(pkg_root: &Path, lockfile_dir: &Path) -> Vec<PathBuf> {
    let mut bin_dirs: Vec<PathBuf> = Vec::new();
    let mut dir: PathBuf = pkg_root.to_path_buf();
    loop {
        let parent = dir.parent().unwrap_or_else(|| Path::new(""));
        let parent_starts_with_at = parent
            .to_str()
            .and_then(|text| text.chars().next())
            .is_some_and(|ch| ch == '@');
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
#[must_use]
pub fn parse_name_version_from_key(key: &str) -> (String, String) {
    let stripped = key.strip_prefix('/').unwrap_or(key);
    match stripped.rfind('@') {
        Some(idx) if idx > 0 => (stripped[..idx].to_string(), stripped[idx + 1..].to_string()),
        _ => (stripped.to_string(), String::new()),
    }
}
