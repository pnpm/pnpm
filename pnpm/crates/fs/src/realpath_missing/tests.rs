use super::{realpath_missing, resolve_path_with_symlinks, symlinked_dir_real_path};
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

#[test]
fn symlinked_dir_real_path_resolves_a_dir_outside_the_base_through_any_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(temp.path()).unwrap();
    let real = root.join("real/deep");
    fs::create_dir_all(&real).unwrap();
    crate::symlink_dir(&real, &root.join("alias")).unwrap();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();

    assert_eq!(
        symlinked_dir_real_path(&project, &root.join("alias/vendor")),
        Some(real.join("vendor")),
    );
    assert_eq!(symlinked_dir_real_path(&project, &root.join("real/deep/vendor")), None);
}

#[test]
fn resolve_path_with_symlinks_expands_link_before_parent_dot_dot() {
    let temp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(temp.path()).unwrap();
    let deep = root.join("else/deep");
    fs::create_dir_all(&deep).unwrap();
    crate::symlink_dir(&deep, &root.join("link")).unwrap();

    assert_eq!(
        resolve_path_with_symlinks(&root.join("link/../hosted")).unwrap(),
        root.join("else/hosted"),
    );
    assert!(resolve_path_with_symlinks(std::path::Path::new("")).is_err());
}
