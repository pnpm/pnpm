use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

const RETRY_BUDGET: Duration = Duration::from_mins(1);
const PERMISSION_DENIED_RETRY_BUDGET: Duration = Duration::from_secs(1);
/// How long a removal waits out access denied. Windows also reports access
/// denied for an executable that a running process has loaded, so this
/// covers a program under `node_modules` that is shutting down, while a
/// restrictive ACL still fails the removal within seconds.
const REMOVAL_PERMISSION_DENIED_RETRY_BUDGET: Duration = Duration::from_secs(5);
const RETRY_BACKOFF_CAP: Duration = Duration::from_millis(100);

pub(crate) const ERROR_SHARING_VIOLATION: i32 = 32;
pub(crate) const ERROR_LOCK_VIOLATION: i32 = 33;

/// Rename a filesystem entry, retrying transient Windows file-lock errors.
///
/// Antivirus and indexer scans briefly hold Windows paths open, failing an
/// unlucky rename or removal with an access-denied, sharing-violation, or
/// busy error that clears moments later. Such errors are retried with a
/// bounded backoff for up to one minute before the last one is returned.
/// Permission errors have a one-second budget because they can also indicate
/// permanent ACL, read-only, or destination-type conflicts.
/// On Unix the operation runs exactly once: the equivalent error kinds
/// there usually mean a permanent permissions or mount-point problem, so
/// retrying would only delay the failure. WSL is the exception: a Windows
/// drive mounted there keeps Windows locking.
pub fn rename_with_retry(src: &Path, dst: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| {
        let result = fs::rename(src, dst);
        #[cfg(all(windows, feature = "test"))]
        crate::test_support::notify_attempt(dst, &result);
        result
    })
}

/// Remove a file with the retry policy of [`rename_with_retry`].
pub fn remove_file_with_retry(path: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| {
        let result = fs::remove_file(path);
        #[cfg(all(windows, feature = "test"))]
        crate::test_support::notify_attempt(path, &result);
        result
    })
}

/// Remove a directory tree with the retry policy of [`rename_with_retry`].
pub fn remove_dir_all_with_retry(path: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| fs::remove_dir_all(path))
}

/// Create a directory, with the retry policy of [`rename_with_retry`].
pub fn create_dir_with_retry(path: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| fs::create_dir(path))
}

/// Create a directory and every missing parent, with the retry policy of
/// [`rename_with_retry`].
pub fn create_dir_all_with_retry(path: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| fs::create_dir_all(path))
}

/// Remove an empty directory, with the retry policy of
/// [`rename_with_retry`].
pub fn remove_dir_with_retry(path: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| fs::remove_dir(path))
}

/// Read metadata following symlinks, with the retry policy of [`rename_with_retry`].
pub fn metadata_with_retry(path: &Path) -> io::Result<fs::Metadata> {
    retry_transient_file_locks(|| fs::metadata(path))
}

/// Read a dirent's metadata without following it, with the retry policy of
/// [`rename_with_retry`].
///
/// A Windows path another process has just unlinked is *delete-pending*
/// until the last handle on it closes, and inspecting it answers
/// `PermissionDenied` rather than `NotFound` for as long as that lasts.
/// Retrying lets the unlink land, so a caller that reads `NotFound` as an
/// absent target sees the same absence Unix shows it at once.
pub fn symlink_metadata_with_retry(path: &Path) -> io::Result<fs::Metadata> {
    retry_transient_file_locks(|| fs::symlink_metadata(path))
}

/// Run a filesystem operation with the retry policy of [`rename_with_retry`];
/// [`is_transient_file_lock_error`] decides which failures are retried.
pub(crate) fn retry_transient_file_locks<Value>(
    operation: impl FnMut() -> io::Result<Value>,
) -> io::Result<Value> {
    if file_locks_are_transient() {
        retry_fs_operation(operation, is_transient_file_lock_error)
    } else {
        let mut operation = operation;
        operation()
    }
}

/// Run a removal with the retry policy of [`rename_with_retry`], except that
/// permission errors get the longer removal budget.
pub(crate) fn retry_transient_removal_locks<Value>(
    operation: impl FnMut() -> io::Result<Value>,
) -> io::Result<Value> {
    if file_locks_are_transient() {
        retry_fs_operation_within(
            operation,
            is_transient_file_lock_error,
            REMOVAL_PERMISSION_DENIED_RETRY_BUDGET,
        )
    } else {
        let mut operation = operation;
        operation()
    }
}

