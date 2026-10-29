use super::{realpath_missing, symlinked_dir_real_path};
use std::fs;

#[test]
fn resolves_symlinks_in_the_existing_ancestor() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    crate::symlink_dir(&real, &root.path().join("link")).unwrap();

    assert_eq!(
        realpath_missing(&root.path().join("link/missing/child")).unwrap(),
        dunce::canonicalize(real).unwrap().join("missing/child"),
    );
}

#[cfg(unix)]
#[test]
fn rejects_a_dangling_symlink() {
    let root = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(root.path().join("missing"), root.path().join("link")).unwrap();

    assert!(realpath_missing(&root.path().join("link/child")).is_err());
}

#[test]
fn symlinked_dir_real_path_resolves_a_symlink_below_the_base() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    crate::symlink_dir(&real, &project.join("node_modules")).unwrap();

    assert_eq!(
        symlinked_dir_real_path(&project, &project.join("node_modules/.pnpm/node_modules")),
        Some(dunce::canonicalize(real).unwrap().join(".pnpm/node_modules")),
    );
}

#[test]
fn symlinked_dir_real_path_skips_real_directories_below_the_base() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let base = root.path().join("link");
    crate::symlink_dir(&real, &base).unwrap();
    fs::create_dir(real.join("node_modules")).unwrap();

    assert_eq!(symlinked_dir_real_path(&base, &base.join("node_modules")), None);
    assert_eq!(symlinked_dir_real_path(&base, &base.join("node_modules/missing")), None);
}
