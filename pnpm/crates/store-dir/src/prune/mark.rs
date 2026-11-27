use super::PruneError;
use pnpm_fs::{
    is_non_link_read_error,
    read_symlink_dir,
};
use std::{
    collections::HashSet,
    fs,
    io::{
        self,
        ErrorKind,
    },
    path::{
        Path,
        PathBuf,
    },
};

pub(super) trait FsReadLink {
    fn read_link(path: &Path) -> io::Result<PathBuf>;
}

pub(super) struct Host;

impl FsReadLink for Host {
    fn read_link(path: &Path) -> io::Result<PathBuf> {
        read_symlink_dir(path)
    }
}

/// Every `<store_dir>/links` slot the registered projects link into.
pub(super) fn mark_reachable_slots<Sys: FsReadLink>(
    links_dir: &Path,
    projects: &[PathBuf],
    loader_roots: &[PathBuf],
    modules_dirs: &[PathBuf],
) -> Result<HashSet<PathBuf>, PruneError> {
    // Canonicalize the links root once and pass it down. The
    // mark walk compares every target's canonical form against
    // this root, and canonicalising inside the per-entry loop
    // would burn one extra syscall per visited symlink, wasteful
    // on large trees where the answer is invariant.
    let canonical_links =
        dunce::canonicalize(links_dir).unwrap_or_else(|_| links_dir.to_path_buf());
    let mut walk = Walk {
        canonical_links: &canonical_links,
        reachable: HashSet::new(),
        visited: HashSet::new(),
    };
    mark_loader_roots::<Sys>(
        loader_roots,
        &canonical_links,
        &mut walk.reachable,
        &mut walk.visited,
    )?;
    for project_dir in projects {
        walk.mark_project::<Sys>(project_dir)?;
    }
    for modules_dir in modules_dirs {
        walk.walk::<Sys>(modules_dir)?;
    }
    Ok(walk.reachable)
}

fn mark_loader_roots<Sys: FsReadLink>(
    loader_roots: &[PathBuf],
    canonical_links: &Path,
    reachable: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), PruneError> {
    for root in loader_roots {
        if let Some(slot) = store_slot_from_target(root, canonical_links) {
            let directory = canonical_links.join(&slot).join("node_modules");
            reachable.insert(slot);
            walk_symlinks_to_store::<Sys>(&directory, canonical_links, reachable, visited)?;
        }
    }
    Ok(())
}

struct Walk<'links> {
    canonical_links: &'links Path,
    reachable: HashSet<PathBuf>,
    visited: HashSet<PathBuf>,
}

impl Walk<'_> {
    /// Mark the slots `project_dir` links to. Every `node_modules/` directory
    /// under it, including those of workspace packages, is walked by
    /// [`walk_symlinks_to_store`]. A store link anywhere else in a non-hidden
    /// directory is followed too, so a modules directory with another name
    /// keeps its slots even when no setting records that name.
    fn mark_project<Sys: FsReadLink>(&mut self, project_dir: &Path) -> Result<(), PruneError> {
        let project =
            dunce::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
        self.scan_project_dir::<Sys>(&project)
    }

    fn scan_project_dir<Sys: FsReadLink>(&mut self, dir: &Path) -> Result<(), PruneError> {
        let store_root = self.canonical_links.parent().expect("links directory has a store root");
        let ProjectEntries { subdirs, symlinks } = project_entries(dir, store_root)?;
        for symlink in symlinks {
            if let Some(slot) = linked_store_slot::<Sys>(&symlink, self.canonical_links)? {
                let slot_modules = self.canonical_links.join(&slot).join("node_modules");
                self.reachable.insert(slot);
                self.walk::<Sys>(&slot_modules)?;
            }
        }
        for subdir in subdirs {
            let name = subdir
                .file_name()
                .expect("directory entry has a name")
                .to_string_lossy();
            if name == "node_modules" {
                self.walk::<Sys>(&subdir)?;
            } else if !name.starts_with('.') {
                self.scan_project_dir::<Sys>(&subdir)?;
            }
        }
        Ok(())
    }

    fn walk<Sys: FsReadLink>(&mut self, dir: &Path) -> Result<(), PruneError> {
        walk_symlinks_to_store::<Sys>(
            dir,
            self.canonical_links,
            &mut self.reachable,
            &mut self.visited,
        )
    }
}

