use super::prune_cas;
use crate::{CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex};
use std::{collections::HashMap, fs};

fn package_index(digest: &str) -> PackageFilesIndex {
    PackageFilesIndex {
        algo: "sha512".to_string(),
        files: HashMap::from([(
            "package.json".to_string(),
            CafsFileInfo { digest: digest.to_string(), mode: 0o644, size: 2, checked_at: None },
        )]),
        ..PackageFilesIndex::default()
    }
}

#[test]
fn removes_unlinked_cas_files_and_their_package_rows() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let orphan_digest = format!("01{}", "a".repeat(126));
    let linked_digest = format!("02{}", "b".repeat(126));
    let orphan = store.file_path_by_hex_str(&orphan_digest, "");
    let linked = store.file_path_by_hex_str(&linked_digest, "");
    fs::create_dir_all(orphan.parent().unwrap()).unwrap();
    fs::create_dir_all(linked.parent().unwrap()).unwrap();
    fs::write(&orphan, "{}").unwrap();
    fs::write(&linked, "{}").unwrap();
    let project_copy = root.path().join("project-package.json");
    fs::hard_link(&linked, &project_copy).unwrap();
    fs::create_dir_all(store.tmp()).unwrap();
    fs::write(store.tmp().join("partial"), "partial").unwrap();

    let index = StoreIndex::open_in(&store).unwrap();
    index
        .set("orphan", &package_index(&orphan_digest))
        .unwrap();
    index
        .set("linked", &package_index(&linked_digest))
        .unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 2);
    assert_eq!(stats.packages, 1);
    assert!(!orphan.exists());
    assert!(linked.exists());
    assert!(!store.tmp().exists());
    let index = StoreIndex::open_in(&store).unwrap();
    assert_eq!(index.keys().unwrap(), ["linked"]);
}

#[test]
fn removes_executable_cas_suffix_from_the_index_digest() {
    let root = tempfile::tempdir().unwrap();
    let store = StoreDir::new(root.path().join("store"));
    let digest = format!("03{}", "c".repeat(126));
    let executable = store.file_path_by_hex_str(&digest, "-exec");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "{}").unwrap();
    let index = StoreIndex::open_in(&store).unwrap();
    index
        .set("executable", &package_index(&digest))
        .unwrap();
    drop(index);

    let stats = prune_cas(&store).unwrap();

    assert_eq!(stats.packages, 1);
    assert!(
        StoreIndex::open_in(&store)
            .unwrap()
            .keys()
            .unwrap()
            .is_empty(),
    );
}
