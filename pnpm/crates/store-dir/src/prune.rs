//! Prune the global virtual store.
//!
//! Mark-and-sweep over `<store_dir>/links/<scope>/<name>/<version>/<hash>`:
//!
//! 1. **Mark** — walk every registered project (see
//!    [`crate::get_registered_projects`]). For each project, find every
//!    `node_modules/` directory (root + workspace packages), follow every
//!    symlink it contains, and if the symlink target lands under
//!    `<store_dir>/links/...` record the slot path
//!    (`<scope>/<name>/<version>/<hash>`) as reachable. Then recurse
//!    into the slot's own `node_modules/` for transitive deps.
//! 2. **Sweep** — walk the four-level
//!    `<scope>/<name>/<version>/<hash>` tree and remove every `<hash>`
//!    that isn't in the reachable set. Empty `<version>/` and `<name>/`
//!    parents are removed in a second pass.
//!
//! Pacquet doesn't yet have rayon plumbing in the store-dir crate, so
//! the walk runs sequentially. Prune is a one-shot CLI command (not
//! on the hot install path); parallelism can be added later if
//! profiling shows it's worth the complexity.

use crate::{
    GetRegisteredProjectsError, StoreDir, StoreLockError, get_registered_projects,
    prune_cas::PruneCasError,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::HashSet,
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

mod mark;

/// Error type of [`StoreDir::prune`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PruneError {
    #[display("Failed to read CAS loader references: {_0}")]
    #[diagnostic(code(ERR_PNPM_STORE_LOADER_REFERENCES))]
    LoaderReferences(#[error(source)] io::Error),
    #[display("Failed to read the store files that registered projects use: {_0}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_IN_USE_FILES))]
    InUseFiles(#[error(source)] io::Error),
    #[diagnostic(transparent)]
    StoreLock(#[error(source)] StoreLockError),

    /// Surface from the read-side of the project registry — stale
    /// entries that can't be unlinked, inaccessible registry dirs,
    /// or projects whose `stat` returned a permission error.
    #[diagnostic(transparent)]
    ListProjects(#[error(source)] GetRegisteredProjectsError),

    #[display("Failed to remove unreferenced slot at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_REMOVE_SLOT))]
    RemoveSlot {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    /// `read_dir` on a sweep-phase directory
    /// (`<store>/links/<scope>/...`) failed with something other
    /// than `NotFound`. Surfaces because silently treating it as
    /// "empty" would leave unreachable slot directories in place
    /// the next time the prune walker can't see them either.
    #[display("Failed to read sweep directory {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_READ_SWEEP_DIR))]
    ReadSweepDir {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read project directory during mark phase {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_READ_MARK_DIR))]
    ReadMarkDir {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read cache directory {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_READ_CACHE_DIR))]
    ReadCacheDir {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove cached metadata at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_REMOVE_CACHED_METADATA))]
    RemoveCachedMetadata {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    PruneCas(#[error(source)] PruneCasError),
}

impl StoreDir {
    /// Remove unreferenced packages from the global virtual store, the
    /// registry metadata cached under `cache_dir`, and content-addressable
    /// files that have no hard links outside the store. A metadata directory
    /// that is or holds one of `protected_dirs` is kept.
    ///
    /// Pacquet doesn't yet thread the install-time reporter into
    /// store-dir, so the informational messages go to stderr via
    /// `eprintln!` until [#344] lands the proper reporter wiring.
    ///
    /// [#344]: https://github.com/pnpm/pacquet/issues/344
    pub fn prune(&self, cache_dir: &Path, protected_dirs: &[&Path]) -> Result<(), PruneError> {
        self.prune_keeping(cache_dir, protected_dirs, || Ok(HashSet::new()))
    }

    /// [`Self::prune`], keeping every content-addressable file that
    /// `in_use_files` returns whatever its hard-link count. `in_use_files`
    /// runs while the prune holds the store lock, and its error stops the
    /// prune before it removes anything.
    pub fn prune_keeping(
        &self,
        cache_dir: &Path,
        protected_dirs: &[&Path],
        in_use_files: impl FnOnce() -> Result<HashSet<PathBuf>, PruneError>,
    ) -> Result<(), PruneError> {
        self.prune_keeping_with_modules_dirs(
            cache_dir,
            protected_dirs,
            |_| Ok(Vec::new()),
            in_use_files,
        )
    }

    pub fn prune_keeping_with_modules_dirs(
        &self,
        cache_dir: &Path,
        protected_dirs: &[&Path],
        modules_dirs: impl FnOnce(&[PathBuf]) -> Result<Vec<PathBuf>, PruneError>,
        in_use_files: impl FnOnce() -> Result<HashSet<PathBuf>, PruneError>,
    ) -> Result<(), PruneError> {
        let _store_lock = self.lock_for_prune().map_err(PruneError::StoreLock)?;
        let mut references = crate::loader_references::loader_references(self)
            .map_err(PruneError::LoaderReferences)?;
        references.files.extend(in_use_files()?);
        self.prune_global_virtual_store(&references.package_roots, modules_dirs)?;
        self.remove_cached_metadata(cache_dir, protected_dirs)?;
        let stats = crate::prune_cas::prune_cas_with_references(self, &references.files)
            .map_err(PruneError::PruneCas)?;
        pnpm_diagnostics::eprintln!(
            "Removed {} file{} ({} bytes)",
            stats.files,
            if stats.files == 1 { "" } else { "s" },
            stats.bytes,
        );
        pnpm_diagnostics::eprintln!(
            "Removed {} package{}",
            stats.packages,
            if stats.packages == 1 { "" } else { "s" },
        );
        if stats.undecodable_packages > 0 {
            pnpm_diagnostics::eprintln!(
                "Kept {} package index entr{} that could not be read",
                stats.undecodable_packages,
                if stats.undecodable_packages == 1 { "y" } else { "ies" },
            );
        }
        Ok(())
    }

    fn prune_global_virtual_store(
        &self,
        loader_roots: &[PathBuf],
        modules_dirs: impl FnOnce(&[PathBuf]) -> Result<Vec<PathBuf>, PruneError>,
    ) -> Result<(), PruneError> {
        let links_dir = self.links();
        if !path_exists(&links_dir) {
            return Ok(());
        }

        let projects = get_registered_projects(self).map_err(PruneError::ListProjects)?;
        if projects.is_empty() {
            pnpm_diagnostics::eprintln!("No registered projects for global virtual store");
            return Ok(());
        }
        pnpm_diagnostics::eprintln!(
            "Checking {} registered project(s) for global virtual store usage",
            projects.len(),
        );

        let modules_dirs = modules_dirs(&projects)?;
        let reachable =
            mark::mark_reachable_slots(&links_dir, &projects, loader_roots, &modules_dirs)?;
        // Projects without the global virtual store register too, so no link
        // from any registered project leaves the slots' users as unknown as
        // an empty registry does.
        if reachable.is_empty() {
            pnpm_diagnostics::eprintln!("No registered project uses the global virtual store");
            return Ok(());
        }
        let removed = remove_unreachable_packages(&links_dir, &reachable)?;
        if removed > 0 {
            pnpm_diagnostics::eprintln!(
                "Removed {} package{} from global virtual store",
                removed,
                if removed == 1 { "" } else { "s" },
            );
        } else {
            pnpm_diagnostics::eprintln!("No unused packages found in global virtual store");
        }
        Ok(())
    }
}

/// Sweep phase: walk `<links_dir>/<scope>/<name>/<version>/<hash>`
/// and remove every `<hash>` directory whose
/// `<scope>/<name>/<version>/<hash>` path isn't in `reachable`.
/// Cleans up emptied `<version>/`, `<name>/`, and `<scope>/`
/// parents.
fn remove_unreachable_packages(
    links_dir: &Path,
    reachable: &HashSet<PathBuf>,
) -> Result<usize, PruneError> {
    let mut count = 0usize;
    for scope in &list_subdirs(links_dir)? {
        let scope_path = links_dir.join(scope);
        let pkg_names = list_subdirs(&scope_path)?;
        let mut emptied_pkgs = 0;
        for pkg_name in &pkg_names {
            let pkg_dir = scope_path.join(pkg_name);
            let pkg_rel = Path::new(scope).join(pkg_name);
            let (removed_here, all_versions_emptied) =
                remove_unreachable_versions(&pkg_dir, &pkg_rel, reachable)?;
            count += removed_here;
            if all_versions_emptied && remove_empty_dir(&pkg_dir)? {
                emptied_pkgs += 1;
            }
        }
        if emptied_pkgs == pkg_names.len() && !pkg_names.is_empty() {
            remove_empty_dir(&scope_path)?;
        }
    }
    Ok(count)
}

fn remove_unreachable_versions(
    pkg_dir: &Path,
    pkg_rel: &Path,
    reachable: &HashSet<PathBuf>,
) -> Result<(usize, bool), PruneError> {
    let versions = list_subdirs(pkg_dir)?;
    let mut count = 0usize;
    let mut emptied_versions = 0;
    for version in &versions {
        let version_dir = pkg_dir.join(version);
        let (removed_here, all_hashes_removed) =
            remove_unreachable_slots(&version_dir, &pkg_rel.join(version), reachable)?;
        count += removed_here;
        if all_hashes_removed && remove_empty_dir(&version_dir)? {
            emptied_versions += 1;
        }
    }
    Ok((count, emptied_versions == versions.len() && !versions.is_empty()))
}

/// Remove every unreachable `<hash>` slot of one version, reporting how many
/// went and whether that emptied the version directory.
fn remove_unreachable_slots(
    version_dir: &Path,
    version_rel: &Path,
    reachable: &HashSet<PathBuf>,
) -> Result<(usize, bool), PruneError> {
    let hashes = list_subdirs(version_dir)?;
    let mut removed = 0usize;
    for hash in &hashes {
        if reachable.contains(&version_rel.join(hash)) {
            continue;
        }
        remove_slot_dir(&version_dir.join(hash))?;
        removed += 1;
    }
    Ok((removed, removed == hashes.len() && !hashes.is_empty()))
}

/// Returns the names of every directory entry under `dir`, swallowing
/// only `NotFound` (the path raced with a parallel install or the
/// shape just isn't materialised yet) and surfacing other I/O errors
/// as [`PruneError::ReadSweepDir`]. A permission failure here would
/// otherwise mark the entire scope as "no children" and the
/// downstream sweep could leave orphan files in place.
fn list_subdirs(dir: &Path) -> Result<Vec<std::ffi::OsString>, PruneError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(PruneError::ReadSweepDir { path: dir.to_path_buf(), error });
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            out.push(entry.file_name());
        }
    }
    Ok(out)
}

