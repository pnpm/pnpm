use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

const RETRY_BUDGET: Duration = Duration::from_mins(1);
const RETRY_BACKOFF_CAP: Duration = Duration::from_millis(100);

/// Rename a filesystem entry, retrying transient Windows file-lock errors.
pub fn rename_with_retry(src: &Path, dst: &Path) -> io::Result<()> {
    retry_fs_operation(|| fs::rename(src, dst), is_transient_file_lock_error)
}

/// Remove a directory tree, retrying transient Windows file-lock errors.
pub fn remove_dir_all_with_retry(path: &Path) -> io::Result<()> {
    retry_fs_operation(|| fs::remove_dir_all(path), is_transient_file_lock_error)
}

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

struct RetryTiming<Elapsed, Sleep> {
    budget: Duration,
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

fn is_transient_file_lock_error(
    #[cfg_attr(not(windows), allow(unused, reason = "only inspected on Windows"))]
    error: &io::Error,
) -> bool {
    // Antivirus and indexer scans can briefly hold a Windows path open. The equivalent error
    // kinds on Unix usually mean a permanent permissions or mount-point problem, so retrying
    // them there would only delay the failure.
    #[cfg(windows)]
    {
        matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ResourceBusy)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests;
