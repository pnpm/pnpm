//! Tests for the [`crate::symlink_dir`] module.
//!
//! Pacquet's symlink writer is meant to be a faithful Rust port of the
//! pnpm [`symlink-dir`](https://github.com/pnpm/symlink-dir) npm
//! package.

#[cfg(unix)]
use super::symlink_dir;
#[cfg(windows)]
use super::to_native_separators;
use super::{ForceSymlinkOutcome, force_symlink_dir, read_symlink_dir};
#[cfg(windows)]
use super::{is_reparse_point, relative_target_for};
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn unix_symlink_contents_are_relative_to_link_parent() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("packages").join("pkg-a");
    let link = root.path().join("node_modules").join("pkg-a");
    fs::create_dir_all(&target).expect("create target dir");
    fs::create_dir_all(link.parent().unwrap()).expect("create link parent");

    symlink_dir(&target, &link).expect("symlink_dir succeeds");

    let contents = fs::read_link(&link).expect("read_link the symlink we just wrote");
    assert_eq!(
        contents,
        std::path::PathBuf::from("..").join("packages").join("pkg-a"),
        "symlink contents must be the relative path from link parent to target",
    );
    assert!(link.exists(), "symlink must resolve to an existing directory");
}

#[test]
fn force_symlink_dir_returns_reused_when_already_pointing_at_target() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("real");
    let link = root.path().join("link");
    fs::create_dir_all(&target).expect("create target dir");

    let first = force_symlink_dir(&target, &link).expect("first call");
    assert_eq!(
        first,
        ForceSymlinkOutcome { reused: false, warning: None },
        "first call creates the link, not reused",
    );

    let second = force_symlink_dir(&target, &link).expect("second call");
    assert_eq!(
        second,
        ForceSymlinkOutcome { reused: true, warning: None },
        "second call with the same target must be a no-op reuse",
    );
}

#[test]
fn force_symlink_inner_surfaces_concurrent_cleanup_warnings() {
    fn create_then_warn(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        super::symlink_dir(target, link)?;
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            super::ConcurrentCleanupWarning("staged junction cleanup failed".into()),
        ))
    }

    let root = tempdir().expect("create temp dir");
    let target = root.path().join("real");
    let link = root.path().join("link");
    fs::create_dir_all(&target).expect("create target");

    let outcome = super::force_symlink_inner(&target, &link, false, create_then_warn)
        .expect("completed link should be reused");

    assert!(outcome.reused);
    assert_eq!(outcome.warning.as_deref(), Some("staged junction cleanup failed"));
}

#[test]
fn force_symlink_dir_retargets_a_stale_symlink() {
    let root = tempdir().expect("create temp dir");
    let stale_target = root.path().join("old-target");
    let fresh_target = root.path().join("new-target");
    let link = root.path().join("link");
    fs::create_dir_all(&stale_target).expect("create stale target");
    fs::create_dir_all(&fresh_target).expect("create fresh target");

    force_symlink_dir(&stale_target, &link).expect("seed stale link");
    let outcome =
        force_symlink_dir(&fresh_target, &link).expect("force-overwrite to the fresh target");
    assert!(!outcome.reused, "the link pointed at the wrong target, so this should be a rewrite");

    // Use the canonical paths to dodge `/private/tmp` vs `/tmp` aliasing.
    let resolved = fs::canonicalize(&link).expect("canonicalize the new link");
    let want = fs::canonicalize(&fresh_target).expect("canonicalize fresh target");
    assert_eq!(resolved, want, "symlink must now resolve to the fresh target");
}

#[test]
fn force_symlink_dir_moves_non_symlink_occupant_to_ignored_name() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir_all(&target).expect("create target");
    fs::write(&link, b"squatting on the link slot").expect("seed occupant file");

    let outcome = force_symlink_dir(&target, &link).expect("force_symlink_dir succeeds");
    assert!(!outcome.reused, "we wrote a fresh link over an occupant — not a reuse");
    assert!(outcome.warning.is_some(), "warning must be set when an occupant was moved aside");
    let warning = outcome.warning.unwrap();
    assert!(
        warning.contains(".ignored_link"),
        "warning should mention the .ignored_ rename: {warning:?}",
    );

    let resolved_link = fs::canonicalize(&link).expect("canonicalize the new symlink");
    let resolved_target = fs::canonicalize(&target).expect("canonicalize target");
    assert_eq!(resolved_link, resolved_target);
    let ignored_path = root.path().join(".ignored_link");
    assert!(ignored_path.is_file(), "displaced occupant must live at {ignored_path:?}");
}

#[test]
fn force_symlink_dir_creates_missing_parent_directories() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    let link = root.path().join("deeply").join("nested").join("modules").join("link");
    fs::create_dir_all(&target).expect("create target");
    assert!(!link.parent().unwrap().exists(), "parent chain must be missing before the call");

    let outcome =
        force_symlink_dir(&target, &link).expect("force_symlink_dir creates parents and link");
    assert_eq!(outcome, ForceSymlinkOutcome { reused: false, warning: None });
    let resolved_link = fs::canonicalize(&link).expect("canonicalize the new symlink");
    let resolved_target = fs::canonicalize(&target).expect("canonicalize target");
    assert_eq!(resolved_link, resolved_target);
}

