#[cfg(unix)]
use super::LINK_STATE_COPY;
use super::{
    LINK_STATE_CLONE, LINK_STATE_HARDLINK, LinkFileError, auto_link, clone_or_copy_link,
    is_call_error, is_cross_device, link_file,
};
use pacquet_config::PackageImportMethod;
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU8, Ordering},
};
use tempfile::tempdir;

fn write_source(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("write source file");
    path
}

/// `Copy` always succeeds regardless of filesystem capabilities, so
/// it's the safest method to assert against on CI.
#[test]
fn copy_materializes_the_file_contents() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"hello");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Copy, &src, &dst)
        .expect("link_file should succeed");

    assert_eq!(fs::read(&dst).unwrap(), b"hello");
    // A plain copy leaves the two files as independent inodes.
    let src_ino = fs::metadata(&src).unwrap();
    let dst_ino = fs::metadata(&dst).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_ne!(src_ino.ino(), dst_ino.ino());
    }
    #[cfg(not(unix))]
    let _ = (src_ino, dst_ino);
}

/// Hardlinking in the same directory on the same filesystem works on
/// every mainstream OS the project supports. We verify the post-link
/// inodes match (on unix) or that the contents match (otherwise).
#[test]
fn hardlink_shares_contents_with_source() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"shared");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Hardlink, &src, &dst)
        .expect("link_file should succeed");

    assert_eq!(fs::read(&dst).unwrap(), b"shared");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let src_meta = fs::metadata(&src).unwrap();
        let dst_meta = fs::metadata(&dst).unwrap();
        assert_eq!(src_meta.ino(), dst_meta.ino(), "hardlinked files share an inode");
        eprintln!("src nlink={}, dst nlink={}", src_meta.nlink(), dst_meta.nlink());
        assert!(src_meta.nlink() >= 2, "hardlink should bump nlink");
    }
}

/// `Auto` must succeed on any filesystem because it falls through to
/// `fs::copy`. We point it at a `tmpfs`-like temp dir — reflink and
/// hardlink may or may not be available, but copy always is.
#[test]
fn auto_falls_through_to_a_working_method() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"auto");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Auto, &src, &dst)
        .expect("Auto should always succeed");
    assert_eq!(fs::read(&dst).unwrap(), b"auto");
}

/// If the target already exists, `link_file` is a no-op — it must not
/// error (which `fs::hard_link` / `reflink` would do on their own) or
/// overwrite the existing contents.
#[test]
fn existing_target_is_preserved() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"new");
    let dst = tmp.path().join("dst.txt");
    fs::write(&dst, b"old").unwrap();

    for method in [
        PackageImportMethod::Auto,
        PackageImportMethod::Copy,
        PackageImportMethod::Hardlink,
        PackageImportMethod::Clone,
        PackageImportMethod::CloneOrCopy,
    ] {
        link_file::<SilentReporter>(&AtomicU8::new(0), method, &src, &dst)
            .expect("existing target should short-circuit");
        assert_eq!(fs::read(&dst).unwrap(), b"old", "method {method:?} must not overwrite");
    }
}

/// Explicit `Hardlink` must surface non-`EXDEV` link-creation errors
/// instead of silently falling back — matches pnpm's `linkOrCopy`,
/// which only swallows `EXDEV` (and a couple of other kernel-level
/// "not permitted" codes, not modelled here). We drive the error
/// path by pointing at a non-existent source (`NotFound`, which is
/// not `EXDEV`) so the failure is deterministic on every platform.
#[test]
fn explicit_hardlink_surfaces_errors() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("does-not-exist");
    let dst = tmp.path().join("dst.txt");

    let err =
        link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Hardlink, &src, &dst)
            .expect_err("no source → error");
    assert!(matches!(err, LinkFileError::Import { .. }), "got: {err:?}");
}

/// `CloneOrCopy` has to succeed on any filesystem because
/// `clone_or_copy_link` falls back to `fs::copy` when the reflink
/// attempt fails with a capability error. This hits the match arm
/// directly — the `existing_target_is_preserved` loop
/// short-circuits before the arm ever runs, so without this we had
/// no coverage of the real code path.
#[test]
fn clone_or_copy_materializes_the_file_contents() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"clone-or-copy");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::CloneOrCopy, &src, &dst)
        .expect("CloneOrCopy should always succeed");
    assert_eq!(fs::read(&dst).unwrap(), b"clone-or-copy");
}

/// Explicit `Clone` must propagate errors rather than silently
/// copying. Pointing at a non-existent source gives us a
/// deterministic failure on every FS regardless of reflink
/// support, so the test doesn't need a btrfs / APFS runner.
#[test]
fn explicit_clone_surfaces_errors() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("does-not-exist");
    let dst = tmp.path().join("dst.txt");

    let err =
        link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Clone, &src, &dst)
            .expect_err("no source → error");
    assert!(matches!(err, LinkFileError::Import { .. }), "got: {err:?}");
}

