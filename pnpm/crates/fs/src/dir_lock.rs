//! A cross-process advisory lock over a shared directory.
//!
//! Creating a directory is atomic on every platform pacquet supports, so
//! the winner of a `create_dir` race owns the lock and everyone else
//! waits. Nothing enforces it: a lock only protects a resource whose
//! every writer takes the same lock.
//!
//! A directory outlives the process that created it, so on its own it
//! cannot tell a waiter whether its holder is still at work or died
//! holding it — interrupted by `Ctrl+C`, say. The holder therefore also
//! keeps an OS file lock on a file inside the directory. The OS releases
//! that lock when the holder's process ends, however it ends, and a
//! waiter that finds the file unlocked takes the directory over at once
//! instead of sitting out its whole wait.
//!
//! The lock is advisory in a second sense — [`DirLock::acquire`] gives
//! up after a bounded wait and reports that it could not take the lock,
//! so a caller can proceed unserialized rather than fail. A lock is
//! there to avoid a race, and a race lost is better than a command that
//! refuses to run.

use std::{
    fs::{self, File, TryLockError},
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How often the lock directory is retried while another process holds
/// it.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// How long a Windows lock-directory release may keep a competing
/// `create_dir` in the delete-pending state.
const RELEASE_RETRY_BUDGET: Duration = Duration::from_secs(1);

/// How long a claimer keeps trying to take the OS lock on its own held
/// file while a waiter's probe has it. A probe holds it for the length
/// of one small read, so this is far more than it needs.
const HOLD_RETRY_BUDGET: Duration = Duration::from_secs(1);

/// Names the file inside the lock directory that records who took it.
const OWNER_FILE: &str = "owner";

/// Names the file inside the lock directory the holder keeps an OS file
/// lock on for as long as it holds the lock.
const HELD_FILE: &str = "held";

/// A held lock. Released on drop.
#[derive(Debug)]
pub struct DirLock {
    path: PathBuf,
    /// Identifies this acquisition, so releasing can tell "the lock I
    /// took" from "a lock someone else took at the same path after mine
    /// was declared abandoned". Without it a slow holder would release
    /// its successor's lock on drop.
    token: String,
    /// The OS lock that tells waiters this process is still running.
    /// `None` on a filesystem that cannot hold one; the lock is then only
    /// as good as its age bound.
    held: Option<File>,
}

impl DirLock {
    /// Take the lock at `path`, waiting up to `wait` for whoever holds
    /// it. Returns `None` when the wait ran out.
    ///
    /// A lock directory whose holder's process has ended is taken over
    /// at once. When that cannot be told — the holder is an older pnpm,
    /// or the filesystem cannot hold an OS file lock — a directory older
    /// than `abandoned_after` is treated as left behind by a process
    /// that died holding it, and is taken over. That bound has to exceed
    /// how long the guarded work can legitimately take, or a slow holder
    /// gets its lock stolen — which is why it is the caller's to choose
    /// and not tied to `wait`.
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
        let deadline = Instant::now() + wait;
        let mut release_retry_started = None;
        loop {
            match try_create_lock_dir(&path, deadline, &mut release_retry_started)? {
                CreateAttempt::Claimed => return claim(path).map(Some),
                CreateAttempt::Retry => continue,
                CreateAttempt::Held => {}
            }
            // Best-effort: whoever removes it first wins the next
            // `create_dir`, and a failure just means another waiter
            // got there first.
            if is_abandoned(&path, abandoned_after) && remove_lock_dir(&path) {
                continue;
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            sleep(POLL_INTERVAL);
        }
    }

    /// Reports whether this acquisition still owns the lock after a possible takeover.
    pub fn is_owner(&self) -> io::Result<bool> {
        match fs::read_to_string(self.path.join(OWNER_FILE)) {
            Ok(owner) => Ok(owner == self.token),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// What one attempt at creating the lock directory settled.
enum CreateAttempt {
    /// The directory is ours.
    Claimed,
    /// A release was still in flight; the attempt was slept out and should be
    /// repeated.
    Retry,
    /// Someone else holds the lock.
    Held,
}

/// Try to create the lock directory once.
///
/// On Windows a directory being released is briefly un-creatable, which is
/// retried within its own budget rather than reported as contention.
fn try_create_lock_dir(
    path: &Path,
    deadline: Instant,
    release_retry_started: &mut Option<Instant>,
) -> io::Result<CreateAttempt> {
    let error = match fs::create_dir(path) {
        Ok(()) => return Ok(CreateAttempt::Claimed),
        Err(error) => error,
    };
    if is_transient_release_error(&error) {
        let started = release_retry_started.get_or_insert_with(Instant::now);
        if Instant::now() >= deadline || started.elapsed() >= RELEASE_RETRY_BUDGET {
            return Err(error);
        }
        sleep(POLL_INTERVAL);
        return Ok(CreateAttempt::Retry);
    }
    if error.kind() != io::ErrorKind::AlreadyExists {
        return Err(error);
    }
    *release_retry_started = None;
    Ok(CreateAttempt::Held)
}

fn is_transient_release_error(
    #[cfg_attr(not(windows), allow(unused, reason = "only inspected on Windows"))]
    error: &io::Error,
) -> bool {
    #[cfg(windows)]
    {
        matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy)
    }
    #[cfg(not(windows))]
    {
        false
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
        // Windows keeps a file with an open handle in place, and the
        // directory with it, so the OS lock goes before the directory.
        drop(self.held.take());
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Record this process as the owner of a lock directory it just created.
/// A lock that cannot be recorded is given back, since [`Drop`] would
/// have no way to tell at release time whether it is still ours.
///
/// The OS lock is taken before the record is written, so a waiter that
/// finds the record beside an unlocked held file knows the holder took
/// the lock and then went away, not that it has yet to take it.
fn claim(path: PathBuf) -> io::Result<DirLock> {
    let held = hold(&path);
    let token = mint_token();
    if let Err(error) = fs::write(path.join(OWNER_FILE), &token) {
        drop(held);
        let _ = fs::remove_dir_all(&path);
        return Err(error);
    }
    Ok(DirLock { path, token, held })
}

/// Take the OS lock that tells waiters this process is still running.
///
/// `None` when the filesystem cannot hold one. The held file is then
/// removed again, so a waiter does not read a lock that was never taken
/// as a holder that died.
fn hold(path: &Path) -> Option<File> {
    let held_path = path.join(HELD_FILE);
    let file = File::create(&held_path).ok()?;
    let started = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            // A waiter probing this very file holds it for the length of
            // one read.
            Err(TryLockError::WouldBlock) if started.elapsed() < HOLD_RETRY_BUDGET => {
                sleep(POLL_INTERVAL);
            }
            Err(_) => break,
        }
    }
    drop(file);
    let _ = fs::remove_file(held_path);
    None
}

/// A value no concurrent acquisition shares. The clock supplies
/// cross-process (and cross-host, for a store on a network filesystem)
/// distinctness that a pid alone cannot, and the counter separates two
/// acquisitions within the same clock tick.
fn mint_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Whether the lock directory at `path` belongs to nobody any more: its
/// holder's process has ended, or it is older than `abandoned_after`.
fn is_abandoned(path: &Path, abandoned_after: Duration) -> bool {
    holder_is_gone(path) || is_older_than(path, abandoned_after)
}

/// Whether the process that took the lock at `path` has ended, told by
/// the OS lock on the held file: a live holder keeps it, and the OS
/// releases it when the holder's process ends.
///
/// `false` whenever that cannot be told: the held file is missing (an
/// older pnpm keeps none), the filesystem cannot hold an OS lock, or the
/// owner record is not there yet, which is a claim still in progress
/// rather than a holder that died.
fn holder_is_gone(path: &Path) -> bool {
    let Ok(held) = File::open(path.join(HELD_FILE)) else {
        return false;
    };
    if held.try_lock().is_err() {
        return false;
    }
    path.join(OWNER_FILE).exists()
}

fn is_older_than(path: &Path, age: Duration) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|elapsed| elapsed > age)
}

/// Remove an abandoned lock directory. A directory that is already gone
/// counts as removed: another waiter got there first, and the next
/// `create_dir` decides between them.
fn remove_lock_dir(path: &Path) -> bool {
    match fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod tests;
