use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::DirLock;
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

const WAIT: Duration = Duration::from_mins(5);
const ABANDONED_AFTER: Duration = Duration::from_mins(30);

#[derive(Debug, Display, Error, Diagnostic)]
enum GlobalBinLockError {
    #[display("Failed to lock the global bin directory at {}", path.display())]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_LOCK_FAILED))]
    Failed {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Timed out waiting for the global bin directory lock at {}", path.display())]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_LOCK_TIMEOUT))]
    TimedOut { path: PathBuf },
}

/// Serialize the bin-slot checks and writes performed by global package,
/// shim, and self-update commands. The guard must cover both the ownership
/// check and any rollback, so a failed transaction cannot restore over a
/// later pnpm process's successful replacement.
pub(crate) fn acquire_global_bin_lock(global_bin_dir: &Path) -> miette::Result<DirLock> {
    let path = global_bin_dir.join(".pnpm-global-bin.lock");
    match DirLock::acquire(path.clone(), WAIT, ABANDONED_AFTER) {
        Ok(Some(lock)) => Ok(lock),
        Ok(None) => Err(GlobalBinLockError::TimedOut { path }.into()),
        Err(source) => Err(GlobalBinLockError::Failed { path, source }.into()),
    }
}

pub(crate) fn try_acquire_global_bin_lock(
    global_bin_dir: &Path,
) -> miette::Result<Option<DirLock>> {
    let path = global_bin_dir.join(".pnpm-global-bin.lock");
    DirLock::acquire(path.clone(), Duration::ZERO, ABANDONED_AFTER)
        .map_err(|source| GlobalBinLockError::Failed { path, source }.into())
}

#[cfg(test)]
mod tests;