fn retry_fs_operation<Func, Value, Classify>(
    operation: Func,
    is_transient: Classify,
) -> io::Result<Value>
where
    Func: FnMut() -> io::Result<Value>,
    Classify: Fn(&io::Error) -> bool,
{
    retry_fs_operation_within(operation, is_transient, PERMISSION_DENIED_RETRY_BUDGET)
}

fn retry_fs_operation_within<Func, Value, Classify>(
    operation: Func,
    is_transient: Classify,
    permission_denied_budget: Duration,
) -> io::Result<Value>
where
    Func: FnMut() -> io::Result<Value>,
    Classify: Fn(&io::Error) -> bool,
{
    let start = Instant::now();
    retry_fs_operation_with_timing(
        operation,
        is_transient,
        RetryTiming {
            budget: RETRY_BUDGET,
            permission_denied_budget,
            elapsed: || start.elapsed(),
            sleep: std::thread::sleep,
        },
    )
}

struct RetryTiming<Elapsed, Sleep> {
    budget: Duration,
    /// The budget once any attempt fails with a permission error other than
    /// a sharing or lock violation.
    permission_denied_budget: Duration,
    elapsed: Elapsed,
    sleep: Sleep,
}

fn retry_fs_operation_with_timing<Func, Value, Classify, Elapsed, Sleep>(
    mut operation: Func,
    is_transient: Classify,
    mut timing: RetryTiming<Elapsed, Sleep>,
) -> io::Result<Value>
where
    Func: FnMut() -> io::Result<Value>,
    Classify: Fn(&io::Error) -> bool,
    Elapsed: FnMut() -> Duration,
    Sleep: FnMut(Duration),
{
    let mut backoff = Duration::ZERO;

    loop {
        let error = match operation() {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        if error.kind() == io::ErrorKind::PermissionDenied
            && !matches!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION))
        {
            timing.budget = timing.budget.min(timing.permission_denied_budget);
        }
        if !is_transient(&error) || !wait_for_retry(&mut timing, backoff) {
            return Err(error);
        }
        backoff = (backoff + Duration::from_millis(10)).min(RETRY_BACKOFF_CAP);
    }
}

/// Sleep out one backoff, capped to what is left of the budget. Reports
/// whether the budget still allows another attempt — checked both before the
/// sleep and after it, since the sleep itself consumes budget.
fn wait_for_retry<Elapsed, Sleep>(
    timing: &mut RetryTiming<Elapsed, Sleep>,
    backoff: Duration,
) -> bool
where
    Elapsed: FnMut() -> Duration,
    Sleep: FnMut(Duration),
{
    if (timing.elapsed)() >= timing.budget {
        return false;
    }
    let remaining = timing.budget.saturating_sub((timing.elapsed)());
    let delay = backoff.min(remaining);
    if !delay.is_zero() {
        (timing.sleep)(delay);
    }
    (timing.elapsed)() < timing.budget
}

/// Whether `error` is a transient Windows file lock in the sense of
/// [`rename_with_retry`]: `ERROR_ACCESS_DENIED` (a directory rename blocked
/// by an open handle below it), a sharing or lock violation (OS errors 32 and
/// 33, an open or delete refused by another handle's share mode), or
/// `ERROR_BUSY`. The sharing and lock violations have no
/// [`io::ErrorKind`] of their own, so they are matched by raw OS error.
/// Under WSL the same locks surface as `EACCES`/`EPERM` or `EBUSY`. Always
/// `false` on other Unix systems.
#[must_use]
pub fn is_transient_file_lock_error(error: &io::Error) -> bool {
    file_locks_are_transient()
        && (matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy)
            || (cfg!(windows)
                && matches!(
                    error.raw_os_error(),
                    Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION),
                )))
}

/// Whether filesystem errors may come from another process's transient
/// Windows file lock. True on Windows and under WSL, where a Windows drive
/// mounted at `/mnt/<letter>` keeps Windows locking: a rename or removal
/// blocked by an antivirus or indexer handle fails with `EACCES` there.
pub(crate) fn file_locks_are_transient() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        static UNDER_WSL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *UNDER_WSL.get_or_init(|| {
            fs::read_to_string("/proc/sys/kernel/osrelease")
                .is_ok_and(|release| is_wsl_kernel_release(&release))
        })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// WSL kernels identify themselves in their release string, e.g.
/// `5.15.167.4-microsoft-standard-WSL2` or `4.4.0-19041-Microsoft` (WSL 1).
#[cfg_attr(
    not(any(target_os = "linux", test)),
    expect(dead_code, reason = "only Linux can run under WSL")
)]
fn is_wsl_kernel_release(release: &str) -> bool {
    release.to_ascii_lowercase().contains("microsoft")
}

#[cfg(test)]
mod tests;
