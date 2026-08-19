use super::read_regular_file_capped;
use std::fs;

#[test]
fn reads_a_regular_file_under_the_cap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("f");
    fs::write(&path, b"hello").unwrap();
    assert_eq!(read_regular_file_capped(&path, 64).unwrap().unwrap(), b"hello");
}

#[test]
fn absent_reads_as_none_and_oversized_as_an_error() {
    let temp = tempfile::tempdir().unwrap();
    assert!(read_regular_file_capped(&temp.path().join("missing"), 64).unwrap().is_none());
    let path = temp.path().join("big");
    fs::write(&path, vec![0u8; 65]).unwrap();
    assert!(read_regular_file_capped(&path, 64).is_err());
}

#[test]
#[cfg(unix)]
fn a_symlink_is_refused_at_open() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::write(&target, b"x").unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(read_regular_file_capped(&link, 64).is_err(), "symlinks must not be followed");
}

#[test]
#[cfg(target_os = "linux")]
fn a_fifo_is_refused_without_blocking() {
    let temp = tempfile::tempdir().unwrap();
    let fifo = temp.path().join("fifo");
    let status = std::process::Command::new("mkfifo").arg(&fifo).status().expect("run mkfifo");
    assert!(status.success(), "mkfifo failed");
    // The whole point: this returns instead of hanging on the open.
    assert!(read_regular_file_capped(&fifo, 64).is_err());
}

#[test]
#[cfg(windows)]
fn a_junction_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target-dir");
    std::fs::create_dir(&target).unwrap();
    let link = temp.path().join("junction");
    junction::create(&target, &link).unwrap();
    assert!(read_regular_file_capped(&link, 64).is_err(), "reparse points must be refused");
}

/// The junction test alone can't tell no-follow from the regular-file
/// check (a followed junction still opens a directory). A file symlink
/// pointing at a small regular file can: if the open followed it, the
/// read would succeed and return the target's bytes.
#[test]
#[cfg(windows)]
fn a_file_symlink_is_refused_not_followed() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target.yaml");
    fs::write(&target, b"x").unwrap();
    let link = temp.path().join("link.yaml");
    // File-symlink creation needs a privilege (or developer mode) the
    // environment may not grant; a fixture we cannot build is a skip,
    // not a failure — the junction test above still covers the reparse
    // rejection itself.
    if std::os::windows::fs::symlink_file(&target, &link).is_err() {
        eprintln!("skipping: this environment cannot create file symlinks");
        return;
    }
    assert!(
        read_regular_file_capped(&link, 64).is_err(),
        "a followed symlink would have read the target's bytes",
    );
}
