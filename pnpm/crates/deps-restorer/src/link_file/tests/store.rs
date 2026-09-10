#[cfg(unix)]
use super::{
    super::{Host, LINK_STATE_COPY, LINK_STATE_HARDLINK, auto_link, clone_or_copy_link},
    write_source,
};
#[cfg(unix)]
use pnpm_reporter::SilentReporter;
#[cfg(unix)]
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::{
    fs,
    sync::atomic::{AtomicU8, Ordering},
};
#[cfg(unix)]
use tempfile::tempdir;

/// Once `Auto`'s state is `COPY`, we use `fs::copy` and must not
/// re-attempt reflink / hardlink. Observable: a successful link
/// with state pre-seeded to `COPY` has independent inodes (copy
/// semantics), not shared ones (hardlink).
#[test]
#[cfg(unix)]
fn auto_respects_cached_copy_state() {
    use std::os::unix::fs::MetadataExt;

    let state = AtomicU8::new(LINK_STATE_COPY);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"cached-copy");
    let dst = tmp.path().join("dst.txt");

    auto_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("copy should succeed");

    assert_eq!(fs::read(&dst).unwrap(), b"cached-copy");
    assert_ne!(
        fs::metadata(&src).unwrap().ino(),
        fs::metadata(&dst).unwrap().ino(),
        "state=COPY must not hardlink",
    );
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY, "state must not drift");
}
/// State=HARDLINK means Auto skips the reflink attempt and jumps
/// straight to `fs::hard_link`. Observable: shared inode on unix.
#[test]
#[cfg(unix)]
fn auto_respects_cached_hardlink_state() {
    use std::os::unix::fs::MetadataExt;

    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"cached-hardlink");
    let dst = tmp.path().join("dst.txt");

    auto_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("hardlink should succeed on same-FS tempdir");

    assert_eq!(
        fs::metadata(&src).unwrap().ino(),
        fs::metadata(&dst).unwrap().ino(),
        "state=HARDLINK must hardlink, not copy",
    );
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_HARDLINK, "state must not drift");
}
/// Pre-seed `CloneOrCopy` state to `COPY` and verify it uses
/// `fs::copy` — mirrors [`auto_respects_cached_copy_state`]. Also
/// confirms we skip the hardlink tier entirely (pnpm
/// `createCloneOrCopyImporter` has no hardlink fallback).
#[test]
#[cfg(unix)]
fn clone_or_copy_respects_cached_copy_state() {
    use std::os::unix::fs::MetadataExt;

    let state = AtomicU8::new(LINK_STATE_COPY);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"cached");
    let dst = tmp.path().join("dst.txt");

    clone_or_copy_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("copy should succeed");

    assert_ne!(
        fs::metadata(&src).unwrap().ino(),
        fs::metadata(&dst).unwrap().ino(),
        "state=COPY must not hardlink",
    );
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY, "state must not drift");
}