/// Regression for the Windows CI failure where the workspace lives
/// on `D:` and the global store (installed by `setup-pnpm`) lives on
/// `C:`: `pathdiff::diff_paths` produced a re-anchored garbage path
/// that Windows rejected with `ERROR_INVALID_PARAMETER` (os error 87).
#[cfg(windows)]
#[test]
fn windows_cross_drive_symlink_target_falls_back_to_absolute() {
    let target = std::path::Path::new(r"C:\Users\runneradmin\setup-pnpm\store\@babel\plugin-x");
    let link = std::path::Path::new(r"D:\a\pnpm\pnpm\node_modules\@babel\plugin-x");

    assert_eq!(relative_target_for(target, link), target);
}

#[cfg(windows)]
#[test]
fn windows_scoped_alias_path_gets_native_separators() {
    let mixed = std::path::Path::new(r"C:\store\v11\links\@\pkg\1.0.0\hash\node_modules")
        .join("@scope/name");
    assert!(
        mixed.as_os_str().to_string_lossy().contains('/'),
        "the join must leave a forward slash for the rewrite to remove: {mixed:?}",
    );

    let native = to_native_separators(&mixed);
    assert!(
        !native.as_os_str().to_string_lossy().contains('/'),
        "no forward slash may survive into the symlink syscall: {native:?}",
    );
    assert_eq!(
        native.as_ref(),
        std::path::Path::new(r"C:\store\v11\links\@\pkg\1.0.0\hash\node_modules\@scope\name",),
    );
}

/// The verbatim `\\?\` form is the case a `Path::components` rewrite
/// would miss, since it treats `/` there as a literal byte.
#[cfg(windows)]
#[test]
fn windows_verbatim_path_forward_slashes_are_rewritten() {
    let verbatim =
        std::path::Path::new(r"\\?\C:\store\v11\links\@\pkg\1.0.0\hash\node_modules\@scope/name");
    let native = to_native_separators(verbatim);
    assert!(
        !native.as_os_str().to_string_lossy().contains('/'),
        "no forward slash may survive into the symlink syscall: {native:?}",
    );
    assert_eq!(
        native.as_ref(),
        std::path::Path::new(r"\\?\C:\store\v11\links\@\pkg\1.0.0\hash\node_modules\@scope\name",),
    );
}

#[cfg(windows)]
#[test]
fn windows_native_path_is_borrowed_unchanged() {
    let native = std::path::Path::new(r"C:\store\v11\links\@\pkg\1.0.0\hash\node_modules\dep");
    assert!(matches!(to_native_separators(native), std::borrow::Cow::Borrowed(_)));
    assert_eq!(to_native_separators(native).as_ref(), native);
}

/// Regression for the Windows failure where a symlink's parent slot is a
/// dangling junction — the shape a tar-based CI cache restore leaves
/// behind (tar can't round-trip a Windows reparse point). The child link
/// can't be created through the dangling junction, and `create_dir_all`
/// can't rebuild the slot because the junction still occupies it (it
/// fails with `AlreadyExists`, os error 183). `force_symlink_dir` must
/// remove the dangling reparse point, rebuild a real directory, and still
/// produce a working link.
#[cfg(windows)]
#[test]
fn windows_force_symlink_dir_repairs_dangling_junction_parent() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("store").join("dep").join("node_modules").join("dep");
    fs::create_dir_all(&target).expect("create target dir");

    // Build a slot `node_modules` that is a dangling junction: point it at
    // a directory, then delete that directory. Windows keeps the reparse
    // point (with the directory attribute) but its target is now missing —
    // the state a cache restore leaves behind.
    let node_modules = root.path().join("store").join("consumer").join("node_modules");
    let junction_target = root.path().join("gone");
    fs::create_dir_all(node_modules.parent().unwrap()).expect("create slot dir");
    fs::create_dir_all(&junction_target).expect("create junction target");
    junction::create(&junction_target, &node_modules).expect("create junction");
    fs::remove_dir_all(&junction_target).expect("delete junction target -> dangling");
    assert!(is_reparse_point(&node_modules), "node_modules must be a dangling reparse point");

    let link = node_modules.join("dep");
    force_symlink_dir(&target, &link).expect("force_symlink_dir must repair the parent and link");

    let resolved_link = fs::canonicalize(&link).expect("canonicalize the repaired symlink");
    let resolved_target = fs::canonicalize(&target).expect("canonicalize target");
    assert_eq!(resolved_link, resolved_target, "the link must resolve to the real target");
}

#[cfg(windows)]
#[test]
fn windows_concurrent_junction_creation_reuses_one_link() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    fs::create_dir_all(&target).expect("create target");

    for iteration in 0..10 {
        let link = root.path().join(format!("link-{iteration}"));
        let barrier = std::sync::Barrier::new(32);
        let outcomes = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..32)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        super::force_symlink_inner(
                            &target,
                            &link,
                            false,
                            super::windows::create_junction,
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("junction worker panicked"))
                .collect::<Vec<_>>()
        });

        let outcomes: Vec<_> = outcomes
            .into_iter()
            .map(|result| result.expect("concurrent junction creation must succeed"))
            .collect();
        assert_eq!(
            outcomes.iter().filter(|outcome| !outcome.reused).count(),
            1,
            "one worker must create the junction and every other worker must reuse it",
        );
    }
}

