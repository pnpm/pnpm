use crate::StoreDir;
use std::fs;

#[test]
fn a_private_install_lives_under_the_store_tmp_until_dropped() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));

    let private_install = store.create_private_install("pnpm-9.3.0").unwrap();

    let dir = private_install.dir().to_path_buf();
    assert!(dir.starts_with(store.tmp().join("private")), "{}", dir.display());
    assert!(
        dir.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("pnpm-9.3.0-"),
    );
    fs::write(dir.join("installed"), "").unwrap();

    drop(private_install);

    assert!(!dir.exists());
}

#[test]
fn prune_removes_the_private_installs_no_process_holds() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let held = store.create_private_install("held").unwrap();
    let unmarked = store
        .tmp()
        .join("private")
        .join("unmarked");
    fs::create_dir_all(&unmarked).unwrap();
    // The marker of a process that has ended: the file stays, its lock went
    // with the process.
    let released = store
        .tmp()
        .join("private")
        .join("released");
    fs::create_dir_all(&released).unwrap();
    fs::write(released.join("in-use"), "").unwrap();

    let removed = store.remove_orphaned_private_installs().unwrap();

    assert_eq!(removed, 2);
    assert!(held.dir().exists());
    assert!(!unmarked.exists());
    assert!(!released.exists());
}

#[test]
fn prune_removes_the_private_installs_dir_once_it_is_empty() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let private_installs = store.tmp().join("private");
    fs::create_dir_all(private_installs.join("abandoned")).unwrap();

    assert_eq!(store.remove_orphaned_private_installs().unwrap(), 1);
    assert!(!private_installs.exists());

    assert_eq!(store.remove_orphaned_private_installs().unwrap(), 0);
}
