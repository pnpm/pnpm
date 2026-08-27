//! macOS materialization cache that turns the per-file import of a
//! package into a single directory `clonefile(2)`.
//!
//! APFS serializes metadata-write syscalls volume-wide: measured on the
//! alotta-files fixture, `clonefile` throughput never exceeds its
//! single-thread rate (~6k files/s) no matter how many threads issue
//! it, and `link` scales *negatively* — extra rayon workers only deepen
//! the kernel lock convoy, which is what regressed large hot installs
//! on many-core Macs (pnpm/pnpm issue 14231). Cloning a whole directory
//! is one syscall for the entire tree, ~20x cheaper than per-file
//! clones of its contents and immune to the convoy.
//!
//! The cache materializes each package once into the global-virtual-
//! store slot layout under `<store_dir>/links` — the same paths, marker
//! protocol, and concurrent-writer healing that `enableGlobalVirtualStore`
//! installs use (see [`fn@crate::import_indexed_dir`]) — and then
//! projects it into the project-local virtual store with one
//! `clonefile`. A hot install whose canonical slots already exist pays
//! one `stat` plus one `clonefile` per package instead of one syscall
//! per file.
//!
//! Only the isolated-linker frozen install path consults the cache, and
//! only when the resolved import method may clone (`auto`, `clone`,
//! `clone-or-copy`): an explicit `hardlink` promises store-shared
//! inodes and an explicit `copy` promises independent data, and a
//! clone of the canonical copy would deliver neither. Per-slot
//! qualification is decided by the caller (see
//! `create_virtual_store::dir_clone_cacheable`): packages that need a
//! build or patch marker, come from mutable local sources, must be
//! force-re-imported, or resolve without a checkable integrity (git
//! dependencies, whose slot hash cannot see whether their fetch-time
//! `prepare` ran) skip the cache — their slots go through the per-file
//! path exactly as before, so cached slots are always plain pre-build
//! CAS content, indistinguishable from what a GVS-enabled install
//! materializes before its build phase.
//!
//! The cache is strictly best-effort: a per-install capability probe
//! (`dir_clone_supported`) declines the whole cache up front when the
//! canonical root and the project's virtual store can't share clones
//! (cross-volume stores, non-APFS filesystems), so no canonical slot is
//! ever populated that the clone step can't use. Past the probe, any
//! per-package failure falls back to the per-file import, and failures
//! that indicate the volume cannot clone directories (`EXDEV`,
//! `ENOTSUP`, ...) disable the cache for the rest of the process.

use crate::{
    AllowBuildPolicy, VirtualStoreLayout,
    import_indexed_dir::{ImportIndexedDirOpts, marker_present},
    safe_join_modules_dir::safe_join_modules_dir,
};
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_reporter::Reporter;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

/// See the [module documentation](self) for what the cache is and when
/// it applies.
pub struct DirCloneCache {
    /// GVS-shaped layout rooted at `Config::global_virtual_store_dir`
    /// (`<store_dir>/links` unless pinned), built via
    /// [`VirtualStoreLayout::global`] so canonical slots coincide with
    /// the slots a GVS-enabled install materializes.
    layout: VirtualStoreLayout,
    /// Under `frozenStore` the store is read-only: the cache may serve
    /// canonical slots that already exist but must not populate new
    /// ones.
    frozen_store: bool,
    disabled: AtomicBool,
}

impl DirCloneCache {
    /// Whether this install's configuration can use the cache at all.
    /// Split from [`Self::build`] so the engine-name resolution in
    /// `install_frozen_lockfile` can know up front that the layout
    /// below will need the engine synchronously.
    #[must_use]
    pub fn eligible(config: &Config, node_linker: NodeLinker) -> bool {
        cfg!(target_os = "macos")
            && node_linker == NodeLinker::Isolated
            && !config.enable_global_virtual_store
            && matches!(
                config.package_import_method,
                PackageImportMethod::Auto
                    | PackageImportMethod::Clone
                    | PackageImportMethod::CloneOrCopy,
            )
    }

    /// Build the cache for one install, or `None` when
    /// [`Self::eligible`] says the configuration can't use it.
    ///
    /// `engine` must be the same value a GVS-enabled install would
    /// compute (see `resolve_engine_name`), so the canonical slot
    /// hashes agree between the two modes.
    #[must_use]
    pub fn build(
        config: &Config,
        node_linker: NodeLinker,
        engine: Option<&str>,
        snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
        packages: Option<&HashMap<PackageKey, PackageMetadata>>,
        allow_build_policy: Option<&AllowBuildPolicy>,
        lockfile_dir: Option<&Path>,
    ) -> Option<Self> {
        if !Self::eligible(config, node_linker) {
            return None;
        }
        // Establish once, before any package is materialized, that a
        // directory clone from the canonical root's volume into the
        // project's virtual store can succeed at all. Without this, a
        // cross-volume or clone-incapable layout would have the first
        // wave of parallel slot links each populate a canonical slot
        // only to fall back per-file after the clone fails. Under
        // `frozenStore` the cache never writes canonical slots, so
        // there is no duplicated work to prevent — and the store must
        // not be written a probe directory either.
        if !config.frozen_store
            && !dir_clone_supported(&config.global_virtual_store_dir, &config.virtual_store_dir)
        {
            return None;
        }
        Some(DirCloneCache {
            layout: VirtualStoreLayout::global(
                config.global_virtual_store_dir.clone(),
                config.virtual_store_dir_max_length as usize,
                engine,
                snapshots,
                packages,
                allow_build_policy,
                lockfile_dir,
            ),
            frozen_store: config.frozen_store,
            disabled: AtomicBool::new(false),
        })
    }

