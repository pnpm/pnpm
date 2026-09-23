use crate::StoreDir;
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

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

    assert_eq!(store.prune_private_installs().unwrap(), 1);
    assert!(!private_installs.exists());

    assert_eq!(store.prune_private_installs().unwrap(), 0);
}

#[test]
fn a_label_is_a_single_path_component() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));

    for label in ["", "a/b", "../escape", "/abs"] {
        let error = store.create_private_install(label).unwrap_err();
        assert!(
            matches!(error, crate::PrivateInstallError::InvalidLabel { .. }),
            "{label}: {error}",
        );
    }
}

#[test]
fn prune_removes_litter_and_refuses_a_linked_private_installs_dir() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let private_installs = store.tmp().join("private");
    fs::create_dir_all(&private_installs).unwrap();
    fs::write(private_installs.join("stray-file"), "").unwrap();
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir_all(elsewhere.join("unmarked")).unwrap();
    pnpm_fs::force_symlink_dir(&elsewhere, &private_installs.join("link")).unwrap();

    assert_eq!(store.remove_orphaned_private_installs().unwrap(), 2);
    assert!(!private_installs.exists());
    assert!(elsewhere.join("unmarked").exists(), "the link's target must be untouched");

    fs::create_dir_all(store.tmp()).unwrap();
    pnpm_fs::force_symlink_dir(&elsewhere, &private_installs).unwrap();
    let error = store.remove_orphaned_private_installs().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(elsewhere.join("unmarked").exists());
}

/// A prune that ran between a directory's creation and its marker would
/// take the directory for one left behind.
#[test]
fn creation_waits_for_a_running_prune() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let prune_lock = store.lock_for_prune().unwrap();
    let created = Arc::new(AtomicBool::new(false));
    let creation = thread::spawn({
        let created = Arc::clone(&created);
        move || {
            let private_install = store.create_private_install("node").unwrap();
            created.store(true, Ordering::SeqCst);
            private_install
        }
    });

    thread::sleep(Duration::from_millis(300));
    assert!(!created.load(Ordering::SeqCst), "creation must wait for the prune");

    drop(prune_lock);
    let private_install = creation.join().unwrap();
    assert!(private_install.dir().exists());
}
