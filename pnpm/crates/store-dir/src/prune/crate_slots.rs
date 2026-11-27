//! Remove the Cargo package slots no registered workspace links. See
//! [`crate::crate_slots`] for the layout.

use super::{
    PruneError,
    list_subdirs,
    remove_empty_dir,
    remove_slot_dir,
};
use crate::crate_slots::{
    CARGO_GIT_LINK_DIR,
    CARGO_REGISTRY_LINK_DIR,
    slot_of_created_record,
};
use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{
        Path,
        PathBuf,
    },
};

/// Remove every slot under `crates_dir` that has a created record and that
/// no workspace in `projects` links. Returns how many slots went.
pub(super) fn remove_unlinked_slots(
    crates_dir: &Path,
    projects: &[PathBuf],
) -> Result<usize, PruneError> {
    if !super::path_exists(crates_dir) {
        return Ok(0);
    }
    let linked = linked_slots(crates_dir, projects)?;
    let mut removed = 0;
    for name in list_subdirs(crates_dir)? {
        let name_dir = crates_dir.join(name);
        for version in list_subdirs(&name_dir)? {
            let version_dir = name_dir.join(version);
            removed += remove_unlinked_version_slots(&version_dir, &linked)?;
            remove_empty_dir(&version_dir)?;
        }
        remove_empty_dir(&name_dir)?;
    }
    Ok(removed)
}

/// The canonical paths of the slots the workspaces in `projects` link.
fn linked_slots(crates_dir: &Path, projects: &[PathBuf]) -> Result<HashSet<PathBuf>, PruneError> {
    let canonical_crates = dunce::canonicalize(crates_dir)
        .map_err(|error| PruneError::ReadSweepDir { path: crates_dir.to_path_buf(), error })?;
    let mut linked = HashSet::new();
    for project in projects {
        for link_dir in [CARGO_REGISTRY_LINK_DIR, CARGO_GIT_LINK_DIR] {
            let link_dir = link_dir
                .iter()
                .fold(project.clone(), |dir, part| dir.join(part));
            add_linked_slots(&link_dir, &canonical_crates, &mut linked)?;
        }
    }
    Ok(linked)
}

fn add_linked_slots(
    link_dir: &Path,
    canonical_crates: &Path,
    linked: &mut HashSet<PathBuf>,
) -> Result<(), PruneError> {
    let entries = match fs::read_dir(link_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(PruneError::ReadMarkDir { path: link_dir.to_path_buf(), error }),
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| PruneError::ReadMarkDir { path: link_dir.to_path_buf(), error })?;
        let link = entry.path();
        let target = match dunce::canonicalize(&link) {
            Ok(target) => target,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(PruneError::ReadMarkLink { path: link, error }),
        };
        if target.starts_with(canonical_crates) {
            linked.insert(target);
        }
    }
    Ok(())
}

/// Remove the slots of one version that have a created record and are not in
/// `linked`, and every created record whose slot is gone.
fn remove_unlinked_version_slots(
    version_dir: &Path,
    linked: &HashSet<PathBuf>,
) -> Result<usize, PruneError> {
    let canonical_version_dir = dunce::canonicalize(version_dir)
        .map_err(|error| PruneError::ReadSweepDir { path: version_dir.to_path_buf(), error })?;
    let mut removed = 0;
    for record in created_records(version_dir)? {
        let Some(slot) = slot_of_created_record(&record) else {
            continue;
        };
        let slot_name = slot.file_name().expect("a slot has a file name");
        if linked.contains(&canonical_version_dir.join(slot_name)) {
            continue;
        }
        if super::path_exists(&slot) {
            remove_slot_dir(&slot)?;
            removed += 1;
        }
        remove_file(&record)?;
    }
    Ok(removed)
}

fn created_records(version_dir: &Path) -> Result<Vec<PathBuf>, PruneError> {
    let entries = match fs::read_dir(version_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(PruneError::ReadSweepDir { path: version_dir.to_path_buf(), error });
        }
    };
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| PruneError::ReadSweepDir {
            path: version_dir.to_path_buf(),
            error,
        })?;
        let path = entry.path();
        if entry.file_type().is_ok_and(|file_type| file_type.is_file())
            && slot_of_created_record(&path).is_some()
        {
            records.push(path);
        }
    }
    Ok(records)
}

fn remove_file(path: &Path) -> Result<(), PruneError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PruneError::RemoveSlot { path: path.to_path_buf(), error }),
    }
}
