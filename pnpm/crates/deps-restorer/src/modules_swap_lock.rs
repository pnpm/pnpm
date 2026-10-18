use std::{
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

/// How long an install waits for another install's replacements under
/// the same modules directory before it replaces unserialized, as the
/// global virtual store slot lock does. Replacing a package takes
/// seconds at most, so a wait this long only runs out on a holder that
/// is stuck.
const WAIT: Duration = Duration::from_mins(10);
/// Past how long an install's linking phase can legitimately take, so a
/// holder that cannot prove it is alive never has its lock stolen
/// mid-install.
const ABANDONED_AFTER: Duration = Duration::from_hours(2);

/// The lock an install holds while it replaces package directories under
/// one modules directory, so that two installs replacing the same
/// directories at once take turns instead of undoing each other's work.
///
/// Taken on the first replacement and held until the install drops it:
/// one acquisition per install, however many packages it replaces, and
/// none for an install that replaces nothing. The workers of one install
/// share the held lock. An install that cannot take it, because the
/// per-user lock root is unusable or the wait ran out, replaces
/// unserialized: the replacement tolerates a concurrent importer on its
/// own, so a lock-directory problem must not fail the install.
#[derive(Debug)]
pub struct ModulesSwapLock {
    modules_dir: PathBuf,
    held: OnceLock<Option<pnpm_fs::DirLock>>,
}

impl ModulesSwapLock {
    #[must_use]
    pub fn new(modules_dir: &Path) -> Self {
        ModulesSwapLock { modules_dir: modules_dir.to_path_buf(), held: OnceLock::new() }
    }

    /// Take the lock on first use, and report whether this install holds
    /// it. `false` means its replacements run unserialized.
    pub fn hold(&self) -> bool {
        self.held.get_or_init(|| acquire(&self.modules_dir)).is_some()
    }
}

fn acquire(modules_dir: &Path) -> Option<pnpm_fs::DirLock> {
    let acquired = lock_path(modules_dir)
        .and_then(|lock_path| pnpm_fs::DirLock::acquire(lock_path, WAIT, ABANDONED_AFTER));
    match acquired {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            tracing::debug!(
                target: "pacquet::import_indexed_dir",
                ?modules_dir,
                "timed out waiting for the modules swap lock; replacing packages unserialized",
            );
            None
        }
        Err(error) => {
            tracing::debug!(
                target: "pacquet::import_indexed_dir",
                ?modules_dir,
                %error,
                "could not take the modules swap lock; replacing packages unserialized",
            );
            None
        }
    }
}

/// The lock file standing for `modules_dir` in the per-user lock
/// directory. The directory is resolved first, so every alias of one
/// location shares the lock; a directory that does not exist yet
/// resolves through its deepest existing ancestor.
fn lock_path(modules_dir: &Path) -> io::Result<PathBuf> {
    let resolved = pnpm_fs::realpath_missing(&pnpm_fs::lexical_normalize(modules_dir))?;
    pnpm_fs::secure_user_lock_file_path("pnpm-modules-swaps", &resolved, "lock")
}

#[cfg(test)]
mod tests;
