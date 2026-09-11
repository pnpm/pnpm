#[cfg(target_os = "linux")]
use super::super::AUTO_FIRST_TIER;
use super::{
    super::{Host, LINK_STATE_HARDLINK, LinkFileError, auto_link, link_file, try_import},
    OutOfLinks, write_source,
};
#[cfg(unix)]
use super::{
    super::{
        LINK_STATE_CLONE, LINK_STATE_COPY, clone_or_copy_link, import_into_fresh_target,
        is_link_permission_error, recover_from_concurrent_import,
    },
    EaccesHardLink, EaccesLinks, EpermHardLink, EpermReflink, inode,
};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    fs, io,
    sync::atomic::{AtomicU8, Ordering},
};
use tempfile::tempdir;

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
    let src = tmp.path().join("does-not-exist");
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
#[test]
#[cfg(unix)]
fn eacces_from_hard_link_downgrades_the_auto_ladder() {
    let state = AtomicU8::new(LINK_STATE_HARDLINK);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"no hardlinks here");
    let dst = tmp.path().join("dst.txt");

    auto_link::<SilentReporter, EaccesHardLink>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("EACCES on the hardlink tier falls through");

    assert_eq!(fs::read(&dst).unwrap(), b"no hardlinks here");
    assert_ne!(inode(&src), inode(&dst), "the file was not hardlinked");
    assert_ne!(state.load(Ordering::Relaxed), LINK_STATE_HARDLINK, "hardlinks are retired");
}
#[test]
#[cfg(unix)]
fn eacces_from_both_link_tiers_copies_and_restores_exec_bits() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "source-exec", b"executable");
    fs::set_permissions(&src, fs::Permissions::from_mode(0o644)).unwrap();

    for first_tier in [LINK_STATE_CLONE, LINK_STATE_HARDLINK] {
        let state = AtomicU8::new(first_tier);
        let logged = AtomicU8::new(0);
        let dst = tmp.path().join(format!("target-{first_tier}"));
        auto_link::<SilentReporter, EaccesLinks>(&logged, &state, &src, &dst)
            .expect("copy works when link permissions are denied");

        assert_eq!(fs::read(&dst).unwrap(), b"executable");
        assert_ne!(inode(&src), inode(&dst), "copy has its own inode");
        assert_eq!(fs::metadata(&dst).unwrap().permissions().mode() & 0o111, 0o111);
        assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY);
        assert_eq!(logged.load(Ordering::Relaxed), super::super::LOG_FLAG_COPY);
    }
}
#[test]
#[cfg(unix)]
fn eacces_from_reflink_downgrades_clone_or_copy() {
    let state = AtomicU8::new(LINK_STATE_CLONE);
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"copied");
    let dst = tmp.path().join("dst.txt");

    clone_or_copy_link::<SilentReporter, EaccesLinks>(&AtomicU8::new(0), &state, &src, &dst)
        .expect("copy works when reflink permissions are denied");

    assert_eq!(fs::read(&dst).unwrap(), b"copied");
    assert_eq!(state.load(Ordering::Relaxed), LINK_STATE_COPY);
}
#[test]
#[cfg(unix)]
fn explicit_link_methods_propagate_eacces() {
    let tmp = tempdir().unwrap();
    let src = write_source(tmp.path(), "src.txt", b"explicit");
    let dst = tmp.path().join("dst.txt");

    for method in [PackageImportMethod::Hardlink, PackageImportMethod::Clone] {
        let err = try_import::<SilentReporter, EaccesLinks>(method, &AtomicU8::new(0), &src, &dst)
            .expect_err("explicit link methods must surface EACCES");
        assert_eq!(err.raw_os_error(), Some(libc::EACCES));
    }
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
#[test]
#[cfg(unix)]
fn is_link_permission_error_accepts_only_eperm_and_eacces() {
    assert!(is_link_permission_error(&io::Error::from_raw_os_error(libc::EPERM)));
    assert!(is_link_permission_error(&io::Error::from_raw_os_error(libc::EACCES)));
    assert!(!is_link_permission_error(&io::Error::from(io::ErrorKind::PermissionDenied)));
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

    let mode = fs::metadata(&victim).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "the referent must not gain exec bits");
    assert_eq!(fs::read(&victim).unwrap(), b"plain data", "nor lose its contents");
}