/// A dangling symlink left behind by an interrupted install is left
/// alone. Matches pnpm's `linkOrCopy` (`fs/indexed-pkg-importer/src/index.ts`),
/// which returns on `EEXIST` without inspecting the dirent — the
/// downside is that a dangling symlink survives until something
/// rewrites the slot, the upside is a single import syscall per file
/// instead of stat-then-link-then-maybe-unlink. The pre-flight
/// `fs::metadata` short-circuit in `link_file` does not fire for a
/// dangling symlink (the syscall follows the link and returns
/// `NotFound`), so the import syscall runs and surfaces `EEXIST`,
/// which we treat as a no-op.
#[test]
#[cfg(unix)]
fn dangling_symlink_is_preserved() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"fresh");
    let dst = tmp.path().join("dst.txt");
    let dangling_target = tmp.path().join("never-created");
    std::os::unix::fs::symlink(&dangling_target, &dst).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Hardlink, &src, &dst)
        .expect("EEXIST must be treated as no-op, matching pnpm");

    let meta = fs::symlink_metadata(&dst).unwrap();
    eprintln!("dst file_type={:?}", meta.file_type());
    assert!(meta.file_type().is_symlink(), "dangling symlink stays in place");
    assert_eq!(std::fs::read_link(&dst).unwrap(), dangling_target, "target unchanged");
}

/// Live symlinks (pointing at real files) should still short-circuit
/// — they're legitimate user state, not corruption from an
/// interrupted install. Observable: we don't remove the link, and
/// we don't overwrite its target either.
#[test]
#[cfg(unix)]
fn live_symlink_short_circuits() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"new");
    let real_target = write_source(tmp.path(), "existing.txt", b"old");
    let dst = tmp.path().join("dst.txt");
    std::os::unix::fs::symlink(&real_target, &dst).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Hardlink, &src, &dst)
        .expect("live symlink should short-circuit");

    let dst_meta = fs::symlink_metadata(&dst).unwrap();
    eprintln!("dst file_type={:?}", dst_meta.file_type());
    assert!(dst_meta.file_type().is_symlink());
    assert_eq!(fs::read(&real_target).unwrap(), b"old", "target must not be overwritten");
}

/// A one-off `NotFound` / `PermissionDenied` / `AlreadyExists` on
/// a single file must not downgrade the cache — those are
/// per-call errors, not capability errors. A different source /
/// target later in the install would still succeed at the current
/// tier, and we'd have permanently disabled it for no reason.
/// Pin the behaviour for `Auto`; the error propagates verbatim
/// and the cache stays at `CLONE`.
///
/// We use `AlreadyExists` as the trigger (pre-populated target)
/// rather than `NotFound` (missing source) because
/// `reflink_copy::reflink` on non-macOS platforms rewrites a
/// missing-source `NotFound` into `ErrorKind::InvalidInput` for
/// diagnostic purposes (see `reflink-copy/src/lib.rs:64`). That
/// makes `NotFound` a poor test for "call errors propagate" — the
/// error surfaces as `InvalidInput` on Linux / Windows and the
/// test would silently pass via the fallback path instead of the
/// propagation path we want to exercise.
#[test]
fn auto_call_errors_propagate_without_downgrading() {
    let state = AtomicU8::new(LINK_STATE_CLONE);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"fresh");
    let dst = tmp.path().join("dst");
    fs::write(&dst, b"pre-existing").unwrap();

    let err = auto_link::<SilentReporter>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("target exists → AlreadyExists");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        state.load(Ordering::Relaxed),
        LINK_STATE_CLONE,
        "AlreadyExists must not poison the cache",
    );
}

/// Same propagation rule at the hardlink tier. `fs::hard_link`
/// doesn't get the same error-rewriting treatment that reflink
/// does, so we can use the simpler "missing source → `NotFound`"
/// trigger here.
#[test]
fn auto_hardlink_tier_call_errors_propagate() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("does-not-exist");
    let dst = tmp.path().join("dst");

    let err = auto_link::<SilentReporter>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("missing source → NotFound");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(
        state.load(Ordering::Relaxed),
        LINK_STATE_HARDLINK,
        "NotFound at the hardlink tier must not poison the cache",
    );
}

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

    auto_link::<SilentReporter>(&AtomicU8::new(0), &state, &src, &dst)
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

    auto_link::<SilentReporter>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("hardlink should succeed on same-FS tempdir");

    assert_eq!(
        fs::metadata(&src).unwrap().ino(),
        fs::metadata(&dst).unwrap().ino(),
        "state=HARDLINK must hardlink, not copy",
    );
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_HARDLINK, "state must not drift");
}

/// Same propagate-on-call-error property for `CloneOrCopy`. Uses
/// `AlreadyExists` trigger for the same reason
/// `auto_call_errors_propagate_without_downgrading` does —
/// `NotFound` gets rewritten to `InvalidInput` inside reflink-copy
/// on non-macOS and would take the fallback path instead of the
/// propagation path.
#[test]
fn clone_or_copy_call_errors_propagate_without_downgrading() {
    let state = AtomicU8::new(LINK_STATE_CLONE);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"fresh");
    let dst = tmp.path().join("dst");
    fs::write(&dst, b"pre-existing").unwrap();

    let err = clone_or_copy_link::<SilentReporter>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("target exists → AlreadyExists");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        state.load(Ordering::Relaxed),
        LINK_STATE_CLONE,
        "AlreadyExists must not poison the cache",
    );
}

