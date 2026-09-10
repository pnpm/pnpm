use super::{
    AUTO_FIRST_TIER, FsHardLink, FsReflink, Host, LINK_STATE_CLONE, LINK_STATE_HARDLINK,
    LinkFileError, auto_link, clone_or_copy_link, downgrade_auto_tier, is_call_error, link_file,
    next_auto_tier, recover_from_concurrent_import, try_import,
};
#[cfg(unix)]
use super::{LINK_STATE_COPY, import_into_fresh_target, is_operation_not_permitted};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
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

/// A CAS entry stored as executable carries the `-exec` suffix in its
/// store path. Copying it out must land an executable file even when
/// the copy tier dropped the exec bit (overlayfs etc.) — the suffix is
/// the source of truth, so the copied binary ends up `0o755`.
#[test]
#[cfg(unix)]
fn copy_restores_executable_mode_from_cas_suffix() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o644)).unwrap();
    let dst = tmp.path().join("nested/dst");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Copy, &src, &dst)
        .expect("copy should restore executable CAS mode");

    let dst_mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
    assert_eq!(dst_mode, 0o755, "copied executable file must stay executable");
}

/// A non-executable CAS entry has no `-exec` suffix, so the copy must
/// leave its mode untouched. Guards against widening permissions on the
/// restrictive end — a `0o600` source stays `0o600`, never `0o711`.
#[test]
#[cfg(unix)]
fn copy_does_not_widen_non_exec_mode() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9", b"private data\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o600)).unwrap();
    let dst = tmp.path().join("nested/dst");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Copy, &src, &dst)
        .expect("copy should succeed");

    let dst_mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
    assert_eq!(dst_mode, 0o600, "non-executable file must not gain exec bits");
}

/// On EEXIST the import adopts the racing writer's dirent, but re-asserts
/// the exec bit from the `-exec` suffix — so a target a prior failed
/// restore left at `0o644` is healed rather than adopted broken. Driven
/// through `Hardlink` for a deterministic EEXIST without needing reflink
/// (copy-on-write) support on the test filesystem.
#[test]
#[cfg(unix)]
fn eexist_restores_executable_mode_from_cas_suffix() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
    let dst = write_source(tmp.path(), "dst", b"#!/usr/bin/env node\n");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o644)).unwrap();

    import_into_fresh_target::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &src,
        &dst,
    )
    .expect("EEXIST import should heal the exec bit");

    let dst_mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
    assert_eq!(dst_mode, 0o755, "stale 0o644 target must be restored to 0o755 on EEXIST");
}

/// The EEXIST exec-bit re-assertion must not widen a non-executable
/// entry: a `-exec`-less source leaves an existing `0o600` target alone.
#[test]
#[cfg(unix)]
fn eexist_does_not_widen_non_exec_mode() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9", b"private data\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o644)).unwrap();
    let dst = write_source(tmp.path(), "dst", b"private data\n");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o600)).unwrap();

    import_into_fresh_target::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &src,
        &dst,
    )
    .expect("EEXIST import should be a no-op for a non-exec entry");

    let dst_mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
    assert_eq!(dst_mode, 0o600, "non-exec EEXIST target must not gain exec bits");
}

/// A dangling symlink squatting at an executable entry's path must
/// keep failing the import: the syscall reports EEXIST for the dirent,
/// the exec-bit re-assertion opens through the symlink and gets
/// `NotFound`, and no concurrent writer will ever heal it — unlike a
/// target that truly vanished, whose remover writes an equivalent file.
#[test]
#[cfg(unix)]
fn eexist_recovery_rejects_a_dangling_symlink_target() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
    let dst = tmp.path().join("dst");
    std::os::unix::fs::symlink(tmp.path().join("missing-target"), &dst).unwrap();

    import_into_fresh_target::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &src,
        &dst,
    )
    .expect_err("a dangling symlink at the target is corruption, not a concurrent writer");
}

