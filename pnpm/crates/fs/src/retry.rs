use std::{fs, io, path::Path};

#[cfg(any(windows, test))]
use std::time::{Duration, Instant};

#[cfg(any(windows, test))]
const RETRY_BUDGET: Duration = Duration::from_mins(1);
#[cfg(any(windows, test))]
const RETRY_BACKOFF_CAP: Duration = Duration::from_millis(100);

/// Rename a filesystem entry, retrying transient Windows file-lock errors.
pub fn rename_with_retry(src: &Path, dst: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| fs::rename(src, dst))
}

/// Remove a directory tree, retrying transient Windows file-lock errors.
pub fn remove_dir_all_with_retry(path: &Path) -> io::Result<()> {
    retry_transient_file_locks(|| fs::remove_dir_all(path))
}

/// Run a filesystem operation, retrying it with a bounded backoff while
/// it fails with a [transient Windows file lock](is_transient_file_lock_error).
/// On Unix the operation runs exactly once.
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

#[cfg(any(windows, test))]
fn retry_fs_operation<Func, Value, Classify>(
    operation: Func,
    is_transient: Classify,
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
            elapsed: || start.elapsed(),
            sleep: std::thread::sleep,
        },
    )
}

#[cfg(any(windows, test))]
struct RetryTiming<Elapsed, Sleep> {
    budget: Duration,
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
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if !is_transient(&error) || (timing.elapsed)() >= timing.budget {
                    return Err(error);
                }
                let remaining = timing.budget.saturating_sub((timing.elapsed)());
                let delay = backoff.min(remaining);
                if !delay.is_zero() {
                    (timing.sleep)(delay);
                }
                if (timing.elapsed)() >= timing.budget {
                    return Err(error);
                }
                backoff = (backoff + Duration::from_millis(10)).min(RETRY_BACKOFF_CAP);
            }
        }
    }
}

/// Whether `error` is the kind of failure an antivirus or indexer scan
/// causes by briefly holding a Windows path open: `ERROR_ACCESS_DENIED`
/// (a directory rename blocked by an open handle below it),
/// `ERROR_SHARING_VIOLATION` / `ERROR_LOCK_VIOLATION` (an open or delete
/// refused by another handle's share mode), or `ERROR_BUSY`. The sharing
/// and lock violations have no [`io::ErrorKind`] of their own, so they
/// are matched by raw OS error.
///
/// Never `true` on Unix: the equivalent error kinds there usually mean a
/// permanent permissions or mount-point problem, so retrying them would
/// only delay the failure.
pub(crate) fn is_transient_file_lock_error(
    #[cfg_attr(not(windows), allow(unused, reason = "only inspected on Windows"))]
    error: &io::Error,
) -> bool {
    #[cfg(windows)]
    {
        const ERROR_SHARING_VIOLATION: i32 = 32;
        const ERROR_LOCK_VIOLATION: i32 = 33;
        matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy)
            || matches!(error.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests;
