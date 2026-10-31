use super::PruneError;
use pnpm_fs::read_symlink_dir;
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// Every `<store_dir>/links` slot the registered projects link into.
pub(super) fn mark_reachable_slots(
    links_dir: &Path,
    projects: &[PathBuf],
    loader_roots: &[PathBuf],
    modules_dirs: &[PathBuf],
) -> Result<HashSet<PathBuf>, PruneError> {
    // Canonicalize the links root once and pass it down. The
    // mark walk compares every target's canonical form against
    // this root, and canonicalising inside the per-entry loop
    // would burn one extra syscall per visited symlink — wasteful
    // on large trees where the answer is invariant.
    let canonical_links =
        dunce::canonicalize(links_dir).unwrap_or_else(|_| links_dir.to_path_buf());
    let mut reachable: HashSet<PathBuf> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    mark_loader_roots(loader_roots, &canonical_links, &mut reachable, &mut visited)?;
    for project_dir in projects {
        for modules_dir in find_all_node_modules_dirs(
            project_dir,
            canonical_links.parent().expect("links directory has a store root"),
        )? {
            walk_symlinks_to_store(&modules_dir, &canonical_links, &mut reachable, &mut visited)?;
        }
    }
    for modules_dir in modules_dirs {
        walk_symlinks_to_store(modules_dir, &canonical_links, &mut reachable, &mut visited)?;
    }
    Ok(reachable)
}

fn mark_loader_roots(
    loader_roots: &[PathBuf],
    canonical_links: &Path,
    reachable: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), PruneError> {
    for root in loader_roots {
        if let Some(slot) = store_slot_from_target(root, canonical_links) {
            let directory = canonical_links.join(&slot).join("node_modules");
            reachable.insert(slot);
            walk_symlinks_to_store(&directory, canonical_links, reachable, visited)?;
        }
    }
    Ok(())
}

/// Find every `node_modules/` directory under `project_dir`,
/// including those inside workspace packages. Descends into every
/// non-hidden subdir until it sees `node_modules`, at which point it
/// records the path and stops descending — the
/// hoisted deps inside `node_modules/.pnpm` and friends are picked up
/// by [`walk_symlinks_to_store`]'s transitive recursion instead.
fn find_all_node_modules_dirs(
    project_dir: &Path,
    excluded_store: &Path,
) -> Result<Vec<PathBuf>, PruneError> {
    let mut out = Vec::new();
    let project = dunce::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
    scan_node_modules_dirs(&project, &mut out, excluded_store)?;
    Ok(out)
}

fn scan_node_modules_dirs(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    excluded_store: &Path,
) -> Result<(), PruneError> {
    for subdir in project_subdirectories(dir, excluded_store)? {
        let name = subdir
            .file_name()
            .expect("directory entry has a name")
            .to_string_lossy();
        if name == "node_modules" {
            out.push(subdir);
        } else if !name.starts_with('.') {
            scan_node_modules_dirs(&subdir, out, excluded_store)?;
        }
    }
    Ok(())
}

fn project_subdirectories(dir: &Path, excluded_store: &Path) -> Result<Vec<PathBuf>, PruneError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(PruneError::ReadMarkDir { path: dir.to_path_buf(), error }),
    };
    let mut subdirs = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| PruneError::ReadMarkDir { path: dir.to_path_buf(), error })?;
        let file_type = entry
            .file_type()
            .map_err(|error| PruneError::ReadMarkDir { path: entry.path(), error })?;
        let entry_path = entry.path();
        if file_type.is_dir() && entry_path != excluded_store {
            subdirs.push(entry_path);
        }
    }
    Ok(subdirs)
}

/// Recursively follow every symlink under `dir`. When a symlink
/// resolves to a slot under `canonical_links`, record the slot's
/// `<scope>/<name>/<version>/<hash>` segment in `reachable` and
/// recurse into the slot's `node_modules/` for transitive deps.
///
/// `canonical_links` must already be the canonicalised links root.
///
/// `visited` is the cycle guard, keyed by the canonical (real) path
/// of `dir`. Storing the canonical `PathBuf` directly is enough — the
/// set is never serialised, so there's no need to hash the realpath
/// first.
fn walk_symlinks_to_store(
    dir: &Path,
    canonical_links: &Path,
    reachable: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), PruneError> {
    let canonical_dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical_dir) {
        return Ok(());
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(());
        }
        Err(error) => {
            return Err(PruneError::ReadMarkDir { path: dir.to_path_buf(), error });
        }
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| PruneError::ReadMarkDir { path: dir.to_path_buf(), error })?;
        if let Some(next_dir) = next_walk_dir(&entry, canonical_links, reachable) {
            walk_symlinks_to_store(&next_dir, canonical_links, reachable, visited)?;
        }
    }
    Ok(())
}

/// The directory to descend into for one entry: the store slot a symlink
/// resolves to (recorded as reachable on the way), or a plain subdirectory.
///
/// `.pnpm` is the project-local virtual store, so it is skipped: the slots
/// we want are reached *through* its symlinks, not by descending into it
/// directly. (When GVS is on, `.pnpm` may also be absent, in which case the
/// skip is a no-op.)
fn next_walk_dir(
    entry: &fs::DirEntry,
    canonical_links: &Path,
    reachable: &mut HashSet<PathBuf>,
) -> Option<PathBuf> {
    let file_type = entry.file_type().ok()?;
    let entry_path = entry.path();
    if file_type.is_symlink() {
        let slot = linked_store_slot(&entry_path, canonical_links)?;
        let inner_modules = canonical_links.join(&slot).join("node_modules");
        reachable.insert(slot);
        return Some(inner_modules);
    }
    if file_type.is_dir()
        && entry.file_name().to_string_lossy() != ".pnpm"
        && canonical_links.parent() != Some(entry_path.as_path())
    {
        return Some(entry_path);
    }
    None
}

/// The `<scope>/<name>/<version>/<hash>` slot a `node_modules` symlink
/// points at, or `None` when it leads somewhere else.
fn linked_store_slot(entry_path: &Path, canonical_links: &Path) -> Option<PathBuf> {
    let target = read_symlink_dir(entry_path).ok()?;
    let absolute_target = if target.is_absolute() {
        target
    } else {
        entry_path
            .parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target)
    };
    store_slot_from_target(&absolute_target, canonical_links)
}

pub(super) fn store_slot_from_target(
    absolute_target: &Path,
    canonical_links: &Path,
) -> Option<PathBuf> {
    // Canonicalise the target so a symlink-bearing path prefix doesn't fool
    // the `starts_with` check against the (already-canonical) links root.
    let canonical_target =
        dunce::canonicalize(absolute_target).unwrap_or_else(|_| absolute_target.to_path_buf());

    // Slot path is the segment after `canonical_links` up to (but excluding)
    // the first `node_modules` component. Layout:
    //   <links>/<scope>/<name>/<version>/<hash>/node_modules/<pkg>
    // We want `<scope>/<name>/<version>/<hash>`.
    let rel = canonical_target.strip_prefix(canonical_links).ok()?;
    let parts: Vec<_> = rel.components().collect();
    if parts
        .iter()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    let node_modules = parts
        .iter()
        .position(|component| component.as_os_str() == std::ffi::OsStr::new("node_modules"))?;
    (node_modules > 0).then(|| parts[..node_modules].iter().collect())
}
