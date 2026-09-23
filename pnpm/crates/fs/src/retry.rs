use std::{fs, io, path::Path};

#[cfg(any(windows, test))]
use std::time::{Duration, Instant};

#[cfg(any(windows, test))]
const RETRY_BUDGET: Duration = Duration::from_mins(1);
#[cfg(any(windows, test))]
const PERMISSION_DENIED_RETRY_BUDGET: Duration = Duration::from_secs(1);
/// How long a removal waits out access denied. Windows also reports access
/// denied for an executable that a running process has loaded, so this
/// covers a program under `node_modules` that is shutting down, while a
/// restrictive ACL still fails the removal within seconds.
#[cfg(any(windows, test))]
const REMOVAL_PERMISSION_DENIED_RETRY_BUDGET: Duration = Duration::from_secs(5);
#[cfg(any(windows, test))]
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
/// retrying would only delay the failure.
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
    #[cfg(windows)]
    {
        retry_fs_operation(operation, is_transient_file_lock_error)
    }
    #[cfg(not(windows))]
    {
        let mut operation = operation;
        operation()
    }
}

/// Run a removal with the retry policy of [`rename_with_retry`], except that
/// permission errors get the longer removal budget on Windows.
pub(crate) fn retry_transient_removal_locks<Value>(
    operation: impl FnMut() -> io::Result<Value>,
) -> io::Result<Value> {
    #[cfg(windows)]
    {
        retry_fs_operation_within(
            operation,
            is_transient_file_lock_error,
            REMOVAL_PERMISSION_DENIED_RETRY_BUDGET,
        )
    }
    #[cfg(not(windows))]
    {
        let mut operation = operation;
        operation()
    }
}

#[cfg(any(windows, test))]
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

#[cfg(any(windows, test))]
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

#[cfg(any(windows, test))]
struct RetryTiming<Elapsed, Sleep> {
    budget: Duration,
    /// The budget once any attempt fails with a permission error other than
    /// a sharing or lock violation.
    permission_denied_budget: Duration,
    elapsed: Elapsed,
    sleep: Sleep,
}

#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
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
/// by an open handle below it), [`ERROR_SHARING_VIOLATION`] or
/// [`ERROR_LOCK_VIOLATION`] (an open or delete refused by another handle's
/// share mode), or `ERROR_BUSY`. The sharing and lock violations have no
/// [`io::ErrorKind`] of their own, so they are matched by raw OS error.
/// Always `false` on Unix.
pub(crate) fn is_transient_file_lock_error(error: &io::Error) -> bool {
    cfg!(windows)
        && (matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy)
            || matches!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)))
}

#[cfg(test)]
mod tests;
