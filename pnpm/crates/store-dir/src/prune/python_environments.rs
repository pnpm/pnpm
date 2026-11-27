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
/// but the one the project's `.venv` links to, and the whole directory when
/// the project is gone. A project directory without a link back to its
/// project is kept whole, since nothing tells which generation is in use.
/// Returns how many generations went.
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
    match fs::metadata(&project) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            remove_slot_dir(dir)?;
            return Ok(generations.len());
        }
        Err(error) => return Err(PruneError::ReadMarkDir { path: project, error }),
    }
    let used = used_generation(&project, dir)?;
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
