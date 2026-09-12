use super::super::CreateVirtualStoreError;
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::path::Path;

/// Whether every child link the symlink layout would create for the
/// snapshot's regular `dependencies` is present in the slot. Mirrors
/// [`crate::create_symlink_layout()`]'s predicate: the slot's own name
/// never gets a link, a `link:` child is linked only when the layout
/// knows the lockfile dir, and a skipped target gets no link. Children
/// the layout would not create are not required to be absent — a
/// shared slot may carry links written by an install with a different
/// skip set, and re-importing would not remove them, so requiring
/// absence would re-materialize the slot on every install.
///
/// Presence is an lstat: the layout creates each symlink regardless of
/// whether its target slot is materialized yet.
pub(super) fn regular_children_match(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
) -> Result<bool, CreateVirtualStoreError> {
    if !link_dependencies {
        return Ok(true);
    }
    let Some(dependencies) = snapshot.dependencies.as_ref() else {
        return Ok(true);
    };
    let modules_dir = layout.slot_dir(snapshot_key).join("node_modules");
    for (alias, dep_ref) in dependencies {
        if alias == &snapshot_key.name || !layout_links_child(alias, dep_ref, layout, skipped) {
            continue;
        }
        if !regular_child_present(&modules_dir, alias)? {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Whether the symlink layout writes a link for `alias` inside the
/// snapshot's slot: a resolved child is linked unless the installability
/// pass skipped it, and a `link:` child only when the layout knows the
/// lockfile dir.
pub(super) fn layout_links_child(
    alias: &pnpm_lockfile::PkgName,
    dep_ref: &pnpm_lockfile::SnapshotDepRef,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
) -> bool {
    if let Some(target) = dep_ref.resolve(alias) {
        !skipped.contains(&target)
    } else {
        dep_ref.as_link_target().is_some() && layout.lockfile_dir().is_some()
    }
}
/// Whether the slot carries the link the symlink layout writes for
/// `alias`. An alias that cannot be joined onto the slot's
/// `node_modules` is reported as absent: the layout would have refused
/// to create it too, so the slot can never satisfy the probe.
pub(super) fn regular_child_present(
    modules_dir: &Path,
    alias: &pnpm_lockfile::PkgName,
) -> Result<bool, CreateVirtualStoreError> {
    let Ok(child_path) =
        crate::safe_join_modules_dir::safe_join_modules_dir(modules_dir, &alias.to_string())
    else {
        return Ok(false);
    };
    child_link_present(&child_path)
}
/// Whether `child_path` holds the directory link the symlink layout
/// writes. A plain file or directory in its place is a corrupted slot,
/// not a link, so it does not count.
pub(super) fn child_link_present(child_path: &Path) -> Result<bool, CreateVirtualStoreError> {
    match std::fs::symlink_metadata(child_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Ok(true);
            }
            // On Windows `symlink_dir` may have fallen back to a
            // junction, which `is_symlink` does not report.
            #[cfg(windows)]
            return pnpm_fs::is_symlink_or_junction(child_path).map_err(|error| {
                CreateVirtualStoreError::InspectVirtualStoreSlot {
                    path: child_path.to_path_buf(),
                    error,
                }
            });
            #[cfg(not(windows))]
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CreateVirtualStoreError::InspectVirtualStoreSlot {
            path: child_path.to_path_buf(),
            error,
        }),
    }
}
/// Kind of dirent a slot probe expects.
#[derive(Clone, Copy)]
pub(super) enum EntryKind {
    Dir,
    File,
}
/// Whether `path` exists as the expected kind. `NotFound` — and
/// `NotADirectory`, a file sitting where a parent directory is
/// expected — mean the slot is not there; any other inspection error
/// aborts the install, per the warm-slot fold's contract.
pub(super) fn probe_slot_entry(
    path: &Path,
    kind: EntryKind,
) -> Result<bool, CreateVirtualStoreError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(match kind {
            EntryKind::Dir => metadata.is_dir(),
            EntryKind::File => metadata.is_file(),
        }),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory,
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(CreateVirtualStoreError::InspectVirtualStoreSlot {
            path: path.to_path_buf(),
            error,
        }),
    }
}
pub(super) fn optional_children_match(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
    include_optional_dependencies: bool,
) -> Result<bool, CreateVirtualStoreError> {
    optional_children_match_with(
        snapshot_key,
        snapshot,
        layout,
        skipped,
        link_dependencies,
        include_optional_dependencies,
        optional_child_matches,
    )
}
pub(super) fn optional_child_matches(
    child_path: &Path,
    should_exist: bool,
) -> std::io::Result<bool> {
    if should_exist {
        return match std::fs::metadata(child_path) {
            Ok(metadata) => Ok(metadata.is_dir()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        };
    }
    match std::fs::symlink_metadata(child_path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}
pub(super) fn optional_children_match_with(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
    include_optional_dependencies: bool,
    mut child_matches: impl FnMut(&Path, bool) -> std::io::Result<bool>,
) -> Result<bool, CreateVirtualStoreError> {
    let Some(optional_dependencies) = snapshot.optional_dependencies.as_ref() else {
        return Ok(true);
    };
    let modules_dir = layout.slot_dir(snapshot_key).join("node_modules");
    for (alias, dep_ref) in optional_dependencies {
        if alias == &snapshot_key.name {
            continue;
        }
        let Ok(child_path) =
            crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, &alias.to_string())
        else {
            return Ok(false);
        };
        let should_exist = link_dependencies
            && include_optional_dependencies
            && layout_links_child(alias, dep_ref, layout, skipped);
        let matches = child_matches(&child_path, should_exist).map_err(|error| {
            CreateVirtualStoreError::InspectOptionalDependency { path: child_path.clone(), error }
        })?;
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}
