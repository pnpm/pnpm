//! Tests for the [`crate::symlink_dir`] module.
//!
//! Pacquet's symlink writer is meant to be a faithful Rust port of the
//! pnpm [`symlink-dir`](https://github.com/pnpm/symlink-dir) npm
//! package.

#[cfg(windows)]
use super::relative_target_for;
#[cfg(unix)]
use super::symlink_dir;
use super::{ForceSymlinkOutcome, force_symlink_dir, read_symlink_dir};
use std::fs;
#[cfg(windows)]
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
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
        PathBuf::from("..").join("packages").join("pkg-a"),
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
    let target = Path::new(r"C:\Users\runneradmin\setup-pnpm\store\@babel\plugin-x");
    let link = Path::new(r"D:\a\pnpm\pnpm\node_modules\@babel\plugin-x");

    assert_eq!(relative_target_for(target, link), target);
}

#[cfg(windows)]
#[test]
fn windows_same_drive_symlink_target_stays_relative() {
    let target = Path::new(r"C:\workspace\packages\pkg-a");
    let link = Path::new(r"C:\workspace\app\node_modules\pkg-a");

    assert!(relative_target_for(target, link).is_relative());
}

#[cfg(windows)]
#[test]
fn windows_verbatim_and_plain_disk_resolve_to_same_root() {
    let target = Path::new(r"\\?\C:\workspace\packages\pkg-a");
    let link = Path::new(r"C:\workspace\app\node_modules\pkg-a");

    assert!(relative_target_for(target, link).is_relative());
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
