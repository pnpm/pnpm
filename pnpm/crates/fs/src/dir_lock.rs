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
//! keeps an OS file lock on the held file beside the directory,
//! `<path>.held`, for as long as it holds the lock. The OS releases that
//! lock when the holder's process ends, however it ends. Only the process
//! holding the file lock creates, removes, or takes over the directory,
//! so a waiter that gets the file lock and still finds the directory
//! there knows its holder is gone, and takes it over at once instead of
//! sitting out its whole wait.
//!
//! The directory stays the lock a pnpm without the file lock — an older
//! release, or one on a filesystem that cannot hold OS locks — agrees on.
//! Such a holder's liveness cannot be told, so its directory's age decides
//! when it counts as gone.
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

/// How long a claimer may take between creating the directory and
/// recording itself in it. A directory still unrecorded past this age
/// was left by a claimer that died in between.
const CLAIM_GRACE: Duration = Duration::from_secs(10);

/// Names the file inside the lock directory that records who took it.
const OWNER_FILE: &str = "owner";

/// Names the file inside the lock directory that says its holder keeps
/// the OS lock on the held file, so a waiter holding that lock knows the
/// holder is gone without waiting for the directory to age.
const HELD_MARKER: &str = "held";

/// The suffix that names the held file beside the lock directory.
const HELD_FILE_SUFFIX: &str = ".held";

