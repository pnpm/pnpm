#[cfg(unix)]
use super::{
    super::{
        Host, LINK_STATE_CLONE, LINK_STATE_COPY, LINK_STATE_HARDLINK, auto_link, clone_or_copy_link,
    },
    ModeDroppingClone, STORE_ENTRY, write_source,
};
#[cfg(unix)]
use pnpm_fs::StoreFile;
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

/// The clone tier aligns a reflinked target only when this install has not
/// already read the source's mode as the desired one: a reflink copies the
/// source's mode, so the alignment's `open` would change nothing.
#[test]
#[cfg(unix)]
fn clone_tier_aligns_only_when_the_source_mode_is_unknown_or_undesired() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let desired = pnpm_fs::file_mode::store_entry_mode(false, pnpm_fs::file_mode::current_umask());
    let src = write_source(tmp.path(), STORE_ENTRY, b"data\n").path;
    fs::set_permissions(&src, fs::Permissions::from_mode(desired)).unwrap();
    let target_mode = |source: StoreFile, name: &str| {
        let dst = tmp.path().join(name);
        auto_link::<SilentReporter, ModeDroppingClone>(
            &AtomicU8::new(0),
            &AtomicU8::new(LINK_STATE_CLONE),
            &source,
            &dst,
        )
        .expect("reflink should succeed");
        fs::metadata(&dst)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };

    assert_eq!(target_mode(StoreFile::with_mode(src.clone(), Some(desired)), "known"), 0o400);
    assert_eq!(target_mode(StoreFile::new(src.clone()), "unknown"), desired);
    assert_eq!(target_mode(StoreFile::with_mode(src, Some(desired ^ 0o004)), "stale"), desired);
}
