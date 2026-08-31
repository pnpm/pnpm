use super::{importer_rel_dir, target_relative_to_importer, target_relative_to_lockfile_root};
use pretty_assertions::assert_eq;
use std::path::{Path, PathBuf};

/// Each fast path must agree with the absolute-space math it stands in
/// for, across importer depths and shared prefixes.
#[test]
fn rel_space_matches_absolute_space() {
    let lockfile_dir = Path::new("/ws/root");
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
    assert_eq!(importer_rel_dir(Path::new("/ws/other/app"), Path::new("/ws/root")), None);
    assert_eq!(importer_rel_dir(Path::new("/ws/root/a/../b"), Path::new("/ws/root")), None);
    assert_eq!(importer_rel_dir(Path::new("/ws/x/../root/app"), Path::new("/ws/x/../root")), None);

    let rel = Path::new("packages/app");
    assert_eq!(target_relative_to_importer(Path::new("../outside"), rel), None);
    assert_eq!(target_relative_to_importer(Path::new("/abs/target"), rel), None);

    assert_eq!(target_relative_to_lockfile_root(Path::new("/abs/target"), rel), None);
    // Escapes the lockfile root: `packages/app` + `../../../outside`.
    assert_eq!(target_relative_to_lockfile_root(Path::new("../../../outside"), rel), None);
}

#[test]
fn root_importer_uses_the_empty_suffix() {
    let rel = importer_rel_dir(Path::new("/ws/root"), Path::new("/ws/root")).expect("same dir");
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