/// A held lock. Released on drop.
#[derive(Debug)]
pub struct DirLock {
    path: PathBuf,
    /// Identifies this acquisition, so releasing can tell "the lock I
    /// took" from "a lock someone else took at the same path after mine
    /// was declared abandoned". Without it a slow holder would release
    /// its successor's lock on drop.
    token: String,
    /// The OS lock on the held file, kept for as long as the lock is.
    /// `None` on a filesystem that cannot hold one; the directory is
    /// then the whole lock, and waiters judge it by its age.
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
        let mut acquisition = Acquisition {
            held: HeldFile::open(&path),
            path,
            abandoned_after,
            deadline,
            release_retry_started: None,
        };
        loop {
            match acquisition.poll()? {
                CreateAttempt::Claimed => return acquisition.claim().map(Some),
                CreateAttempt::Retry => continue,
                CreateAttempt::Held => {}
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

/// One process's attempt to take the lock at `path`, polled until it
/// succeeds or the deadline passes.
struct Acquisition {
    path: PathBuf,
    held: HeldFile,
    abandoned_after: Duration,
    deadline: Instant,
    release_retry_started: Option<Instant>,
}

impl Acquisition {
    /// One poll: create the directory, remove one whose holder is gone so
    /// that the next poll can, or find it held.
    fn poll(&mut self) -> io::Result<CreateAttempt> {
        let liveness = self.held.lock();
        if liveness == Liveness::Busy {
            return Ok(CreateAttempt::Held);
        }
        match try_create_lock_dir(&self.path, self.deadline, &mut self.release_retry_started)? {
            CreateAttempt::Held => {}
            attempt => return Ok(attempt),
        }
        // Best-effort: a removal that fails is retried on the next poll,
        // and a directory already gone was removed by a process without
        // the held file.
        if is_abandoned(&self.path, self.abandoned_after, liveness) && remove_lock_dir(&self.path) {
            return Ok(CreateAttempt::Retry);
        }
        Ok(CreateAttempt::Held)
    }

    fn claim(self) -> io::Result<DirLock> {
        claim(self.path, self.held.into_locked())
    }
}

/// What the OS lock on the held file says about the lock directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Liveness {
    /// This process holds the file lock: no process that proves its
    /// liveness through it holds the directory.
    Proven,
    /// Another process holds the file lock, and with it the directory
    /// or the right to claim it.
    Busy,
    /// The file lock cannot be taken here, so the directory is the whole
    /// lock, as for an older pnpm.
    Unavailable,
}

/// The held file beside the lock directory, opened once per acquisition
/// and locked on the first poll that finds it free.
struct HeldFile {
    file: Option<File>,
    locked: bool,
}

impl HeldFile {
    fn open(lock_path: &Path) -> Self {
        let file = crate::open_secure_lock_file(&held_path(lock_path)).ok();
        Self { file, locked: false }
    }

    fn lock(&mut self) -> Liveness {
        let Some(file) = &self.file else {
            return Liveness::Unavailable;
        };
        if self.locked {
            return Liveness::Proven;
        }
        match file.try_lock() {
            Ok(()) => {
                self.locked = true;
                Liveness::Proven
            }
            Err(TryLockError::WouldBlock) => Liveness::Busy,
            Err(TryLockError::Error(_)) => {
                self.file = None;
                Liveness::Unavailable
            }
        }
    }

    fn into_locked(self) -> Option<File> {
        self.file.filter(|_| self.locked)
    }
}

fn held_path(lock_path: &Path) -> PathBuf {
    let mut held = lock_path.as_os_str().to_owned();
    held.push(HELD_FILE_SUFFIX);
    PathBuf::from(held)
}

/// What one attempt at creating the lock directory settled.
enum CreateAttempt {
    /// The directory is ours.
    Claimed,
    /// The attempt should be repeated at once: a release was still in
    /// flight and slept out, or an abandoned directory was removed.
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
        // Only release a lock that is still ours. A holder judged by age
        // alone — because it or its successor lacks the held file — may
        // have had its directory removed and replaced by the next process
        // in line; removing that one would hand the resource to two
        // processes at once.
        //
        // Reading the record and removing the directory are two steps, so
        // a takeover landing between them is still removable — the check
        // narrows the window from the whole guarded operation to a couple
        // of syscalls, it does not close it. Closing it needs an atomic
        // compare-and-remove no portable filesystem API offers. The
        // residual is acceptable because reaching it requires a takeover
        // by age, which requires this holder to have already run past
        // `abandoned_after` — a bound the caller sizes well above the
        // work being guarded.
        match fs::read_to_string(self.path.join(OWNER_FILE)) {
            Ok(owner) if owner != self.token => return,
            Err(error) if error.kind() != io::ErrorKind::NotFound => return,
            _ => {}
        }
        let _ = fs::remove_dir_all(&self.path);
        // Released only now, so no waiter gets the file lock while the
        // directory is still there.
        drop(self.held.take());
    }
}

/// Record this process as the owner of a lock directory it just created.
/// A lock that cannot be recorded is given back, since [`Drop`] would
/// have no way to tell at release time whether it is still ours.
fn claim(path: PathBuf, held: Option<File>) -> io::Result<DirLock> {
    let token = mint_token();
    if let Err(error) = record_claim(&path, &token, held.is_some()) {
        let _ = fs::remove_dir_all(&path);
        return Err(error);
    }
    Ok(DirLock { path, token, held })
}

/// The marker goes in before the record: a waiter holding the file lock
/// then takes over a claimer that died at any point after creating the
/// directory, either at once or after the claim grace.
fn record_claim(path: &Path, token: &str, holds_held_file: bool) -> io::Result<()> {
    if holds_held_file {
        fs::write(path.join(HELD_MARKER), "")?;
    }
    fs::write(path.join(OWNER_FILE), token)
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

/// Whether the lock directory at `path` belongs to nobody any more.
///
/// Holding the file lock settles it for a holder that marked the
/// directory: it would hold the file lock if it were still running. Any
/// other directory is judged by its age — `abandoned_after` once its
/// claimer recorded itself, the claim grace before that.
fn is_abandoned(path: &Path, abandoned_after: Duration, liveness: Liveness) -> bool {
    if liveness == Liveness::Proven && path.join(HELD_MARKER).exists() {
        return true;
    }
    let age = if path.join(OWNER_FILE).exists() {
        abandoned_after
    } else {
        CLAIM_GRACE.min(abandoned_after)
    };
    is_older_than(path, age)
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
/// counts as removed, and the next `create_dir` decides who claims it.
fn remove_lock_dir(path: &Path) -> bool {
    match fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod tests;
