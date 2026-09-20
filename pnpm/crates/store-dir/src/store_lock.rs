use crate::StoreDir;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
pub enum StoreLockError {
    #[display("Failed to resolve the store operation lock identity for {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_RESOLVE_OPERATION_LOCK))]
    Resolve {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to open the store operation lock at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_OPEN_OPERATION_LOCK))]
    Open {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to acquire the store operation lock at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_ACQUIRE_OPERATION_LOCK))]
    Acquire {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// A cross-process lock that keeps destructive store maintenance from
/// overlapping an operation that may still consume store files.
#[derive(Debug)]
pub struct StoreOperationLock {
    _files: Vec<File>,
}

impl StoreDir {
    /// Keep the store stable while an install, fetch, or engine setup may
    /// create or consume files in it. Multiple consumers may run together.
    pub fn lock_for_use(&self) -> Result<StoreOperationLock, StoreLockError> {
        lock_files(self, false)
    }

    /// Keep an immutable store stable without creating a lock file inside it.
    pub fn lock_for_frozen_use(&self) -> Result<StoreOperationLock, StoreLockError> {
        lock_files(self, false)
    }

    /// Keep consumers out of the store while destructive maintenance runs.
    pub fn lock_for_prune(&self) -> Result<StoreOperationLock, StoreLockError> {
        lock_files(self, true)
    }
}

fn lock_files(store_dir: &StoreDir, exclusive: bool) -> Result<StoreOperationLock, StoreLockError> {
    // The shared global barrier keeps path aliases safe even if a symlink or
    // junction changes target after its per-store identity is resolved.
    // Consumers of every store still overlap; only destructive maintenance
    // takes this barrier exclusively.
    let paths = [global_operation_lock_path()?, operation_lock_path(store_dir)?];
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let file = pnpm_fs::open_secure_lock_file(&path)
            .map_err(|error| StoreLockError::Open { path: path.clone(), error })?;
        let result = if exclusive { File::lock(&file) } else { File::lock_shared(&file) };
        result.map_err(|error| StoreLockError::Acquire { path, error })?;
        files.push(file);
    }
    Ok(StoreOperationLock { _files: files })
}

fn global_operation_lock_path() -> Result<PathBuf, StoreLockError> {
    operation_lock_directory().map(|directory| directory.join("all-stores.lock"))
}

fn operation_lock_directory() -> Result<PathBuf, StoreLockError> {
    pnpm_fs::secure_user_lock_dir("pnpm-store-operation-locks")
        .map_err(|error| StoreLockError::Open {
            path: PathBuf::from("pnpm-store-operation-locks"),
            error,
        })
}

fn operation_lock_path(store_dir: &StoreDir) -> Result<PathBuf, StoreLockError> {
    let normalized_root = pnpm_fs::lexical_normalize(store_dir.root());
    let normalized_root = if normalized_root.is_absolute() {
        normalized_root
    } else {
        let current_dir = std::env::current_dir()
            .map_err(|error| StoreLockError::Resolve {
                path: store_dir.root().to_path_buf(),
                error,
            })?;
        pnpm_fs::lexical_normalize(&current_dir.join(normalized_root))
    };
    let root = pnpm_fs::realpath_missing(&normalized_root)
        .map_err(|error| StoreLockError::Resolve { path: store_dir.root().to_path_buf(), error })?;
    let key = pnpm_crypto_hash::create_hex_hash_bytes(&native_path_bytes(&root));
    let directory = operation_lock_directory()?;
    Ok(directory.join(format!("{key}.lock")))
}

#[cfg(unix)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .as_encoded_bytes()
        .to_vec()
}

#[cfg(test)]
mod tests;
