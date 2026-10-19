use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

/// How long an install waits for another install's linking phase under
/// the same modules directory before it replaces unserialized, as the
/// global virtual store slot lock does. A linking phase that runs
/// longer than this, as a large cold install on a slow disk can, leaves
/// the waiter to the tolerant swap, which accepts or replaces an
/// equivalent concurrent import on its own.
const WAIT: Duration = Duration::from_mins(10);
/// Past how long an install's linking phase can legitimately take, so a
/// holder that cannot prove it is alive never has its lock stolen
/// mid-install.
const ABANDONED_AFTER: Duration = Duration::from_hours(2);

/// The lock an install holds while it replaces package directories under
/// one modules directory, so that two installs replacing the same
/// directories at once take turns instead of undoing each other's work.
///
/// Taken on the first forced import of a private target, including into
/// an absent one, and held until [`Self::release`]: one acquisition per
/// linking phase, however many packages it imports, and none for an
/// isolated install that forces nothing. The workers of one install
/// share the held lock. The linking phase releases it before any
/// lifecycle script runs, so an install a script starts in the same
/// project does not wait on its parent. An install that cannot take it,
/// because the per-user lock root is unusable or the wait ran out,
/// replaces unserialized for the rest of its life: the replacement
/// tolerates a concurrent importer on its own, so a lock-directory
/// problem must not fail the install, and must not cost every later
/// import another wait.
#[derive(Debug)]
pub struct ModulesSwapLock {
    modules_dir: PathBuf,
    state: Mutex<State>,
}

#[derive(Debug)]
enum State {
    /// Not held; the next [`ModulesSwapLock::hold`] takes it.
    Free,
    /// Held for as long as the value lives; nothing reads it.
    Held { _lock: pnpm_fs::DirLock },
    /// Could not be taken; no later [`ModulesSwapLock::hold`] tries again.
    Unserialized,
}

impl ModulesSwapLock {
    #[must_use]
    pub fn new(modules_dir: &Path) -> Self {
        ModulesSwapLock { modules_dir: modules_dir.to_path_buf(), state: Mutex::new(State::Free) }
    }

    /// Take the lock unless this install already holds it or already
    /// failed to take it, and report whether it holds it. `false` means
    /// its replacements run unserialized.
    pub fn hold(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*state, State::Free) {
            *state = match acquire(&self.modules_dir) {
                Some(lock) => State::Held { _lock: lock },
                None => State::Unserialized,
            };
        }
        matches!(*state, State::Held { .. })
    }

    /// Give the lock back, so that another install's replacements can
    /// run while this one goes on to work that may start installs of
    /// its own. A later [`Self::hold`] takes it again.
    pub fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*state, State::Held { .. }) {
            *state = State::Free;
        }
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
