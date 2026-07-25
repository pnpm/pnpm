//! A cross-process advisory lock over a shared directory.
//!
//! Creating a directory is atomic on every platform pacquet supports, so
//! the winner of a `create_dir` race owns the lock and everyone else
//! waits. Nothing enforces it: a lock only protects a resource whose
//! every writer takes the same lock.
//!
//! The lock is advisory in a second sense — [`DirLock::acquire`] gives
//! up after a bounded wait and reports that it could not take the lock,
//! so a caller can proceed unserialized rather than fail. A lock is
//! there to avoid a race, and a race lost is better than a command that
//! refuses to run.

use std::{
    fs, io,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, SystemTime},
};

/// How often the lock directory is retried while another process holds
/// it.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A held lock. Released on drop.
#[derive(Debug)]
pub struct DirLock {
    path: PathBuf,
}

impl DirLock {
    /// Take the lock at `path`, waiting up to `wait` for whoever holds
    /// it. Returns `None` when the wait ran out.
    ///
    /// A lock directory older than `abandoned_after` is treated as left
    /// behind by a process that died holding it, and is taken over. That
    /// bound has to exceed how long the guarded work can legitimately
    /// take, or a slow holder gets its lock stolen — which is why it is
    /// the caller's to choose and not tied to `wait`.
    ///
    /// Errors are limited to creating the lock's parent directory;
    /// everything else is a lost race, which is what the wait is for.
    pub fn acquire(
        path: PathBuf,
        wait: Duration,
        abandoned_after: Duration,
    ) -> io::Result<Option<DirLock>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let deadline = SystemTime::now() + wait;
        loop {
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Some(DirLock { path })),
                Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
                Err(_) => {}
            }
            if is_abandoned(&path, abandoned_after) {
                // Best-effort: whoever removes it first wins the next
                // `create_dir`, and a failure just means another waiter
                // got there first.
                let _ = fs::remove_dir(&path);
                continue;
            }
            if SystemTime::now() >= deadline {
                return Ok(None);
            }
            sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

fn is_abandoned(path: &Path, abandoned_after: Duration) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    SystemTime::now().duration_since(modified).is_ok_and(|age| age > abandoned_after)
}

#[cfg(test)]
mod tests;
