use crate::{remove_dirent, test_support::with_retry_observer};
use std::{fs, io, os::windows::fs::OpenOptionsExt, path::Path, sync::mpsc};
use tempfile::tempdir;

/// Remove `target` while `locked` is held open without `FILE_SHARE_DELETE`,
/// the way an editor or indexer holds a file under `node_modules`.
fn assert_removal_recovers_after_lock(target: &Path, locked: &Path) {
    // Permit normal reads and writes, but omit FILE_SHARE_DELETE.
    let mut handle = Some(
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(locked)
            .unwrap(),
    );
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
            if attempt.is_err() {
                // Release only after a real failed attempt, not after a scheduled delay.
                drop(handle.take());
            }
        },
        || remove_dirent(target),
    );
    let attempts: Vec<_> = receiver.try_iter().collect();
    eprintln!("removal result: {result:?}; real filesystem attempts: {attempts:?}");
    result.expect("the removal must recover after the deny-delete handle closes");
    assert!(matches!(attempts.first(), Some(Err(Some(5 | 32 | 33)))));
    assert_eq!(attempts.last(), Some(&Ok(())));
    let metadata = fs::symlink_metadata(target);
    assert!(
        matches!(&metadata, Err(error) if error.kind() == io::ErrorKind::NotFound),
        "the removed entry must be gone, got {metadata:?}",
    );
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