/// Regression for the rename-failure race: if the atomic rename loses to a
/// concurrent worker (failing with `AlreadyExists`) but that worker's link then
/// disappears before the re-inspection, the surfaced error must not be
/// `AlreadyExists`, or `force_symlink_inner` would treat the now-missing link as
/// reusable. Reproducing the full window deterministically isn't practical, so
/// this drives the commit helper directly.
#[cfg(windows)]
#[test]
fn windows_rename_failure_with_missing_destination_is_not_reusable() {
    let root = tempdir().expect("create temp dir");
    let staging = root.path().join("staging");
    let link = root.path().join("link");
    fs::create_dir(&staging).expect("create staged junction stand-in");

    let rename_error = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    let error = super::windows::discard_staging_after_rename(&staging, &link, rename_error);

    assert_ne!(
        error.kind(),
        std::io::ErrorKind::AlreadyExists,
        "a genuine rename failure with no destination must not look like a reusable link",
    );
    assert!(!staging.exists(), "the staged junction must be cleaned up");
}

#[cfg(windows)]
#[test]
fn windows_same_drive_symlink_target_stays_relative() {
    let target = std::path::Path::new(r"C:\workspace\packages\pkg-a");
    let link = std::path::Path::new(r"C:\workspace\app\node_modules\pkg-a");

    assert!(relative_target_for(target, link).is_relative());
}

#[cfg(windows)]
#[test]
fn windows_verbatim_and_plain_disk_resolve_to_same_root() {
    let target = std::path::Path::new(r"\\?\C:\workspace\packages\pkg-a");
    let link = std::path::Path::new(r"C:\workspace\app\node_modules\pkg-a");

    assert!(relative_target_for(target, link).is_relative());
}

#[cfg(windows)]
#[test]
fn windows_error_directory_falls_back_to_junctions() {
    assert!(super::windows::should_fallback_to_junction(&std::io::Error::from_raw_os_error(267)));
    assert!(!super::windows::should_fallback_to_junction(&std::io::Error::from_raw_os_error(123)));
}

#[test]
fn force_symlink_dir_links_a_scoped_alias() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("store").join("node_modules").join("@scope").join("name");
    let modules = root.path().join("app").join("node_modules");
    let link = modules.join("@scope/name");
    fs::create_dir_all(&target).expect("create target dir");

    let outcome = force_symlink_dir(&target, &link).expect("force_symlink_dir succeeds");
    assert!(!outcome.reused);

    let resolved_link = fs::canonicalize(&link).expect("canonicalize the scoped symlink");
    let resolved_target = fs::canonicalize(&target).expect("canonicalize target");
    assert_eq!(resolved_link, resolved_target);
}

#[test]
fn read_symlink_dir_reads_back_what_force_symlink_dir_wrote() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("real");
    let link = root.path().join("link");
    fs::create_dir_all(&target).expect("create target");

    force_symlink_dir(&target, &link).expect("write link");
    let read = read_symlink_dir(&link).expect("read back the link");
    // On Unix the read-back content matches what `symlink_dir`
    // computed (relative). On Windows true symlinks read back as
    // absolute; junctions get normalized by the `junction` crate.
    // Both must canonicalize to the same target.
    let resolved_read = if read.is_absolute() {
        fs::canonicalize(&read)
    } else {
        fs::canonicalize(link.parent().unwrap().join(&read))
    }
    .expect("canonicalize read-back path");
    assert_eq!(resolved_read, fs::canonicalize(&target).expect("canonicalize target"));
}

/// Same reuse contract when the link lives in a *different* parent
/// directory than the target, so the on-disk link contents contain
/// `..` segments (the virtual-store layout shape:
/// `.pnpm/a@1/node_modules/b` → `.pnpm/b@2/node_modules/b`). The
/// up-to-date check must collapse those segments like Node's
/// `path.relative` does; comparing the joined path verbatim reads
/// every such link as stale.
#[test]
fn force_symlink_dir_reuses_relative_link_across_parents() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("store").join("b@2").join("node_modules").join("b");
    let link = root.path().join("store").join("a@1").join("node_modules").join("b");
    fs::create_dir_all(&target).expect("create target dir");

    let first = force_symlink_dir(&target, &link).expect("first call");
    assert_eq!(first, ForceSymlinkOutcome { reused: false, warning: None });
    #[cfg(unix)]
    {
        let contents = fs::read_link(&link).expect("read link");
        assert!(
            contents.to_string_lossy().contains(".."),
            "link contents should be relative with parent segments: {contents:?}",
        );
    }

    let second = force_symlink_dir(&target, &link).expect("second call");
    assert_eq!(
        second,
        ForceSymlinkOutcome { reused: true, warning: None },
        "an up-to-date relative link must be reused, not rewritten",
    );
}
