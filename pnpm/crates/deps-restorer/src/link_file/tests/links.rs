#[cfg(target_os = "linux")]
use super::super::AUTO_FIRST_TIER;
#[cfg(unix)]
use super::super::{import_into_fresh_target, recover_from_concurrent_import};
use super::{
    super::{Host, LINK_STATE_HARDLINK, LinkFileError, auto_link, link_file},
    write_source,
};
use pnpm_config::PackageImportMethod;
use pnpm_fs::StoreFile;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    fs, io,
    sync::atomic::{AtomicU8, Ordering},
};
use tempfile::tempdir;

/// A symlink squatting at an executable entry's path is left alone, as
/// at a plain one: no writer materialized it, and pnpm's `linkOrCopy`
/// returns on `EEXIST` without touching whatever occupies the path.
#[test]
#[cfg(unix)]
fn eexist_recovery_leaves_a_dangling_symlink_target() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    let dangling_target = tmp.path().join("missing-target");
    let dst = tmp.path().join("dst");
    std::os::unix::fs::symlink(&dangling_target, &dst).unwrap();

    import_into_fresh_target::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &src,
        &dst,
    )
    .expect("a symlink at the target is left for its owner to resolve");

    assert_eq!(
        std::fs::read_link(&dst).unwrap(),
        dangling_target,
        "the symlink survives an EEXIST it cannot own",
    );
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

    for (src, dst) in [(src, dangling("dangling-dst")), (dangling("dangling-src").into(), dst.path)]
    {
        let error =
            recover_from_concurrent_import(io::Error::from(io::ErrorKind::NotFound), &src, &dst)
                .expect_err("a dangling symlink is corruption, not a concurrent writer");
        let LinkFileError::Import { error, .. } = error;
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
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
/// Explicit `Hardlink` must surface non-`EXDEV` link-creation errors
/// instead of silently falling back — matches pnpm's `linkOrCopy`,
/// which only swallows `EXDEV` (and a couple of other kernel-level
/// "not permitted" codes, not modelled here). We drive the error
/// path by pointing at a non-existent source (`NotFound`, which is
/// not `EXDEV`) so the failure is deterministic on every platform.
#[test]
fn explicit_hardlink_surfaces_errors() {
    let tmp = tempdir().unwrap();
    let src = StoreFile::new(tmp.path().join("does-not-exist"));
    let dst = tmp.path().join("dst.txt");

    let err =
        link_file::<SilentReporter>(&AtomicU8::new(0), PackageImportMethod::Hardlink, &src, &dst)
            .expect_err("no source → error");
    assert!(matches!(err, LinkFileError::Import { .. }), "got: {err:?}");
}
/// A dangling symlink left behind by an interrupted install is left
/// alone. Matches pnpm's `linkOrCopy` (`fs/indexed-pkg-importer/src/index.ts`),
/// which returns on `EEXIST` without inspecting the dirent — the
/// downside is that a dangling symlink survives until something
/// rewrites the slot, the upside is a single import syscall per file
/// instead of stat-then-link-then-maybe-unlink. The pre-flight
/// `fs::metadata` short-circuit in [`link_file`](fn@link_file) does not fire for a
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
/// Same propagation rule at the hardlink tier. `fs::hard_link`
/// doesn't get the same error-rewriting treatment that reflink
/// does, so we can use the simpler "missing source → `NotFound`"
/// trigger here.
#[test]
fn auto_hardlink_tier_call_errors_propagate() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = StoreFile::new(tmp.path().join("does-not-exist"));
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
        fs::symlink_metadata(&dst)
            .unwrap()
            .file_type()
            .is_symlink(),
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
/// Content is not the only thing a squatting link can lose. The exec
/// bit is restored through the target after the import adopts it, so an
/// executable store entry must not be able to make a link's referent
/// executable either.
#[test]
#[cfg(unix)]
fn an_exec_source_does_not_make_a_symlinked_referent_executable() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let victim = tmp.path().join("victim");
    fs::write(&victim, b"plain data").unwrap();
    fs::set_permissions(&victim, fs::Permissions::from_mode(0o644)).unwrap();
    let src = write_source(tmp.path(), "1b59d9-exec", b"#!/usr/bin/env node\n");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o755)).unwrap();
    let dst = tmp.path().join("dst");
    std::os::unix::fs::symlink(&victim, &dst).unwrap();

    let _ = import_into_fresh_target::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &src,
        &dst,
    );

    let mode = fs::metadata(&victim)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o644, "the referent must not gain exec bits");
    assert_eq!(fs::read(&victim).unwrap(), b"plain data", "nor lose its contents");
}
