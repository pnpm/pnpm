use super::unlink_children;
use std::fs;

#[test]
fn unlinks_a_scoped_link_to_the_skipped_package() {
    let root = tempfile::tempdir().expect("create temp dir");
    let target = root.path().join("slot/node_modules/@scope/pkg");
    fs::create_dir_all(&target).expect("create target");
    let modules = root.path().join("node_modules");
    fs::create_dir_all(modules.join("@scope")).expect("create scope");
    let link = modules.join("@scope/pkg");
    pnpm_fs::symlink_dir(&target, &link).expect("link package");

    unlink_children(&modules, &[target]);

    assert!(fs::symlink_metadata(&link).is_err(), "the scoped link must be removed");
}

#[test]
fn does_not_follow_a_linked_scope_directory() {
    let root = tempfile::tempdir().expect("create temp dir");
    let target = root.path().join("slot/node_modules/@scope/pkg");
    fs::create_dir_all(&target).expect("create target");
    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).expect("create outside dir");
    let outside_link = outside.join("pkg");
    pnpm_fs::symlink_dir(&target, &outside_link).expect("link package outside");
    let modules = root.path().join("node_modules");
    fs::create_dir_all(&modules).expect("create modules dir");
    pnpm_fs::symlink_dir(&outside, &modules.join("@scope")).expect("link scope");

    unlink_children(&modules, &[target]);

    assert!(
        fs::symlink_metadata(&outside_link).is_ok(),
        "a link reached through a linked scope directory must stay",
    );
}