/// The writer that owns a shared slot may replace the target again
/// before the exec-bit re-assertion opens it; it restores the bit itself,
/// so an EEXIST whose dirent is gone by then is finished work, not a loss.
#[test]
#[cfg(unix)]
fn eexist_recovery_tolerates_a_target_its_writer_replaced() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    let dst = tmp.path().join("dst");

    recover_from_concurrent_import(io::Error::from(io::ErrorKind::AlreadyExists), &src, &dst)
        .expect("a target unlinked after EEXIST belongs to a writer that finishes it");
}

/// APFS `clonefile` can report a destination another importer renamed
/// into place as `NotFound` (pnpm/pnpm#14560). With the dirent and the
/// source both present that is the concurrent-writer case, and the
/// adopted target gets its exec bit re-asserted like an EEXIST one.
#[test]
#[cfg(unix)]
fn spurious_not_found_with_an_existing_target_is_adopted() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
    let dst = write_source(tmp.path(), "dst", b"#!/usr/bin/env node\n");
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o644)).unwrap();

    recover_from_concurrent_import(io::Error::from(io::ErrorKind::NotFound), &src, &dst)
        .expect("a NotFound against an existing target is a concurrent import");

    let dst_mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
    assert_eq!(dst_mode, 0o755, "the adopted target must be restored to 0o755");
}

#[test]
fn not_found_without_a_target_propagates() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9", b"data\n");
    let dst = tmp.path().join("dst");

    let error =
        recover_from_concurrent_import(io::Error::from(io::ErrorKind::NotFound), &src, &dst)
            .expect_err("a NotFound with no target dirent is a real failure");
    let LinkFileError::Import { from, to, error } = error;
    assert_eq!((from, to), (src, dst));
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn not_found_without_a_source_propagates() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("missing-blob");
    let dst = write_source(tmp.path(), "dst", b"data\n");

    let error =
        recover_from_concurrent_import(io::Error::from(io::ErrorKind::NotFound), &src, &dst)
            .expect_err("a NotFound for a missing source is a real failure");
    let LinkFileError::Import { error, .. } = error;
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

