//! Pacquet port of upstream pnpm's
//! [`pruneGlobalVirtualStore`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts).
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
//! the port walks sequentially. Prune is a one-shot CLI command (not
//! on the hot install path); parallelism can be added later if
//! profiling shows it's worth the complexity. The shape mirrors
//! upstream's `Promise.all`-driven layout so the parallelism graft
//! is mechanical when it lands.

use crate::{GetRegisteredProjectsError, StoreDir, get_registered_projects};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::read_symlink_dir;
use std::{
    collections::HashSet,
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// Error type of [`StoreDir::prune`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PruneError {
    /// Surface from the read-side of the project registry — stale
    /// entries that can't be unlinked, inaccessible registry dirs,
    /// or projects whose `stat` returned a permission error.
    #[diagnostic(transparent)]
    ListProjects(#[error(source)] GetRegisteredProjectsError),

    #[display("Failed to remove unreferenced slot at {path:?}: {error}")]
    #[diagnostic(code(pacquet_store_dir::prune::remove_slot))]
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
    #[diagnostic(code(pacquet_store_dir::prune::read_sweep_dir))]
    ReadSweepDir {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

impl StoreDir {
    /// Remove unreferenced packages from the global virtual store at
    /// `<store_dir>/links`. Mirrors upstream's
    /// [`pruneGlobalVirtualStore`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L21-L58).
    ///
    /// Behaviour:
    /// - No `links/` directory yet: silent no-op (matches upstream's
    ///   `if (!await pathExists(linksDir)) return`).
    /// - No registered projects: prints `pnpm`'s informational message
    ///   and returns. Pacquet doesn't yet thread the install-time
    ///   reporter into store-dir, so the message goes to stderr via
    ///   `eprintln!` until [#344] lands the proper reporter wiring.
    /// - Otherwise: mark-and-sweep as documented in the module-level
    ///   comment. The removed-slot count is reported to stderr to
    ///   match upstream's `globalInfo("Removed N package(s) ...")`
    ///   message — the function itself returns `()` like upstream's
    ///   `Promise<void>` shape.
    ///
    /// Returns `Ok(())` on success; surfaces I/O errors from the mark
    /// or sweep walks as [`PruneError`]. Stale registry entries are
    /// healed transparently by [`crate::get_registered_projects`].
    ///
    /// [#344]: https://github.com/pnpm/pacquet/issues/344
    pub fn prune(&self) -> Result<(), PruneError> {
        let links_dir = self.links();
        if !path_exists(&links_dir) {
            return Ok(());
        }

        let projects = get_registered_projects(self).map_err(PruneError::ListProjects)?;
        if projects.is_empty() {
            eprintln!("No registered projects for global virtual store");
            return Ok(());
        }
        eprintln!(
            "Checking {} registered project(s) for global virtual store usage",
            projects.len(),
        );

        // Canonicalize the links root once and pass it down. The
        // mark walk compares every target's canonical form against
        // this root, and canonicalising inside the per-entry loop
        // would burn one extra syscall per visited symlink — wasteful
        // on large trees where the answer is invariant.
        let canonical_links = dunce::canonicalize(&links_dir).unwrap_or_else(|_| links_dir.clone());
        let mut reachable: HashSet<PathBuf> = HashSet::new();
        let mut visited: HashSet<PathBuf> = HashSet::new();
        for project_dir in &projects {
            for modules_dir in find_all_node_modules_dirs(project_dir) {
                walk_symlinks_to_store(
                    &modules_dir,
                    &canonical_links,
                    &mut reachable,
                    &mut visited,
                );
            }
        }

        let removed = remove_unreachable_packages(&links_dir, &reachable)?;
        if removed > 0 {
            eprintln!(
                "Removed {} package{} from global virtual store",
                removed,
                if removed == 1 { "" } else { "s" },
            );
        } else {
            eprintln!("No unused packages found in global virtual store");
        }
        Ok(())
    }
}

/// Find every `node_modules/` directory under `project_dir`,
/// including those inside workspace packages. Mirrors upstream's
/// [`findAllNodeModulesDirs`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L64-L97):
/// descends into every non-hidden subdir until it sees `node_modules`,
/// at which point it records the path and stops descending — the
/// hoisted deps inside `node_modules/.pnpm` and friends are picked up
/// by `walk_symlinks_to_store`'s transitive recursion instead.
fn find_all_node_modules_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    scan(project_dir, &mut out);
    return out;

    fn scan(dir: &Path, out: &mut Vec<PathBuf>) {
        // Swallow every `read_dir` error — matches upstream's
        // [`scan`'s bare `catch { return }`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L67-L73).
        // A permission failure inside a workspace package would make
        // `prune` over-aggressive (its node_modules wouldn't be
        // marked), but tightening this without an upstream change
        // would diverge from pnpm's behaviour — and `pacquet store
        // prune` shares a store directory with `pnpm store prune`,
        // so the two must agree on what counts as reachable.
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let entry_path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "node_modules" {
                out.push(entry_path);
                // Don't descend into node_modules
            } else if !name_str.starts_with('.') {
                subdirs.push(entry_path);
            }
        }
        for sub in subdirs {
            scan(&sub, out);
        }
    }
}

/// Recursively follow every symlink under `dir`. When a symlink
/// resolves to a slot under `canonical_links`, record the slot's
/// `<scope>/<name>/<version>/<hash>` segment in `reachable` and
/// recurse into the slot's `node_modules/` for transitive deps.
/// Mirrors upstream's
/// [`walkSymlinksToStore`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L103-L163).
///
/// `canonical_links` must already be the canonicalised links root
/// — [`StoreDir::prune`] does this once and threads it through, so
/// the per-entry loop doesn't pay a `canonicalize` syscall for an
/// invariant value.
///
/// `visited` is the cycle guard, keyed by the canonical (real) path
/// of `dir`. Upstream uses a sha256-base64url hash of the realpath;
/// in pacquet we can store the canonical `PathBuf` directly — the
/// extra hashing only helps when serialising the set, which we
/// don't.
fn walk_symlinks_to_store(
    dir: &Path,
    canonical_links: &Path,
    reachable: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) {
    let canonical_dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical_dir) {
        return;
    }

    // Swallow every `read_dir` error — matches upstream's
    // [`walkSymlinksToStore`'s bare `catch { return }`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L116-L121).
    // Same caveat as in [`find_all_node_modules_dirs`]: tightening
    // this would diverge from pnpm and risk a `pacquet store
    // prune` deciding more slots are unreachable than a parallel
    // `pnpm store prune` would.
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_symlink() {
            // `read_symlink_dir` handles Windows junctions (which
            // `pacquet_fs::symlink_dir` creates for every
            // `node_modules/<pkg>` entry); plain `fs::read_link`
            // would EINVAL on them and the mark walk would miss
            // every direct dep on Windows. See
            // [`rust-lang/rust#28528`](https://github.com/rust-lang/rust/issues/28528).
            let Ok(target) = read_symlink_dir(&entry_path) else {
                continue;
            };
            let absolute_target = if target.is_absolute() {
                target
            } else {
                entry_path.parent().map(|p| p.join(&target)).unwrap_or(target)
            };
            // Canonicalise the target so a symlink-bearing path
            // prefix doesn't fool the `starts_with` check against
            // the (already-canonical) links root.
            let canonical_target =
                dunce::canonicalize(&absolute_target).unwrap_or_else(|_| absolute_target.clone());
            if !canonical_target.starts_with(canonical_links) {
                continue;
            }
            // Slot path is the segment after `canonical_links` up to
            // (but excluding) the first `node_modules` component.
            // Layout:
            //   <links>/<scope>/<name>/<version>/<hash>/node_modules/<pkg>
            // We want `<scope>/<name>/<version>/<hash>`.
            let Ok(rel) = canonical_target.strip_prefix(canonical_links) else {
                continue;
            };
            let parts: Vec<_> = rel.components().collect();
            let nm_idx = parts
                .iter()
                .position(|comp| comp.as_os_str() == std::ffi::OsStr::new("node_modules"));
            if let Some(idx) = nm_idx {
                let slot: PathBuf = parts[..idx].iter().collect();
                reachable.insert(slot.clone());
                // Recurse into the slot's own node_modules for
                // transitive deps.
                let inner_modules = canonical_links.join(&slot).join("node_modules");
                walk_symlinks_to_store(&inner_modules, canonical_links, reachable, visited);
            }
        } else if file_type.is_dir() {
            // Skip `.pnpm` — that's the project-local virtual store.
            // The slots we want are reached *through* `.pnpm`'s
            // symlinks, not by descending into it directly. (When
            // GVS is on, `.pnpm` may also be absent, in which case
            // the skip is a no-op.)
            let name = entry.file_name();
            if name.to_string_lossy() == ".pnpm" {
                continue;
            }
            walk_symlinks_to_store(&entry_path, canonical_links, reachable, visited);
        }
    }
}

