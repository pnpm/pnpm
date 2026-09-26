use super::{ImporterAnchor, importer_rel_dir};
use pretty_assertions::assert_eq;
use std::path::Path;

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
        let anchor = ImporterAnchor::new(&project_dir, lockfile_dir);
        for target in ["packages/lib", "packages/group/other", "tools"] {
            let via_rel = anchor.target_relative_to_importer(target).expect("clean target renders");
            let via_abs = pathdiff::diff_paths(lockfile_dir.join(target), &project_dir)
                .expect("diff")
                .display()
                .to_string()
                .replace('\\', "/");
            assert_eq!(via_rel, via_abs, "importer {importer:?} target {target:?}");

            // And the inverse direction round-trips back to the
            // lockfile-root-relative form.
            let back = anchor
                .target_relative_to_lockfile_root(&via_rel)
                .expect("internal target stays under the root");
            assert_eq!(back, target);
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

    let anchor = ImporterAnchor::new(&root.join("packages/app"), root);
    let abs_target = root.join("target");
    let abs_target = abs_target.to_str().unwrap();
    assert_eq!(anchor.target_relative_to_importer("../outside"), None);
    assert_eq!(anchor.target_relative_to_importer(abs_target), None);

    assert_eq!(anchor.target_relative_to_lockfile_root(abs_target), None);
    // Escapes the lockfile root: `packages/app` + `../../../outside`.
    assert_eq!(anchor.target_relative_to_lockfile_root("../../../outside"), None);

    // A disarmed anchor renders nothing at all.
    let disarmed = ImporterAnchor::new(&root.parent().unwrap().join("other/app"), root);
    assert_eq!(disarmed.target_relative_to_importer("packages/lib"), None);
    assert_eq!(disarmed.target_relative_to_lockfile_root("packages/lib"), None);
}

/// Targets whose kept tail is not verbatim — collapsed separators, `.`
/// segments, a trailing separator — re-render through the segment join
/// and agree with the clean spelling of the same target.
#[test]
fn filtered_segments_render_like_their_clean_spelling() {
    let anchor = ImporterAnchor::new(&abs_root().join("packages/app"), abs_root());
    for messy in ["packages//lib", "packages/./lib", "packages/lib/", "./packages/lib"] {
        assert_eq!(
            anchor.target_relative_to_importer(messy),
            Some("../lib".to_string()),
            "target {messy:?}",
        );
    }
    assert_eq!(
        anchor.target_relative_to_importer("packages/app"),
        Some(String::new()),
        "a target that is the importer itself renders empty",
    );
    assert_eq!(
        anchor.target_relative_to_importer("packages"),
        Some("..".to_string()),
        "a target that is a prefix of the importer only climbs",
    );
}

#[test]
fn root_importer_uses_the_empty_suffix() {
    let rel = importer_rel_dir(abs_root(), abs_root()).expect("same dir");
    assert_eq!(rel, Path::new(""));
    let anchor = ImporterAnchor::new(abs_root(), abs_root());
    assert_eq!(
        anchor.target_relative_to_importer("packages/lib"),
        Some("packages/lib".to_string()),
    );
    assert_eq!(
        anchor.target_relative_to_lockfile_root("packages/lib"),
        Some("packages/lib".to_string()),
    );
}

/// Windows-only path shapes that are rooted or prefixed without being
/// absolute must not pass the clean-absolute anchor guard.
#[cfg(windows)]
#[test]
fn windows_drive_relative_and_rootless_anchors_use_the_fallback() {
    assert_eq!(importer_rel_dir(Path::new(r"C:ws\root\app"), Path::new(r"C:ws\root")), None);
    assert_eq!(importer_rel_dir(Path::new(r"\ws\root\app"), Path::new(r"\ws\root")), None);
    let anchor = ImporterAnchor::new(Path::new(r"C:\ws\root\app"), Path::new(r"C:\ws\root"));
    assert_eq!(anchor.target_relative_to_lockfile_root(r"\abs\target"), None);
    assert_eq!(anchor.target_relative_to_lockfile_root(r"C:abs"), None);
}
