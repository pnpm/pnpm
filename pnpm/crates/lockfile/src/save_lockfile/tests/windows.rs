use super::LOCKFILE_YAML;
use crate::Lockfile;
use pnpm_fs::test_support::with_retry_observer;
use std::{fs, io, os::windows::fs::OpenOptionsExt, sync::mpsc};
use tempfile::tempdir;

/// An editor or indexer holding `pnpm-lock.yaml` open without delete sharing
/// blocks the rename that publishes the new lockfile until it lets go.
#[test]
fn save_recovers_after_the_lockfile_is_briefly_held() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(Lockfile::FILE_NAME);
    fs::write(&path, "lockfileVersion: '9.0'\n").unwrap();
    // Permit normal reads and writes, but omit FILE_SHARE_DELETE.
    let mut handle = Some(
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&path)
            .unwrap(),
    );
    let lockfile: Lockfile = serde_saphyr::from_str(LOCKFILE_YAML).unwrap();

    let (sender, receiver) = mpsc::channel();
    let result = with_retry_observer(
        &path,
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
        || lockfile.save_to_path(&path),
    );
    let attempts: Vec<_> = receiver.try_iter().collect();
    eprintln!("save result: {result:?}; real filesystem attempts: {attempts:?}");

    result.expect("the save must recover once the holder lets go");
    assert!(matches!(attempts.first(), Some(Err(Some(5 | 32 | 33)))));
    assert_eq!(attempts.last(), Some(&Ok(())));
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("{LOCKFILE_YAML}\n"));
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name != Lockfile::FILE_NAME)
        .collect();
    assert!(leftovers.is_empty(), "temp file should have been cleaned up, found: {leftovers:?}");
}
