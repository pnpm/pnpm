//! Remove the Python environment generations no project runs. See
//! [`crate::python_environments`] for the layout.

use super::{
    PruneError,
    list_subdirs,
    remove_slot_dir,
};
use crate::python_environments::{
    GENERATION_PREFIX,
    PROJECT_LINK,
};
use pnpm_fs::{
    is_non_link_read_error,
    read_symlink_dir,
};
use std::{
    ffi::OsString,
    fs,
    io::ErrorKind,
    path::Path,
};

/// In each project directory under `python_envs`, remove every generation
/// but the one in use. Returns how many generations went.
///
/// - The generation in use is the one the project's `.venv` links to.
/// - When the project is gone, it is the newest generation. A moved project's
///   `.venv` still links to the generation it had, and the link back names
///   the old location.
/// - A project directory without a link back to its project is kept whole,
///   since nothing tells which generation is in use.
pub(super) fn remove_unused_generations(python_envs: &Path) -> Result<usize, PruneError> {
    let mut removed = 0;
    for name in list_subdirs(python_envs)? {
        removed += remove_unused_project_generations(&python_envs.join(name))?;
    }
    Ok(removed)
}

fn remove_unused_project_generations(dir: &Path) -> Result<usize, PruneError> {
    let link = dir.join(PROJECT_LINK);
    let project = match read_symlink_dir(&link) {
        Ok(project) => project,
        Err(error) if error.kind() == ErrorKind::NotFound || is_non_link_read_error(&error) => {
            return Ok(0);
        }
        Err(error) => return Err(PruneError::ReadMarkLink { path: link, error }),
    };
    let generations = generations(dir)?;
    let used = match fs::metadata(&project) {
        Ok(_) => used_generation(&project, dir)?,
        Err(error) if error.kind() == ErrorKind::NotFound => newest_generation(dir, &generations)?,
        Err(error) => return Err(PruneError::ReadMarkDir { path: project, error }),
    };
    let mut removed = 0;
    for generation in generations {
        if used.as_ref() != Some(&generation) {
            remove_slot_dir(&dir.join(generation))?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn generations(dir: &Path) -> Result<Vec<OsString>, PruneError> {
    Ok(list_subdirs(dir)?
        .into_iter()
        .filter(|name| name.to_string_lossy().starts_with(GENERATION_PREFIX))
        .collect())
}

fn newest_generation(dir: &Path, generations: &[OsString]) -> Result<Option<OsString>, PruneError> {
    let mut newest = None;
    for generation in generations {
        let path = dir.join(generation);
        let modified = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .map_err(|error| PruneError::ReadSweepDir { path, error })?;
        if newest
            .as_ref()
            .is_none_or(|(time, _)| modified > *time)
        {
            newest = Some((modified, generation.clone()));
        }
    }
    Ok(newest.map(|(_, generation)| generation))
}

/// The name of the generation in `dir` that `<project>/.venv` links to.
fn used_generation(project: &Path, dir: &Path) -> Result<Option<OsString>, PruneError> {
    let venv = project.join(".venv");
    let target = match dunce::canonicalize(&venv) {
        Ok(target) => target,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PruneError::ReadMarkLink { path: venv, error }),
    };
    let dir = dunce::canonicalize(dir)
        .map_err(|error| PruneError::ReadSweepDir { path: dir.to_path_buf(), error })?;
    Ok((target.parent() == Some(dir.as_path()))
        .then(|| target.file_name().map(OsString::from))
        .flatten())
}
