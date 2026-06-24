use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::{is_subdir, lexical_normalize};
use pacquet_lockfile::PackageKey;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const STATE_DIR: &str = ".pnpm_patches";
const STATE_FILE: &str = "state.json";
pub(crate) const MAX_STATE_FILE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EditDirState {
    #[serde(rename = "patchedPkg")]
    pub(crate) patched_pkg: String,
    #[serde(rename = "applyToAll")]
    pub(crate) apply_to_all: bool,
    #[serde(rename = "packageKey", default, skip_serializing_if = "Option::is_none")]
    pub(crate) package_key: Option<PackageKey>,
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

    #[display("Unsafe patch state path {path:?}: {reason}")]
    #[diagnostic(code(pacquet::patch_state_unsafe_path))]
    UnsafePath { path: PathBuf, reason: &'static str },

    #[display("Patch state file {path:?} is larger than {limit} bytes")]
    #[diagnostic(code(pacquet::patch_state_file_too_large))]
    StateFileTooLarge { path: PathBuf, limit: usize },

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
    let path = checked_state_file_path_for_read(modules_dir)?;
    let Some(text) = read_state_file_text(&path)? else { return Ok(None) };
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
    let path = checked_state_file_path_for_write(modules_dir)?;
    let mut state = read_state_file_for_write(&path)?;
    state.insert(edit_dir_key(edit_dir)?, edit_dir_state.clone());

    let text = serde_json::to_string_pretty(&state)
        .map_err(|source| StateFileError::Serialize { path: path.clone(), source })?;
    write_state_file_atomically(&path, text.as_bytes())
        .map_err(|source| StateFileError::Write { path, source })
}

fn read_state_file_for_write(
    path: &Path,
) -> Result<BTreeMap<String, EditDirState>, StateFileError> {
    let Some(text) = read_state_file_text(path)? else { return Ok(BTreeMap::new()) };
    serde_json::from_str(&text)
        .map_err(|source| StateFileError::Parse { path: path.to_path_buf(), source })
}

fn read_state_file_text(path: &Path) -> Result<Option<String>, StateFileError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StateFileError::Read { path: path.to_path_buf(), source }),
    };
    if !metadata.is_file() {
        return Err(StateFileError::Read {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidData, "state path is not a file"),
        });
    }
    if metadata.len() > MAX_STATE_FILE_BYTES as u64 {
        return Err(StateFileError::StateFileTooLarge {
            path: path.to_path_buf(),
            limit: MAX_STATE_FILE_BYTES,
        });
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|source| StateFileError::Read { path: path.to_path_buf(), source })
}

fn state_file_path(modules_dir: &Path) -> PathBuf {
    modules_dir.join(STATE_DIR).join(STATE_FILE)
}

fn checked_state_file_path_for_read(modules_dir: &Path) -> Result<PathBuf, StateFileError> {
    let path = state_file_path(modules_dir);
    validate_existing_state_path(modules_dir, &path)?;
    Ok(path)
}

fn checked_state_file_path_for_write(modules_dir: &Path) -> Result<PathBuf, StateFileError> {
    let path = state_file_path(modules_dir);
    let state_dir = path.parent().expect("state file has parent");
    reject_state_symlink_if_exists(state_dir)?;
    fs::create_dir_all(state_dir)
        .map_err(|source| StateFileError::Write { path: state_dir.to_path_buf(), source })?;
    validate_existing_state_path(modules_dir, &path)?;
    Ok(path)
}

fn validate_existing_state_path(modules_dir: &Path, path: &Path) -> Result<(), StateFileError> {
    let state_dir = path.parent().expect("state file has parent");
    reject_state_symlink_if_exists(state_dir)?;
    reject_state_symlink_if_exists(path)?;
    if let (Ok(real_modules_dir), Ok(real_state_dir)) =
        (dunce::canonicalize(modules_dir), dunce::canonicalize(state_dir))
        && !is_subdir(&real_modules_dir, &real_state_dir)
    {
        return Err(StateFileError::UnsafePath {
            path: state_dir.to_path_buf(),
            reason: "must stay under the modules directory",
        });
    }
    Ok(())
}

fn reject_state_symlink_if_exists(path: &Path) -> Result<(), StateFileError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(StateFileError::UnsafePath {
            path: path.to_path_buf(),
            reason: "must not be a symbolic link",
        }),
        Ok(_) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StateFileError::Read { path: path.to_path_buf(), source }),
    }
}

fn write_state_file_atomically(target: &Path, content: &[u8]) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
    tmp.persist(target).map_err(|error| error.error)?;
    Ok(())
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