/// A dangling symlink at either path also opens as `NotFound` for the
/// copy tier, and no concurrent importer will ever heal it.
#[test]
#[cfg(unix)]
fn not_found_with_a_dangling_symlink_at_either_path_propagates() {
    let tmp = tempdir().unwrap();
    let dangling = |name: &str| {
        let link = tmp.path().join(name);
        std::os::unix::fs::symlink(tmp.path().join("missing-target"), &link).unwrap();
        link
    };
    let src = write_source(tmp.path(), "1b59d9", b"data\n");
    let dst = write_source(tmp.path(), "dst", b"data\n");

    for (src, dst) in [(src, dangling("dangling-dst")), (dangling("dangling-src"), dst)] {
        let error =
            recover_from_concurrent_import(io::Error::from(io::ErrorKind::NotFound), &src, &dst)
                .expect_err("a dangling symlink is corruption, not a concurrent writer");
        let LinkFileError::Import { error, .. } = error;
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}

#[test]
fn other_import_errors_propagate() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9", b"data\n");
    let dst = write_source(tmp.path(), "dst", b"data\n");

    let error = recover_from_concurrent_import(
        io::Error::from(io::ErrorKind::PermissionDenied),
        &src,
        &dst,
    )
    .expect_err("PermissionDenied is not a concurrent import");
    let LinkFileError::Import { error, .. } = error;
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

/// Hardlinking in the same directory on the same filesystem works on
/// every mainstream OS the project supports.
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
/// directly — the [`existing_target_is_preserved`] loop
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

    let err = auto_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
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

    let err = auto_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
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

/// The platform ladder itself: hardlink before clone on Linux, clone
/// first everywhere else (`next_auto_tier` carries the why). Pinned so
/// a refactor of the downgrade machinery can't quietly put Linux back
/// on the reflink tier.
#[test]
fn auto_ladder_order_is_platform_specific() {
    #[cfg(target_os = "linux")]
    {
        assert_eq!(AUTO_FIRST_TIER, LINK_STATE_HARDLINK);
        assert_eq!(next_auto_tier(LINK_STATE_HARDLINK), LINK_STATE_CLONE);
        assert_eq!(next_auto_tier(LINK_STATE_CLONE), super::LINK_STATE_COPY);
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(AUTO_FIRST_TIER, LINK_STATE_CLONE);
        assert_eq!(next_auto_tier(LINK_STATE_CLONE), LINK_STATE_HARDLINK);
        assert_eq!(next_auto_tier(LINK_STATE_HARDLINK), super::LINK_STATE_COPY);
    }
}

/// The downgrade cache only steps forward from the exact tier that
/// failed: a worker holding a stale view of the ladder must neither
/// skip it ahead nor drag it back once another worker has advanced it.
#[test]
fn stale_downgrades_neither_skip_nor_regress() {
    let state = AtomicU8::new(AUTO_FIRST_TIER);
    let second = next_auto_tier(AUTO_FIRST_TIER);

    // The first failure advances the ladder head to its successor.
    downgrade_auto_tier(&state, AUTO_FIRST_TIER);
    assert_eq!(state.load(Ordering::Relaxed), second);

    // A racing worker that still saw the head reports the same failure:
    // its exchange must lose rather than skip the ladder toward copy.
    downgrade_auto_tier(&state, AUTO_FIRST_TIER);
    assert_eq!(state.load(Ordering::Relaxed), second);

    // Only the current tier failing moves the ladder again — and a
    // last stale report from the head still cannot move it back.
    downgrade_auto_tier(&state, second);
    assert_eq!(state.load(Ordering::Relaxed), super::LINK_STATE_COPY);
    downgrade_auto_tier(&state, AUTO_FIRST_TIER);
    assert_eq!(state.load(Ordering::Relaxed), super::LINK_STATE_COPY);
}

/// A fresh `Auto` on Linux hardlinks: same-filesystem tempdir, no
/// prior downgrades, and the observable is the shared inode — the
/// exact on-disk shape an install's first file gets.
#[test]
#[cfg(target_os = "linux")]
fn auto_fresh_state_hardlinks_on_linux() {
    use std::os::unix::fs::MetadataExt;

    let state = AtomicU8::new(AUTO_FIRST_TIER);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"first-tier");
    let dst = tmp.path().join("dst.txt");

    auto_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("hardlink should succeed on same-FS tempdir");

    assert_eq!(
        fs::metadata(&src).unwrap().ino(),
        fs::metadata(&dst).unwrap().ino(),
        "a fresh Auto on Linux must land on the hardlink tier",
    );
    assert_eq!(state.load(Ordering::Relaxed), AUTO_FIRST_TIER, "success must not downgrade");
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

/// Same propagate-on-call-error property for `CloneOrCopy`. Uses
/// `AlreadyExists` trigger for the same reason
/// [`auto_call_errors_propagate_without_downgrading`] does —
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

    let err = clone_or_copy_link::<SilentReporter, Host>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("target exists → AlreadyExists");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        state.load(Ordering::Relaxed),
        LINK_STATE_CLONE,
        "AlreadyExists must not poison the cache",
    );
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

    // The two errnos behind `PermissionDenied` split: EPERM is the
    // filesystem refusing the operation (fall through), EACCES is the
    // caller lacking access to the paths (propagate).
    #[cfg(unix)]
    {
        let eperm = io::Error::from_raw_os_error(libc::EPERM);
        assert!(!is_call_error(&eperm), "EPERM is a capability signal, not a call error");
        let eacces = io::Error::from_raw_os_error(libc::EACCES);
        assert!(is_call_error(&eacces), "EACCES is a call error");
    }
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

