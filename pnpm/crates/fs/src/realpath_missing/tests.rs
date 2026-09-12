use super::realpath_missing;
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
