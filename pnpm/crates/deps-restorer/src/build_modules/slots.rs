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
/// The package-name segment still comes from the peer-stripped key,
/// because the slot's `node_modules/<pkg>` is keyed by the bare
/// package name regardless of peer context.
///
/// [#432]: https://github.com/pnpm/pacquet/issues/432
pub(crate) fn virtual_store_dir_for_key(
    layout: &crate::VirtualStoreLayout,
    key: &PackageKey,
) -> PathBuf {
    let bare_key = key.without_peer();
    let key_str = bare_key.to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);

    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let name = &name_version[..at_idx];

    layout.slot_dir(key).join("node_modules").join(name)
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
        && overlay.keys().all(|relative| pkg_dir.join(relative).exists())
}

/// Whether `slot_dir` is a strict descendant of `root` reached only
/// through `..`-free path components.
///
/// The gate for [`discard_failed_global_virtual_store_slot`]'s recursive
/// delete: `slot_dir` is derived from a lockfile-controlled package
/// name, so a crafted `..` segment must not let the delete escape the
/// store root.
pub(crate) fn is_contained_descendant(root: &Path, slot_dir: &Path) -> bool {
    slot_dir.strip_prefix(root).is_ok_and(|suffix| {
        let mut components = suffix.components().peekable();
        components.peek().is_some()
            && components.all(|component| matches!(component, std::path::Component::Normal(_)))
    })
}

/// Remove a snapshot's whole global-virtual-store hash directory after
/// its patch application or build script failed.
///
/// The hash directory is shared across every project that resolves to
/// the same dependency graph, so leaving a half-built one behind would
/// serve broken files to all of them: the next install finds the
/// directory present, takes the warm fast path, and never re-fetches.
/// Removing it restores the cold path.
///
/// No-op when the global virtual store is off — a project-local
/// `node_modules/.pnpm` slot is rebuilt from scratch by the next
/// install anyway. Removal failures are logged and swallowed; the build
/// error the caller is already returning is the one worth surfacing.
pub(crate) fn discard_failed_global_virtual_store_slot(
    layout: &crate::VirtualStoreLayout,
    key: &PackageKey,
) {
    if !layout.enable_global_virtual_store() {
        return;
    }
    let slot_dir = layout.slot_dir(key);
    // Defense-in-depth: the slot path is built from a lockfile-controlled
    // package name, which is not validated against `..` segments. Refuse
    // to recurse-delete anything that isn't a plain descendant of the GVS
    // root, so a crafted name can't turn cleanup into a path traversal
    // that removes directories outside the store.
    let root = layout.package_store_dir();
    if !is_contained_descendant(root, &slot_dir) {
        tracing::warn!(
            target: "pacquet::build",
            dep_path = %key,
            slot_dir = %slot_dir.display(),
            store_root = %root.display(),
            "refusing to remove a build slot outside the store root",
        );
        return;
    }
    if let Err(err) = std::fs::remove_dir_all(&slot_dir)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            target: "pacquet::build",
            ?err,
            dep_path = %key,
            slot_dir = %slot_dir.display(),
            "failed to remove the global virtual store slot of a failed build",
        );
    }
}

/// Resolve the canonical on-disk package directory for a snapshot — the
/// one whose lifecycle scripts run and whose contents seed the
/// side-effects cache.
///
/// Two-mode lookup:
///
/// - **Isolated** (`pkg_roots_by_key.is_none()`) — fall through to
///   [`virtual_store_dir_for_key`], which routes through the
///   install-scoped [`crate::VirtualStoreLayout`].
/// - **Hoisted** (`pkg_roots_by_key.is_some()`) — take the first
///   directory the slice 4 walker recorded for the snapshot. `None` here
///   means the snapshot is absent from the hoisted graph (pre-skipped, or
///   the walker decided not to record it); the caller should treat
///   that the same as the isolated `pkg_dir.exists() == false` skip.
///
/// Use [`pkg_roots_for_key`] instead for a write that has to reach every
/// copy of the package.
pub(crate) fn pkg_root_for_key(
    layout: &crate::VirtualStoreLayout,
    pkg_roots_by_key: Option<&HashMap<PackageKey, Vec<PathBuf>>>,
    key: &PackageKey,
) -> Option<PathBuf> {
    match pkg_roots_by_key {
        Some(map) => map.get(key).and_then(|dirs| dirs.first()).cloned(),
        None => Some(virtual_store_dir_for_key(layout, key)),
    }
}

/// Every on-disk directory holding a snapshot's package.
///
/// The isolated linker gives each snapshot exactly one virtual-store
/// slot, so this is [`pkg_root_for_key`] in a one-element list. The
/// hoisted linker can place the same snapshot at several paths — a
/// version conflict keeps a package out of the root and the walker nests
/// a copy under each consumer that needs it.
pub(crate) fn pkg_roots_for_key(
    layout: &crate::VirtualStoreLayout,
    pkg_roots_by_key: Option<&HashMap<PackageKey, Vec<PathBuf>>>,
    key: &PackageKey,
) -> Vec<PathBuf> {
    match pkg_roots_by_key {
        Some(map) => map.get(key).cloned().unwrap_or_default(),
        None => vec![virtual_store_dir_for_key(layout, key)],
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
#[must_use]
pub fn parse_name_version_from_key(key: &str) -> (String, String) {
    let stripped = key.strip_prefix('/').unwrap_or(key);
    match stripped.rfind('@') {
        Some(idx) if idx > 0 => (stripped[..idx].to_string(), stripped[idx + 1..].to_string()),
        _ => (stripped.to_string(), String::new()),
    }
}