/// `is_cross_device` picks up EXDEV (raw 18) on every Unix we
/// support, but raw 17 is `EEXIST` on Unix and must NOT be
/// classified as cross-device — misclassifying a concurrent-create
/// race as EXDEV would fall back to `fs::copy` and overwrite the
/// other process's file. On Windows raw 17 is
/// `ERROR_NOT_SAME_DEVICE`, which is a genuine cross-device
/// signal, so the detection IS correct there.
#[test]
fn is_cross_device_distinguishes_unix_eexist_from_windows_not_same_device() {
    #[cfg(unix)]
    {
        let exdev = io::Error::from_raw_os_error(18);
        assert!(is_cross_device(&exdev), "raw 18 is EXDEV on every Unix");

        let eexist = io::Error::from_raw_os_error(17);
        assert!(
            !is_cross_device(&eexist),
            "Unix EEXIST (raw 17) is NOT cross-device — misclassifying would overwrite files",
        );
    }

    #[cfg(windows)]
    {
        let not_same_device = io::Error::from_raw_os_error(17);
        assert!(
            is_cross_device(&not_same_device),
            "Windows ERROR_NOT_SAME_DEVICE (raw 17) IS cross-device",
        );

        let not_exdev = io::Error::from_raw_os_error(18);
        assert!(
            !is_cross_device(&not_exdev),
            "raw 18 on Windows is not the cross-device code — must not be classified as EXDEV",
        );
    }
}

/// Pin the deny-list classifier. The state-machine tests above
/// exercise `NotFound` on the common path, but we also care that
/// the capability-style errors we see on real filesystems —
/// notably Windows NTFS's `ERROR_INVALID_FUNCTION` for
/// `FSCTL_DUPLICATE_EXTENTS_TO_FILE`, which Rust maps to
/// `InvalidInput` — fall through to the next tier instead of
/// propagating.
#[test]
fn is_call_error_rejects_capability_codes() {
    // Call-shape errors: must propagate.
    for kind in
        [io::ErrorKind::NotFound, io::ErrorKind::PermissionDenied, io::ErrorKind::AlreadyExists]
    {
        let err = io::Error::from(kind);
        assert!(is_call_error(&err), "kind {kind:?} should be a call error");
    }

    // Capability / cross-device / weird OS codes: must fall
    // through, so they must NOT be classified as call errors.
    for err in [
        io::Error::from(io::ErrorKind::Unsupported),
        io::Error::from(io::ErrorKind::InvalidInput), // Windows ERROR_INVALID_FUNCTION lands here
        io::Error::from_raw_os_error(18),             // EXDEV
        io::Error::from_raw_os_error(25),             // ENOTTY — ext4 reflink rejection
        io::Error::from_raw_os_error(95),             // EOPNOTSUPP
    ] {
        assert!(!is_call_error(&err), "{err:?} should trigger fallback, not propagate");
    }
}

/// Pre-seed `CloneOrCopy` state to `COPY` and verify it uses
/// `fs::copy` — mirrors `auto_respects_cached_copy_state`. Also
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

    clone_or_copy_link::<SilentReporter>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("copy should succeed");

    assert_ne!(
        fs::metadata(&src).unwrap().ino(),
        fs::metadata(&dst).unwrap().ino(),
        "state=COPY must not hardlink",
    );
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY, "state must not drift");
}

/// `log_method_once` emits one `pnpm:package-import-method` event
/// per resolved method per `logged` atomic. Repeated calls with the
/// same flag are suppressed, distinct flags fire independently.
/// Production threads an install-scoped atomic from `Install::run`
/// down to [`link_file`]; this test passes a per-test atomic so it
/// observes the single-emit-per-method contract without racing
/// other tests.
#[test]
fn log_method_once_emits_first_call_per_method_only() {
    use pacquet_reporter::{LogEvent, PackageImportMethod as WireImportMethod, Reporter};
    use std::sync::Mutex;

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    // Reset in case nextest reuses the process for a retry of this test.
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let logged = AtomicU8::new(0);

    super::log_method_once::<RecordingReporter>(
        &logged,
        super::LOG_FLAG_CLONE,
        WireImportMethod::Clone,
    );
    super::log_method_once::<RecordingReporter>(
        &logged,
        super::LOG_FLAG_CLONE,
        WireImportMethod::Clone,
    );
    super::log_method_once::<RecordingReporter>(
        &logged,
        super::LOG_FLAG_HARDLINK,
        WireImportMethod::Hardlink,
    );

    let captured = EVENTS.lock().unwrap();
    let kinds: Vec<WireImportMethod> = captured
        .iter()
        .map(|event| match event {
            LogEvent::PackageImportMethod(log) => log.method,
            other => panic!("unexpected event {other:?}"),
        })
        .collect();
    assert_eq!(kinds, [WireImportMethod::Clone, WireImportMethod::Hardlink]);
}
