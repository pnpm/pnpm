use crate::{remove_dirent, test_support::with_retry_observer};
use std::{
    env, fs, io,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use tempfile::tempdir;

/// Remove `target` while something holds it, and run `release` from the
/// retry observer on the first failed attempt that comes at least
/// `held_for` after the first failed attempt. Releasing only after real
/// failed attempts keeps the outcome independent of scheduling.
fn assert_removal_recovers(
    target: &Path,
    expected_error: i32,
    held_for: Duration,
    release: impl FnOnce() + Send + 'static,
) {
    let mut release = Some(release);
    let mut first_failure = None;
    let (sender, receiver) = mpsc::channel();
    let result = with_retry_observer(
        target,
        move |attempt| {
            sender
                .send(
                    attempt
                        .as_ref()
                        .copied()
                        .map_err(io::Error::raw_os_error),
                )
                .unwrap();
            if attempt.is_err()
                && first_failure.get_or_insert_with(Instant::now).elapsed() >= held_for
                && let Some(release) = release.take()
            {
                release();
            }
        },
        || remove_dirent(target),
    );
    let attempts: Vec<_> = receiver.try_iter().collect();
    eprintln!("removal result: {result:?}; real filesystem attempts: {attempts:?}");
    result.expect("the removal must recover once the holder lets go");
    let (success, failures) = attempts.split_last().expect("the removal must have been attempted");
    assert_eq!(success, &Ok(()));
    assert!(!failures.is_empty(), "the held entry must fail at least one attempt");
    assert!(failures.iter().all(Result::is_err), "only the last attempt may succeed");
    assert!(
        failures.contains(&Err(Some(expected_error))),
        "a failed attempt must report os error {expected_error}",
    );
    let metadata = fs::symlink_metadata(target);
    assert!(
        matches!(&metadata, Err(error) if error.kind() == io::ErrorKind::NotFound),
        "the removed entry must be gone, got {metadata:?}",
    );
}

/// Remove `target` while `locked` is held open without `FILE_SHARE_DELETE`,
/// the way an editor or indexer holds a file under `node_modules`.
fn assert_removal_recovers_after_lock(target: &Path, locked: &Path) {
    // Permit normal reads and writes, but omit FILE_SHARE_DELETE.
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(locked)
        .unwrap();
    const ERROR_SHARING_VIOLATION: i32 = 32;
    assert_removal_recovers(target, ERROR_SHARING_VIOLATION, Duration::ZERO, move || {
        drop(handle);
    });
}

#[test]
fn file_removal_recovers_after_transient_lock() {
    let root = tempdir().unwrap();
    let file = root.path().join("index.js");
    fs::write(&file, "module.exports = true").unwrap();

    assert_removal_recovers_after_lock(&file, &file);
}

/// The shape of pnpm/pnpm#15081: `pnpm clean` removes `node_modules/.pnpm`
/// while an editor holds one of the package files open.
#[test]
fn directory_removal_recovers_after_transient_lock() {
    let root = tempdir().unwrap();
    let virtual_store = root.path().join(".pnpm");
    let package = virtual_store.join("is-positive@3.1.0/node_modules/is-positive");
    fs::create_dir_all(&package).unwrap();
    let locked = package.join("index.js");
    fs::write(&locked, "module.exports = true").unwrap();
    fs::write(package.join("package.json"), "{}").unwrap();

    assert_removal_recovers_after_lock(&virtual_store, &locked);
}

/// A running program's executable cannot be deleted, and Windows reports
/// that with the access denied that a restrictive ACL also gives. The
/// program keeps running until the removal has failed for longer than the
/// one second that other retried operations give access denied.
/// Other failures, such as a scanner briefly holding the fresh copy, may
/// come first.
#[test]
fn directory_removal_waits_out_a_running_executable() {
    let root = tempdir().unwrap();
    let package = root.path().join("node_modules/@oxlint/binding-win32-x64-msvc");
    fs::create_dir_all(&package).unwrap();
    let executable = package.join("oxlint.exe");
    let cmd = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set on Windows")
        .join(r"System32\cmd.exe");
    fs::copy(cmd, &executable).unwrap();
    // The copy of cmd.exe reads commands from its stdin and runs until it is
    // killed or its stdin closes.
    let mut program = Command::new(&executable)
        .args(["/d", "/q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    const ERROR_ACCESS_DENIED: i32 = 5;
    let target = root.path().join("node_modules/@oxlint");

    assert_removal_recovers(&target, ERROR_ACCESS_DENIED, Duration::from_secs(2), move || {
        program.kill().unwrap();
        program.wait().unwrap();
    });
}
