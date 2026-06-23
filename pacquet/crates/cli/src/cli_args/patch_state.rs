use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::lexical_normalize;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};

const STATE_DIR: &str = ".pnpm_patches";
const STATE_FILE: &str = "state.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EditDirState {
    #[serde(rename = "patchedPkg")]
    pub(crate) patched_pkg: String,
    #[serde(rename = "applyToAll")]
    pub(crate) apply_to_all: bool,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub(crate) enum StateFileError {
    #[display("Failed to read patch state file {path:?}: {source}")]
    #[diagnostic(code(pacquet::patch_state_read))]
    Read {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse patch state file {path:?}: {source}")]
    #[diagnostic(code(pacquet::patch_state_parse))]
    Parse {
        path: PathBuf,
        #[error(source)]
        source: serde_json::Error,
    },

    #[display("Failed to serialize patch state file {path:?}: {source}")]
    #[diagnostic(code(pacquet::patch_state_serialize))]
    Serialize {
        path: PathBuf,
        #[error(source)]
        source: serde_json::Error,
    },

    #[display("Failed to write patch state file {path:?}: {source}")]
    #[diagnostic(code(pacquet::patch_state_write))]
    Write {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to resolve patch edit directory {path:?}: {source}")]
    #[diagnostic(code(pacquet::patch_state_resolve_edit_dir))]
    ResolveEditDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

pub(crate) fn read_edit_dir_state(
    modules_dir: &Path,
    edit_dir: &Path,
) -> Result<Option<EditDirState>, StateFileError> {
    let path = state_file_path(modules_dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StateFileError::Read { path, source }),
    };
    let state: BTreeMap<String, EditDirState> = serde_json::from_str(&text)
        .map_err(|source| StateFileError::Parse { path: path.clone(), source })?;
    let key = edit_dir_key(edit_dir)?;
    Ok(state.get(&key).cloned())
}

pub(crate) fn write_edit_dir_state(
    modules_dir: &Path,
    edit_dir: &Path,
    edit_dir_state: &EditDirState,
) -> Result<(), StateFileError> {
    let path = state_file_path(modules_dir);
    let mut state = read_state_file_for_write(&path)?;
    state.insert(edit_dir_key(edit_dir)?, edit_dir_state.clone());

    let text = serde_json::to_string_pretty(&state)
        .map_err(|source| StateFileError::Serialize { path: path.clone(), source })?;
    let state_dir = path.parent().expect("state file has parent");
    fs::create_dir_all(state_dir)
        .map_err(|source| StateFileError::Write { path: state_dir.to_path_buf(), source })?;
    fs::write(&path, text).map_err(|source| StateFileError::Write { path, source })
}

fn read_state_file_for_write(
    path: &Path,
) -> Result<BTreeMap<String, EditDirState>, StateFileError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(source) => return Err(StateFileError::Read { path: path.to_path_buf(), source }),
    };
    serde_json::from_str(&text)
        .map_err(|source| StateFileError::Parse { path: path.to_path_buf(), source })
}

fn state_file_path(modules_dir: &Path) -> PathBuf {
    modules_dir.join(STATE_DIR).join(STATE_FILE)
}

fn edit_dir_key(edit_dir: &Path) -> Result<String, StateFileError> {
    let absolute = if edit_dir.is_absolute() {
        edit_dir.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|source| StateFileError::ResolveEditDir {
                path: edit_dir.to_path_buf(),
                source,
            })?
            .join(edit_dir)
    };
    let normalized = lexical_normalize(&absolute);
    Ok(match dunce::canonicalize(&normalized) {
        Ok(path) => path,
        Err(source) if source.kind() == io::ErrorKind::NotFound => normalized,
        Err(source) => {
            return Err(StateFileError::ResolveEditDir { path: edit_dir.to_path_buf(), source });
        }
    }
    .display()
    .to_string())
}

#[cfg(test)]
mod tests;