/// `log_method_once` emits one `pnpm:package-import-method` event
/// per resolved method per `logged` atomic. Repeated calls with the
/// same flag are suppressed, distinct flags fire independently.
/// Production threads an install-scoped atomic from `Install::run`
/// down to [`link_file`]; this test passes a per-test atomic so it
/// observes the single-emit-per-method contract without racing
/// other tests.
#[test]
fn log_method_once_emits_first_call_per_method_only() {
    use pnpm_reporter::{LogEvent, PackageImportMethod as WireImportMethod, Reporter};
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

/// A filesystem that refuses `link(2)` with `EPERM`: what a FUSE
/// filesystem without hardlinks (`EdenFS`) answers for every link inside
/// its mount. pnpm's store-to-checkout links usually fail `EXDEV` first
/// and retire the tier before the ladder reaches such a link, so this
/// is met when source and target both sit inside the mount, as the
/// `file:` packages a repeat install re-imports do. Reflink and copy
/// behave as the real filesystem does.
#[cfg(unix)]
struct EpermHardLink;

#[cfg(unix)]
impl FsHardLink for EpermHardLink {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

#[cfg(unix)]
impl FsReflink for EpermHardLink {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        Host::reflink(source, target)
    }
}

/// The user-namespace containers of pnpm/pnpm#14722, where `FICLONE`
/// is refused with `EPERM`. Hardlinks work there.
#[cfg(unix)]
struct EpermReflink;

#[cfg(unix)]
impl FsHardLink for EpermReflink {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()> {
        Host::hard_link(source, target)
    }
}

#[cfg(unix)]
impl FsReflink for EpermReflink {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

/// A filesystem that refuses both links with `EPERM`, so the `Auto`
/// ladder has only the copy tier left.
#[cfg(unix)]
struct EpermLinks;

#[cfg(unix)]
impl FsHardLink for EpermLinks {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

#[cfg(unix)]
impl FsReflink for EpermLinks {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

/// `EACCES`: the caller was denied an access the link needed. Whether
/// a copy would get past that is unknown, so the ladder surfaces it
/// rather than guessing; it stays a call error.
#[cfg(unix)]
struct EaccesHardLink;

#[cfg(unix)]
impl FsHardLink for EaccesHardLink {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EACCES))
    }
}

#[cfg(unix)]
impl FsReflink for EaccesHardLink {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        Host::reflink(source, target)
    }
}

#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().ino()
}

/// `EPERM` from the hardlink tier retires the tier and the file still
/// lands, materialized by a lower tier rather than shared with the
/// source. This is the repeat install that re-imports a `file:` package
/// inside an `EdenFS` checkout.
#[test]
#[cfg(unix)]
fn eperm_from_hard_link_downgrades_the_auto_ladder() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"no hardlinks here");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    auto_link::<SilentReporter, EpermHardLink>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("EPERM on the hardlink tier falls through to a tier that works");

    assert_eq!(fs::read(&dst).unwrap(), b"no hardlinks here");
    assert_ne!(inode(&src), inode(&dst), "the file was not hardlinked");
    assert_ne!(
        state.load(Ordering::Relaxed),
        LINK_STATE_HARDLINK,
        "the hardlink tier is retired for the rest of the process",
    );
}

/// `EPERM` from the reflink tier is the same capability signal: the
/// ladder moves on and the file lands.
#[test]
#[cfg(unix)]
fn eperm_from_reflink_downgrades_the_clone_tier() {
    let state = AtomicU8::new(LINK_STATE_CLONE);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"no clones here");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    auto_link::<SilentReporter, EpermReflink>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("EPERM on the clone tier falls through to a tier that works");

    assert_eq!(fs::read(&dst).unwrap(), b"no clones here");
    assert_ne!(state.load(Ordering::Relaxed), LINK_STATE_CLONE, "the clone tier is retired");
}