/// Sweep phase: walk `<links_dir>/<scope>/<name>/<version>/<hash>`
/// and remove every `<hash>` directory whose
/// `<scope>/<name>/<version>/<hash>` path isn't in `reachable`.
/// Cleans up emptied `<version>/`, `<name>/`, and `<scope>/`
/// parents. Mirrors upstream's
/// [`removeUnreachablePackages`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L187-L226)
/// + [`removeUnreachableVersions`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L232-L271).
fn remove_unreachable_packages(
    links_dir: &Path,
    reachable: &HashSet<PathBuf>,
) -> Result<usize, PruneError> {
    let mut count = 0usize;
    let scopes = list_subdirs(links_dir)?;
    for scope in &scopes {
        let scope_path = links_dir.join(scope);
        let pkg_names = list_subdirs(&scope_path)?;
        let mut emptied_pkgs = 0;
        for pkg_name in &pkg_names {
            let pkg_dir = scope_path.join(pkg_name);
            let pkg_rel = Path::new(scope).join(pkg_name);
            let (removed_here, all_versions_emptied) =
                remove_unreachable_versions(&pkg_dir, &pkg_rel, reachable)?;
            count += removed_here;
            if all_versions_emptied {
                // Every version under this pkg was emptied — try to
                // drop the now-empty `<name>/` parent. Race-safe
                // remove: a concurrent install that just materialised
                // a fresh version dir here keeps its work.
                if remove_empty_dir(&pkg_dir)? {
                    emptied_pkgs += 1;
                }
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
        let hashes = list_subdirs(&version_dir)?;
        let mut removed_hashes = 0;
        for hash in &hashes {
            let slot_rel = pkg_rel.join(version).join(hash);
            if !reachable.contains(&slot_rel) {
                let slot_dir = version_dir.join(hash);
                // The slot subtree is unreferenced — recursive
                // remove of its files is correct.
                remove_slot_dir(&slot_dir)?;
                removed_hashes += 1;
                count += 1;
            }
        }
        if removed_hashes == hashes.len() && !hashes.is_empty() {
            // Try to drop the `<version>/` parent only if it's
            // genuinely empty after the slot removals. A concurrent
            // install that just landed a new hash dir here survives.
            if remove_empty_dir(&version_dir)? {
                emptied_versions += 1;
            }
        }
    }
    Ok((count, emptied_versions == versions.len() && !versions.is_empty()))
}

/// Mirrors upstream's
/// [`getSubdirsSafely`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L282-L299):
/// returns the names of every directory entry under `dir`, swallowing
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
/// Pacquet deliberately diverges from upstream here. Upstream uses
/// [`rimraf`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/pruneGlobalVirtualStore.ts#L210-L223)
/// on the empty `<version>/`, `<name>/`, and `<scope>/` parents — a
/// concurrent install that materialises a fresh slot in the window
/// between `list_subdirs` and the parent cleanup would have its
/// just-written tree wiped by upstream's recursive remove. Switching
/// to `fs::remove_dir` keeps pacquet race-safe (the new slot stays;
/// only the parent that's truly empty is removed) while producing
/// the same on-disk result in the non-race case. Slot directories
/// themselves still go through [`remove_slot_dir`] — those are
/// known-unreferenced by the time prune reaches them, so recursive
/// removal is correct.
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
