use pnpm_store_dir::{PackageFilesIndex, StoreIndex};
use std::{path::Path, time::Duration};

pub fn check(directory: &Path, mode: &str) {
    let root = directory.join("store-probe");
    if mode == "roundtrip" {
        check_cas(directory);
    }
    let index = StoreIndex::open(&root).expect("open SQLite store with process lease");
    let key = "sha512-probe@1.0.0";
    if mode == "read" {
        assert_eq!(index.get(key).unwrap().unwrap().algo, "sha512");
    } else {
        let value = PackageFilesIndex { algo: "sha512".into(), ..Default::default() };
        index.set(key, &value).expect("persist store entry");
        assert_eq!(index.get(key).unwrap(), Some(value));
        let reader = StoreIndex::open_readonly(&root).expect("share lease with second connection");
        assert!(reader.contains_key(key).unwrap());
    }
    if mode == "hold" {
        pnpm_fs::file_mode::set_path_permissions(&root.join("index.db"), 0o600).unwrap();
        let connection = rusqlite::Connection::open(root.join("index.db")).unwrap();
        connection.execute_batch("PRAGMA cache_size=1; BEGIN IMMEDIATE; UPDATE package_index SET data=zeroblob(262144);").unwrap();
        println!("store-lease-held");
        std::thread::sleep(Duration::from_secs(30));
        drop(connection);
    }
    drop(index);
    let reopened = StoreIndex::open(&root).expect("reopen after releasing lease");
    assert!(reopened.contains_key(key).unwrap());
    let verification = rusqlite::Connection::open(root.join("index.db")).unwrap();
    let integrity: String = verification
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    println!("Rust SQLite store persistence and connection lease passed");
}

fn check_cas(directory: &Path) {
    check_unreadable_parent(directory);
    check_unreadable_directory_grant(directory);
    let root = directory.join("cas-probe");
    std::fs::create_dir_all(&root).unwrap();
    pnpm_fs::file_mode::set_path_permissions(&root, 0o2770).unwrap();
    let store = pnpm_store_dir::StoreDir::new(&root);
    assert_eq!(store.root(), &root.join("v11-wasm"));
    assert_eq!(pnpm_store_dir::StoreDir::new(root.join("v11")).root(), &root.join("v11/v11-wasm"));
    assert_eq!(pnpm_store_dir::StoreDir::new(root.join("v11-wasm")).root(), store.root());
    let (file, _) = store.write_cas_file(b"executable contents", true).unwrap();
    assert_eq!(pnpm_fs::copy_permissions(&file).unwrap() & 0o770, 0o770);
    assert_eq!(pnpm_fs::copy_permissions(file.parent().unwrap()).unwrap() & 0o2770, 0o2770);
    let installed = directory.join("installed-executable");
    pnpm_fs::copy_file_atomic(&file, &installed).unwrap();
    assert_eq!(
        pnpm_fs::copy_permissions(&installed).unwrap(),
        pnpm_fs::copy_permissions(&file).unwrap()
    );
    std::fs::remove_file(installed).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    println!("Content-addressable store inherits directory and executable file permissions");
}

fn check_unreadable_directory_grant(directory: &Path) {
    let template = directory.join("unreadable-directory-grant");
    std::fs::create_dir(&template).unwrap();
    pnpm_fs::file_mode::set_path_permissions(&template, 0o2770).unwrap();
    let child = template.join("child");
    std::fs::create_dir(&child).unwrap();
    let opened = std::fs::File::open(&child).unwrap();
    pnpm_fs::set_file_permissions(&opened, &0o300).unwrap();
    pnpm_fs::file_mode::grant_inherited_dir_mode(&child, &template)
        .expect("grant inherited mode to a write-and-search-only directory");
    assert_eq!(pnpm_fs::read_file_permissions(&opened).unwrap() & 0o7777, 0o2370);
    pnpm_fs::set_file_permissions(&opened, &0o700).unwrap();
    drop(opened);
    std::fs::remove_dir_all(template).unwrap();
}

fn check_unreadable_parent(directory: &Path) {
    let parent = directory.join("unreadable-parent");
    std::fs::create_dir(&parent).unwrap();
    let parent_file = std::fs::File::open(&parent).unwrap();
    pnpm_fs::set_file_permissions(&parent_file, &0o330).unwrap();
    let created = pnpm_fs::create_file_inheriting_mode(&parent, &parent.join("inherited"), None);
    pnpm_fs::set_file_permissions(&parent_file, &0o700).unwrap();
    let file = created.expect("create under a write-and-search-only parent");
    assert_eq!(pnpm_fs::read_file_permissions(&file).unwrap() & 0o777, 0o620);
    drop(file);
    drop(parent_file);
    std::fs::remove_dir_all(parent).unwrap();
}