/// `CloneOrCopy` has only the copy tier to fall to, and it must get
/// there on `EPERM` too: this is the `pnpm deploy` project-file import
/// of pnpm/pnpm#14722.
#[test]
#[cfg(unix)]
fn eperm_from_reflink_downgrades_clone_or_copy_to_copy() {
    let state = AtomicU8::new(LINK_STATE_CLONE);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"copied instead");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    clone_or_copy_link::<SilentReporter, EpermReflink>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("EPERM on the clone tier falls through to copy");

    assert_eq!(fs::read(&dst).unwrap(), b"copied instead");
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY);
}

/// `EACCES` shares `ErrorKind::PermissionDenied` with `EPERM` and must
/// keep propagating: the tier stays, the error surfaces, nothing is
/// written.
#[test]
#[cfg(unix)]
fn eacces_from_hard_link_propagates_without_downgrading() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"forbidden");
    let dst = tmp.path().join("nested/dst.txt");
    fs::create_dir_all(dst.parent().unwrap()).unwrap();

    let err = auto_link::<SilentReporter, EaccesHardLink>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("EACCES is a call error");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(err.raw_os_error(), Some(libc::EACCES));
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_HARDLINK, "the tier is not retired");
    assert!(!dst.exists(), "nothing was written");
}

/// The explicit `hardlink` method has no ladder to downgrade, and it
/// must not answer `EPERM` with a copy: a filesystem that refuses links
/// would copy every package, which is what choosing `hardlink` rules
/// out. The error surfaces so the user can pick another method.
#[test]
#[cfg(unix)]
fn explicit_hardlink_propagates_eperm() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"explicit");
    let dst = tmp.path().join("dst.txt");

    let err = try_import::<SilentReporter, EpermHardLink>(
        PackageImportMethod::Hardlink,
        &AtomicU8::new(0),
        &src,
        &dst,
    )
    .expect_err("EPERM on an explicit hardlink is not hidden behind a copy");

    assert_eq!(err.raw_os_error(), Some(libc::EPERM));
    assert!(!dst.exists(), "nothing was written");
}

/// Only the raw errno tells `EPERM` from `EACCES`; an error built from
/// the kind alone carries no errno and stays a call error.
#[test]
#[cfg(unix)]
fn is_operation_not_permitted_is_eperm_only() {
    assert!(is_operation_not_permitted(&io::Error::from_raw_os_error(libc::EPERM)));
    assert!(!is_operation_not_permitted(&io::Error::from_raw_os_error(libc::EACCES)));
    assert!(!is_operation_not_permitted(&io::Error::from(io::ErrorKind::PermissionDenied)));
}

/// Downgrading on `EPERM` does not swallow what the copy tier then
/// finds wrong with the paths: with both links refused and no parent
/// directory for the target, the ladder reaches the copy tier, the copy
/// fails, and that error is the one the caller sees.
#[test]
#[cfg(unix)]
fn eperm_downgrade_still_surfaces_a_failed_copy() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"unreachable target");
    let dst = tmp.path().join("missing-parent/dst.txt");

    let err = auto_link::<SilentReporter, EpermLinks>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("the copy tier cannot create a file under a missing directory");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY, "both link tiers were retired");
    assert!(!dst.exists());
}

/// A source that has run out of names: `EMLINK` on Unix,
/// `ERROR_TOO_MANY_LINKS` on Windows, one [`io::ErrorKind`] either way.
/// A store file linked into enough projects reaches NTFS's cap of 1024
/// names long before ext4's 65000.
struct OutOfLinks;

impl FsHardLink for OutOfLinks {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::TooManyLinks))
    }
}

impl FsReflink for OutOfLinks {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        unreachable!("the hardlink tier materializes the file itself, so no tier follows it")
    }
}