    /// Try to materialize `save_path` (the project slot's
    /// `node_modules/<name>` package directory) by cloning the
    /// package's canonical slot. Returns `true` when the slot is fully
    /// materialized; `false` means the caller must run the per-file
    /// import instead. Never fails the install: the per-file path can
    /// succeed where the cache cannot (and reports its own errors when
    /// it can't).
    pub fn try_import<Reporter: self::Reporter>(
        &self,
        logged_methods: &AtomicU8,
        import_method: PackageImportMethod,
        package_key: &PackageKey,
        save_path: &Path,
        cas_paths: &HashMap<String, PathBuf>,
    ) -> bool {
        if self.disabled.load(Ordering::Relaxed) {
            return false;
        }
        // An existing dirent at the target — a completed slot, a
        // partial import to repair, or stale junk — is
        // `import_indexed_dir`'s business; `clonefile` requires a
        // fresh destination.
        match fs::symlink_metadata(save_path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Ok(_) | Err(_) => return false,
        }
        // Only a hash-suffixed canonical slot is content-addressed;
        // a snapshot the layout has no precomputed suffix for goes
        // through the per-file import.
        let Some(slot_dir) = self.layout.hashed_slot_dir(package_key) else {
            return false;
        };
        let canonical_node_modules = slot_dir.join("node_modules");
        let Ok(canonical) =
            safe_join_modules_dir(&canonical_node_modules, &package_key.name.to_string())
        else {
            return false;
        };
        if self.frozen_store {
            if !marker_present(&canonical, cas_paths) {
                return false;
            }
        } else if let Err(error) = crate::import_indexed_dir::<Reporter>(
            logged_methods,
            import_method,
            &canonical,
            cas_paths,
            ImportIndexedDirOpts { safe_to_skip: true, ..ImportIndexedDirOpts::default() },
        ) {
            tracing::debug!(
                target: "pacquet::dir_clone_cache",
                package = %package_key,
                %error,
                "failed to materialize the canonical slot; falling back to the per-file import",
            );
            return false;
        }
        // A scoped package's directory sits below a `@scope/` component
        // the per-file importer would have created; the clone needs the
        // parent in place itself.
        if let Some(parent) = save_path.parent()
            && fs::create_dir_all(parent).is_err()
        {
            return false;
        }
        // `reflink_copy::reflink` is a raw `clonefile(2)`, which clones
        // directories: one syscall for the whole package tree.
        match reflink_copy::reflink(&canonical, save_path) {
            Ok(()) => true,
            Err(error) => {
                // `NotFound` (a concurrent prune removed the canonical
                // slot), `AlreadyExists` (a concurrent importer claimed
                // the target), and `PermissionDenied` are per-call
                // conditions; everything else — `EXDEV`, `ENOTSUP`, and
                // the grab-bag of "this volume can't do that" codes —
                // condemns every later clone too, the same deny-list
                // reasoning as `link_file::is_call_error`.
                if !matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::PermissionDenied
                        | io::ErrorKind::AlreadyExists,
                ) {
                    self.disabled.store(true, Ordering::Relaxed);
                }
                tracing::debug!(
                    target: "pacquet::dir_clone_cache",
                    package = %package_key,
                    %error,
                    "failed to clone the canonical slot; falling back to the per-file import",
                );
                false
            }
        }
    }
}

/// Whether a directory `clonefile` from under `links_root` can land in
/// `virtual_store_dir`: clone an empty probe directory across and
/// remove both. The store side is created if absent (the caller
/// guarantees the store is writable), but nothing is created on the
/// project side — the probe lands in the deepest existing ancestor of
/// the virtual-store dir, which is on the same volume, so a rejected
/// install can still assert that `node_modules/.pnpm` was never
/// written. A stale destination from a crashed probe is removed first
/// so pid reuse can't fail the probe with `EEXIST`.
fn dir_clone_supported(links_root: &Path, virtual_store_dir: &Path) -> bool {
    let probe_name = format!(".pacquet-dir-clone-probe-{}", std::process::id());
    let src = links_root.join(&probe_name);
    if fs::create_dir_all(&src).is_err() {
        return false;
    }
    let dst_parent = deepest_existing_ancestor(virtual_store_dir);
    let dst = dst_parent.join(&probe_name);
    let _ = fs::remove_dir(&dst);
    let supported = reflink_copy::reflink(&src, &dst).is_ok();
    let _ = fs::remove_dir(&src);
    let _ = fs::remove_dir(&dst);
    supported
}

/// `path`, or the nearest ancestor of it that exists on disk. Falls
/// back to `.` for a fully relative path with no existing ancestors.
fn deepest_existing_ancestor(path: &Path) -> &Path {
    let mut candidate = path;
    while !candidate.exists() {
        match candidate.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => candidate = parent,
            _ => return Path::new("."),
        }
    }
    candidate
}

#[cfg(test)]
mod tests;