/// The directories and symlinks directly inside a project directory.
struct ProjectEntries {
    subdirs: Vec<PathBuf>,
    symlinks: Vec<PathBuf>,
}

fn project_entries(dir: &Path, excluded_store: &Path) -> Result<ProjectEntries, PruneError> {
    let mut found = ProjectEntries { subdirs: Vec::new(), symlinks: Vec::new() };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(found);
        }
        Err(error) => return Err(PruneError::ReadMarkDir { path: dir.to_path_buf(), error }),
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| PruneError::ReadMarkDir { path: dir.to_path_buf(), error })?;
        let file_type = entry
            .file_type()
            .map_err(|error| PruneError::ReadMarkDir { path: entry.path(), error })?;
        let entry_path = entry.path();
        if file_type.is_symlink() {
            found.symlinks.push(entry_path);
        } else if file_type.is_dir() && entry_path != excluded_store {
            found.subdirs.push(entry_path);
        }
    }
    Ok(found)
}

/// Recursively follow every symlink under `dir`. When a symlink
/// resolves to a slot under `canonical_links`, record the slot's
/// `<scope>/<name>/<version>/<hash>` segment in `reachable` and
/// recurse into the slot's `node_modules/` for transitive deps.
///
/// `canonical_links` must already be the canonicalised links root.
///
/// `visited` is the cycle guard, keyed by the canonical (real) path
/// of `dir`. Storing the canonical `PathBuf` directly is enough, the
/// set is never serialised, so there's no need to hash the realpath
/// first.
fn walk_symlinks_to_store<Sys: FsReadLink>(
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
        if let Some(next_dir) = next_walk_dir::<Sys>(&entry, canonical_links, reachable)? {
            walk_symlinks_to_store::<Sys>(&next_dir, canonical_links, reachable, visited)?;
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
fn next_walk_dir<Sys: FsReadLink>(
    entry: &fs::DirEntry,
    canonical_links: &Path,
    reachable: &mut HashSet<PathBuf>,
) -> Result<Option<PathBuf>, PruneError> {
    let file_type = entry
        .file_type()
        .map_err(|error| PruneError::ReadMarkDir { path: entry.path(), error })?;
    let entry_path = entry.path();
    if file_type.is_symlink() {
        let Some(slot) = linked_store_slot::<Sys>(&entry_path, canonical_links)? else {
            return Ok(None);
        };
        let inner_modules = canonical_links.join(&slot).join("node_modules");
        reachable.insert(slot);
        return Ok(Some(inner_modules));
    }
    if file_type.is_dir()
        && entry.file_name().to_string_lossy() != ".pnpm"
        && canonical_links.parent() != Some(entry_path.as_path())
    {
        return Ok(Some(entry_path));
    }
    Ok(None)
}

/// The `<scope>/<name>/<version>/<hash>` slot a `node_modules` symlink
/// points at, or `None` when it leads somewhere else.
fn linked_store_slot<Sys: FsReadLink>(
    entry_path: &Path,
    canonical_links: &Path,
) -> Result<Option<PathBuf>, PruneError> {
    let target = match Sys::read_link(entry_path) {
        Ok(target) => target,
        Err(error) if error.kind() == ErrorKind::NotFound || is_non_link_read_error(&error) => {
            return Ok(None);
        }
        Err(error) => {
            return Err(PruneError::ReadMarkLink { path: entry_path.to_path_buf(), error });
        }
    };
    let absolute_target = if target.is_absolute() {
        target
    } else {
        entry_path
            .parent()
            .map(|parent| parent.join(&target))
            .unwrap_or(target)
    };
    Ok(store_slot_from_target(&absolute_target, canonical_links))
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
