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
    mut operation: Func,
    is_transient: Classify,
) -> io::Result<Value>
where
    Func: FnMut() -> io::Result<Value>,
    Classify: Fn(&io::Error) -> bool,
{
    let mut backoff = Duration::ZERO;
    let start = Instant::now();

    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if !is_transient(&error) || start.elapsed() >= RETRY_BUDGET {
                    return Err(error);
                }
                if !backoff.is_zero() {
                    std::thread::sleep(backoff);
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