/// The link limit belongs to one file, so the `Auto` ladder copies that
/// file and keeps hardlinking everything after it. Retiring the tier
/// would make one popular store entry downgrade the whole install.
#[test]
fn too_many_links_copies_one_file_and_keeps_the_hardlink_tier() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"out of names");
    let dst = tmp.path().join("dst.txt");

    auto_link::<SilentReporter, OutOfLinks>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("a source out of names is copied, not failed");

    assert_eq!(fs::read(&dst).unwrap(), b"out of names");
    assert_eq!(
        state.load(Ordering::Relaxed),
        LINK_STATE_HARDLINK,
        "the tier stays: the next file has names left",
    );
    fs::write(&src, b"rewritten").unwrap();
    assert_eq!(fs::read(&dst).unwrap(), b"out of names", "the copy is independent of the source");
}

/// The explicit `hardlink` method copies here for the same reason it
/// copies on `EXDEV`: it costs one file rather than the install, so it
/// is not the silent whole-install copy that `EPERM` would be.
#[test]
fn explicit_hardlink_copies_on_too_many_links() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"explicit");
    let dst = tmp.path().join("dst.txt");

    try_import::<SilentReporter, OutOfLinks>(
        PackageImportMethod::Hardlink,
        &AtomicU8::new(0),
        &src,
        &dst,
    )
    .expect("a source out of names is copied, not failed");

    assert_eq!(fs::read(&dst).unwrap(), b"explicit");
    fs::write(&src, b"rewritten").unwrap();
    assert_eq!(fs::read(&dst).unwrap(), b"explicit", "the copy is independent of the source");
}

/// The fresh-target import skips the stat short-circuit, so a symlink
/// squatting at the target reaches the import call itself. `fs::copy`
/// would open it and write store content into whatever it names; the
/// exclusive create reports the occupied path instead and the file the
/// link points at is left alone.
#[test]
#[cfg(unix)]
fn copy_does_not_write_through_a_symlink_at_the_target() {
    let tmp = tempdir().unwrap();
    let victim = tmp.path().join("victim");
    fs::write(&victim, b"do not touch").unwrap();
    let src = write_source(tmp.path(), "1b59d9", b"store content\n");
    let dst = tmp.path().join("dst");
    std::os::unix::fs::symlink(&victim, &dst).unwrap();

    let _ = import_into_fresh_target::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &src,
        &dst,
    );

    assert_eq!(fs::read(&victim).unwrap(), b"do not touch", "the referent keeps its contents");
    assert!(
        fs::symlink_metadata(&dst).unwrap().file_type().is_symlink(),
        "the squatter is reported, not silently written through",
    );
}

/// A symlink whose referent does not exist yet passes the stat
/// short-circuit, since that stat follows the link and fails. `fs::copy`
/// would then *create* the referent and fill it with store content,
/// putting a file wherever the link points. Exclusive creation cannot:
/// `O_EXCL` fails on the link itself.
#[test]
#[cfg(unix)]
fn copy_does_not_create_the_referent_of_a_dangling_symlink() {
    let tmp = tempdir().unwrap();
    let referent = tmp.path().join("not-yet-here");
    let src = write_source(tmp.path(), "1b59d9", b"store content\n");
    let dst = tmp.path().join("dst");
    std::os::unix::fs::symlink(&referent, &dst).unwrap();

    let _ = link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Copy, &src, &dst);

    assert!(!referent.exists(), "the import must not create a file the symlink names");
}

/// A copy that dies partway must not leave its half-written target
/// behind. The next import reads an occupied target as a concurrent
/// writer's finished work and adopts it, so a truncated file left here
/// would be adopted as the package's content. Reading a directory as a
/// file fails after the target is created, which is the shape of a
/// device error or a disk filling up.
#[test]
#[cfg(unix)]
fn a_failed_copy_removes_its_partial_target() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("a-directory");
    fs::create_dir(&src).unwrap();
    let dst = tmp.path().join("dst");

    link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Copy, &src, &dst)
        .expect_err("a directory cannot be read as a file");

    assert!(!dst.exists(), "the partial target must not survive the failure");
}