/// Recursively remove an unreferenced slot directory and everything
/// under it (`<store>/links/<scope>/<name>/<version>/<hash>/`). Used
/// for the actual sweep target — that subtree is known unreachable
/// at this point, so a recursive remove is safe.
fn remove_slot_dir(path: &Path) -> Result<(), PruneError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PruneError::RemoveSlot { path: path.to_path_buf(), error }),
    }
}

/// Race-safe parent-cleanup: try to remove `path` as an empty
/// directory and report whether it actually disappeared. Returns
/// `Ok(true)` when the directory was empty and is now gone,
/// `Ok(false)` when it survived because something raced into it
/// (`DirectoryNotEmpty`) or was already missing (`NotFound`), and
/// propagates any other I/O error.
///
/// Pacquet deliberately diverges from pnpm here. pnpm recursively
/// removes the empty `<version>/`, `<name>/`, and `<scope>/` parents —
/// a concurrent install that materialises a fresh slot in the window
/// between [`list_subdirs`] and the parent cleanup would have its
/// just-written tree wiped by that recursive remove. Using
/// `fs::remove_dir` keeps pacquet race-safe (the new slot stays;
/// only the parent that's truly empty is removed) while producing
/// the same on-disk result in the non-race case.
fn remove_empty_dir(path: &Path) -> Result<bool, PruneError> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) =>
        {
            Ok(false)
        }
        Err(error) => Err(PruneError::RemoveSlot { path: path.to_path_buf(), error }),
    }
}

fn path_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

#[cfg(test)]
mod tests;
