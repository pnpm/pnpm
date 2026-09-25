use super::open_components;
use std::{fs, path::Path};

#[test]
fn fallback_opens_components_without_following_intermediate_symlinks() {
    let workspace = tempfile::tempdir().unwrap();
    fs::create_dir_all(workspace.path().join("real/child")).unwrap();
    pnpm_fs::symlink_dir(&workspace.path().join("real"), &workspace.path().join("link")).unwrap();
    let root =
        cap_primitives::fs::open_ambient_dir(workspace.path(), cap_primitives::ambient_authority())
            .unwrap();
    let mut opens = 0;
    let directory = open_components(&root, Path::new("real/child"), &mut opens).unwrap();
    assert_eq!(opens, 2);
    assert!(directory.metadata().unwrap().is_dir());
    let result = open_components(&root, Path::new("link/child"), &mut 0);
    assert!(result.is_err(), "{result:?}");
}
