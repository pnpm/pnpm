use super::{importer_rel_dir, target_relative_to_importer, target_relative_to_lockfile_root};
use pretty_assertions::assert_eq;
use std::path::{Path, PathBuf};

/// An anchor that is absolute on every platform — a bare `/ws/root` is
/// not absolute on Windows (no drive prefix), which the guards reject.
fn abs_root() -> &'static Path {
    if cfg!(windows) { Path::new(r"C:\ws\root") } else { Path::new("/ws/root") }
}

/// Each fast path must agree with the absolute-space math it stands in
/// for, across importer depths and shared prefixes.
#[test]
fn rel_space_matches_absolute_space() {
    let lockfile_dir = abs_root();
    for importer in ["", "packages/app", "packages/group/deep/app"] {
        let project_dir = lockfile_dir.join(importer);
        let rel = importer_rel_dir(&project_dir, lockfile_dir).expect("importer under the root");
        assert_eq!(rel, Path::new(importer));
        for target in ["packages/lib", "packages/group/other", "tools"] {
            let via_rel =
                target_relative_to_importer(Path::new(target), rel).expect("clean target renders");
            let via_abs =
                pathdiff::diff_paths(lockfile_dir.join(target), &project_dir).expect("diff");
            assert_eq!(via_rel, via_abs, "importer {importer:?} target {target:?}");

            // And the inverse direction round-trips back to the
            // lockfile-root-relative form.
            let back = target_relative_to_lockfile_root(&via_rel, rel)
                .expect("internal target stays under the root");
            assert_eq!(back, PathBuf::from(target));
        }
    }
}

#[test]
fn guards_send_unclean_inputs_to_the_fallback() {
    let root = abs_root();
    assert_eq!(importer_rel_dir(&root.parent().unwrap().join("other/app"), root), None);
    assert_eq!(importer_rel_dir(&root.join("a/../b"), root), None);
    assert_eq!(
        importer_rel_dir(&root.join("../root/app"), &root.join("../root")),
        None,
        "a dot-carrying anchor must not pass the clean-absolute guard",
    );

    let rel = Path::new("packages/app");
    assert_eq!(target_relative_to_importer(Path::new("../outside"), rel), None);
    assert_eq!(target_relative_to_importer(&root.join("target"), rel), None);

    assert_eq!(target_relative_to_lockfile_root(&root.join("target"), rel), None);
    // Escapes the lockfile root: `packages/app` + `../../../outside`.
    assert_eq!(target_relative_to_lockfile_root(Path::new("../../../outside"), rel), None);
}

#[test]
fn root_importer_uses_the_empty_suffix() {
    let rel = importer_rel_dir(abs_root(), abs_root()).expect("same dir");
    assert_eq!(rel, Path::new(""));
    assert_eq!(
        target_relative_to_importer(Path::new("packages/lib"), rel),
        Some(PathBuf::from("packages/lib")),
    );
    assert_eq!(
        target_relative_to_lockfile_root(Path::new("packages/lib"), rel),
        Some(PathBuf::from("packages/lib")),
    );
}

/// Windows-only path shapes that are rooted or prefixed without being
/// absolute must not pass the clean-absolute anchor guard.
#[cfg(windows)]
#[test]
fn windows_drive_relative_and_rootless_anchors_use_the_fallback() {
    assert_eq!(importer_rel_dir(Path::new(r"C:ws\root\app"), Path::new(r"C:ws\root")), None);
    assert_eq!(importer_rel_dir(Path::new(r"\ws\root\app"), Path::new(r"\ws\root")), None);
    assert_eq!(target_relative_to_lockfile_root(Path::new(r"\abs\target"), Path::new("app")), None);
    assert_eq!(target_relative_to_lockfile_root(Path::new(r"C:abs"), Path::new("app")), None);
}
