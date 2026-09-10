use super::{
    super::{
        AUTO_FIRST_TIER, Host, LINK_STATE_CLONE, LINK_STATE_HARDLINK, LinkFileError, auto_link,
        clone_or_copy_link, downgrade_auto_tier, is_call_error, link_file, next_auto_tier,
        recover_from_concurrent_import,
    },
    write_source,
};
#[cfg(unix)]
use super::{
    super::{LINK_STATE_COPY, import_into_fresh_target, path_still_names},
    EaccesLinks, EpermLinks,
};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    fs, io,
    sync::atomic::{AtomicU8, Ordering},
};
use tempfile::tempdir;

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
/// A one-off `NotFound` / `AlreadyExists` on
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
        assert_eq!(next_auto_tier(LINK_STATE_CLONE), super::super::LINK_STATE_COPY);
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(AUTO_FIRST_TIER, LINK_STATE_CLONE);
        assert_eq!(next_auto_tier(LINK_STATE_CLONE), LINK_STATE_HARDLINK);
        assert_eq!(next_auto_tier(LINK_STATE_HARDLINK), super::super::LINK_STATE_COPY);
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
    assert_eq!(state.load(Ordering::Relaxed), super::super::LINK_STATE_COPY);
    downgrade_auto_tier(&state, AUTO_FIRST_TIER);
    assert_eq!(state.load(Ordering::Relaxed), super::super::LINK_STATE_COPY);
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

    // Link permissions can be denied while ordinary copying is allowed.
    #[cfg(unix)]
    {
        let eperm = io::Error::from_raw_os_error(libc::EPERM);
        assert!(!is_call_error(&eperm), "EPERM is a capability signal, not a call error");
        let eacces = io::Error::from_raw_os_error(libc::EACCES);
        assert!(!is_call_error(&eacces), "EACCES should allow a copy fallback");
    }
}
#[test]
#[cfg(unix)]
fn eacces_downgrade_still_surfaces_a_failed_copy() {
    let state = AtomicU8::new(AUTO_FIRST_TIER);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"unreachable target");
    let dst = tmp.path().join("missing-parent/dst.txt");

    let err = auto_link::<SilentReporter, EaccesLinks>(&AtomicU8::new(0), &state, &src, &dst)
        .expect_err("copy cannot create a file under a missing directory");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY);
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
/// The failed-copy cleanup unlinks by path, so it has to confirm the
/// path still names what it created. A concurrent `import_atomic`
/// renames a complete file onto the target, and removing that would
/// undo an import that already reported success.
///
/// Unix only: the Windows arm cannot stage this, since deleting a file
/// with an open handle leaves the name in place until the handle closes.
#[test]
#[cfg(unix)]
fn path_still_names_rejects_a_replaced_dirent() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("f");
    let created = fs::File::create(&path).unwrap();

    assert!(path_still_names(&created, &path), "the path names the file this call created");

    fs::remove_file(&path).unwrap();
    fs::write(&path, b"another importer's file").unwrap();

    assert!(!path_still_names(&created, &path), "a replaced dirent is not ours to remove");

    fs::remove_file(&path).unwrap();
    assert!(!path_still_names(&created, &path), "a path that names nothing has nothing to remove");
}
