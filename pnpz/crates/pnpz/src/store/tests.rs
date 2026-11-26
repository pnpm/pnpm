use super::{
    Store,
    remove_tree,
};
use pnpz_nix_store::StorePath;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
};
use tempfile::TempDir;

fn hello() -> StorePath {
    StorePath::from_base_name("1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1").unwrap()
}

#[test]
fn commits_a_read_only_tree() {
    let temp = TempDir::new().unwrap();
    let store = Store::at(temp.path().join("nix")).unwrap();
    assert!(!store.is_native());
    assert!(!store.contains(&hello()));

    let staging = store.staging_location_of(&hello());
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("file"), "content").unwrap();
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o555)).unwrap();
    store
        .commit(&staging, &hello())
        .unwrap();

    assert!(store.contains(&hello()));
    assert!(!staging.exists());
    let installed =
        temp.path().join("nix/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1/file");
    assert_eq!(fs::read_to_string(installed).unwrap(), "content");
    remove_tree(&store.location_of(&hello())).unwrap();
}

#[test]
fn keeps_the_path_another_process_installed_first() {
    let temp = TempDir::new().unwrap();
    let store = Store::at(temp.path().join("nix")).unwrap();
    fs::create_dir(store.location_of(&hello())).unwrap();
    fs::write(store.location_of(&hello()).join("file"), "first").unwrap();

    let staging = store.staging_location_of(&hello());
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("file"), "second").unwrap();
    store
        .commit(&staging, &hello())
        .unwrap();

    assert!(!staging.exists());
    assert_eq!(fs::read_to_string(store.location_of(&hello()).join("file")).unwrap(), "first");
}

#[test]
fn removes_a_tree_with_read_only_directories() {
    let temp = TempDir::new().unwrap();
    let tree = temp.path().join("tree");
    fs::create_dir_all(tree.join("nested")).unwrap();
    fs::write(tree.join("nested/file"), "").unwrap();
    fs::set_permissions(tree.join("nested"), fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(&tree, fs::Permissions::from_mode(0o555)).unwrap();

    remove_tree(&tree).unwrap();
    assert!(!tree.exists());
    remove_tree(&tree).unwrap();
}
