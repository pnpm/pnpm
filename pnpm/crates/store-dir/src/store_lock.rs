use crate::StoreDir;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    fs::{File, OpenOptions},
    io,
    path::PathBuf,
};

#[derive(Debug, Display, Error, Diagnostic)]
pub enum StoreLockError {
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
    _file: File,
}

impl StoreDir {
    /// Keep the store stable while an install, fetch, or engine setup may
    /// create or consume files in it. Multiple consumers may run together.
    pub fn lock_for_use(&self) -> Result<StoreOperationLock, StoreLockError> {
        lock_file(operation_lock_path(self), false)
    }

    /// Keep an immutable store stable without creating a lock file inside it.
    pub fn lock_for_frozen_use(&self) -> Result<StoreOperationLock, StoreLockError> {
        lock_file(operation_lock_path(self), false)
    }

    pub(crate) fn lock_for_prune(&self) -> Result<StoreOperationLock, StoreLockError> {
        lock_file(operation_lock_path(self), true)
    }
}

fn lock_file(path: PathBuf, exclusive: bool) -> Result<StoreOperationLock, StoreLockError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| StoreLockError::Open { path: path.clone(), error })?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| StoreLockError::Open { path: path.clone(), error })?;
    let result = if exclusive { File::lock(&file) } else { File::lock_shared(&file) };
    result.map_err(|error| StoreLockError::Acquire { path, error })?;
    Ok(StoreOperationLock { _file: file })
}

fn operation_lock_path(store_dir: &StoreDir) -> PathBuf {
    let key = pnpm_crypto_hash::create_hex_hash(&store_dir.root().to_string_lossy());
    std::env::temp_dir()
        .join("pnpm-store-operation-locks")
        .join(format!("{key}.lock"))
}

#[cfg(test)]
mod tests;
