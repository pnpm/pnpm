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
    sync::atomic::{AtomicU64, Ordering},
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How often the lock directory is retried while another process holds
/// it.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Names the file inside the lock directory that records who took it.
const OWNER_FILE: &str = "owner";

/// A held lock. Released on drop.
#[derive(Debug)]
pub struct DirLock {
    path: PathBuf,
    /// Identifies this acquisition, so releasing can tell "the lock I
    /// took" from "a lock someone else took at the same path after mine
    /// was declared abandoned". Without it a slow holder would release
    /// its successor's lock on drop.
    token: String,
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
    /// `Ok(None)` is a lock someone else holds; `Err` is one that could
    /// not be established at all. Callers that degrade rather than fail
    /// need to tell those apart.
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
                Ok(()) => return claim(path).map(Some),
                Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
                // Held by someone else: fall through to the wait below.
                Err(_) => {}
            }
            if is_abandoned(&path, abandoned_after) {
                // Best-effort: whoever removes it first wins the next
                // `create_dir`, and a failure just means another waiter
                // got there first.
                let _ = fs::remove_dir_all(&path);
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
        // Only release a lock that is still ours. A holder that outran
        // `abandoned_after` has already had its directory removed and
        // replaced by the next process in line; removing that one would
        // hand the resource to two processes at once.
        //
        // Reading the record and removing the directory are two steps, so
        // a takeover landing between them is still removable — the check
        // narrows the window from the whole guarded operation to a couple
        // of syscalls, it does not close it. Closing it needs an atomic
        // compare-and-remove no portable filesystem API offers. The
        // residual is acceptable because reaching it requires a takeover,
        // which requires this holder to have already run past
        // `abandoned_after` — a bound the caller sizes well above the
        // work being guarded.
        match fs::read_to_string(self.path.join(OWNER_FILE)) {
            Ok(owner) if owner != self.token => return,
            Err(error) if error.kind() != io::ErrorKind::NotFound => return,
            _ => {}
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Record this process as the owner of a lock directory it just created.
/// A lock that cannot be recorded is given back, since [`Drop`] would
/// have no way to tell at release time whether it is still ours.
fn claim(path: PathBuf) -> io::Result<DirLock> {
    let token = mint_token();
    if let Err(error) = fs::write(path.join(OWNER_FILE), &token) {
        let _ = fs::remove_dir_all(&path);
        return Err(error);
    }
    Ok(DirLock { path, token })
}

/// A value no concurrent acquisition shares. The clock supplies
/// cross-process (and cross-host, for a store on a network filesystem)
/// distinctness that a pid alone cannot, and the counter separates two
/// acquisitions within the same clock tick.
fn mint_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{}-{nanos}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn is_abandoned(path: &Path, abandoned_after: Duration) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    SystemTime::now().duration_since(modified).is_ok_and(|age| age > abandoned_after)
}

#[cfg(test)]
mod tests;
