use super::{
    ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION, RetryTiming, is_transient_file_lock_error,
    remove_dir_all_with_retry, rename_with_retry, retry_fs_operation,
    retry_fs_operation_with_timing,
};
use std::{cell::Cell, fs, io, time::Duration};
use tempfile::tempdir;

#[test]
fn retries_transient_errors_until_the_operation_succeeds() {
    let attempts = Cell::new(0);

    let result = retry_fs_operation(
        || {
            let attempt = attempts.get();
            attempts.set(attempt + 1);
            if attempt < 2 {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                Ok("done")
            }
        },
        |error| error.kind() == io::ErrorKind::PermissionDenied,
    );

    assert_eq!(result.unwrap(), "done");
    assert_eq!(attempts.get(), 3);
}

#[test]
fn propagates_non_transient_errors_without_retrying() {
    let attempts = Cell::new(0);

    let result: io::Result<()> = retry_fs_operation(
        || {
            attempts.set(attempts.get() + 1);
            Err(io::Error::from(io::ErrorKind::NotFound))
        },
        |_| false,
    );

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(attempts.get(), 1);
}

#[test]
fn stops_retrying_at_the_budget_deadline() {
    let attempts = Cell::new(0);
    let elapsed = Cell::new(Duration::ZERO);
    let budget = Duration::from_millis(15);

    let result: io::Result<()> = retry_fs_operation_with_timing(
        || {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            Err(io::Error::other(format!("attempt {attempt}")))
        },
        |_| true,
        RetryTiming {
            budget,
            elapsed: || elapsed.get(),
            sleep: |delay| elapsed.set(elapsed.get() + delay),
        },
    );

    assert_eq!(result.unwrap_err().to_string(), "attempt 3");
    assert_eq!(attempts.get(), 3);
    assert_eq!(elapsed.get(), budget);
}

#[test]
fn transient_file_lock_error_classifier_is_windows_specific() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::ResourceBusy] {
        let error = io::Error::from(kind);
        assert_eq!(is_transient_file_lock_error(&error), cfg!(windows), "{kind:?}");
    }

    for code in [ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION] {
        let error = io::Error::from_raw_os_error(code);
        assert_eq!(is_transient_file_lock_error(&error), cfg!(windows), "os error {code}");
    }

    for kind in [
        io::ErrorKind::NotFound,
        io::ErrorKind::AlreadyExists,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::InvalidData,
        io::ErrorKind::Unsupported,
        io::ErrorKind::Other,
    ] {
        assert!(!is_transient_file_lock_error(&io::Error::from(kind)), "{kind:?}");
    }
}

#[test]
fn rename_with_retry_moves_the_entry() {
    let root = tempdir().unwrap();
    let src = root.path().join("src");
    let dst = root.path().join("dst");
    fs::write(&src, b"payload").unwrap();

    rename_with_retry(&src, &dst).expect("rename should succeed");

    assert_eq!(fs::read(&dst).unwrap(), b"payload");
    assert!(!src.exists(), "source should be gone after rename");
}

#[test]
fn remove_dir_all_with_retry_removes_the_tree() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir_all(target.join("nested")).unwrap();
    fs::write(target.join("nested/file"), b"payload").unwrap();

    remove_dir_all_with_retry(&target).expect("remove should succeed");

    assert!(!target.exists(), "directory tree should be gone after removal");
}
