use crate::{CAS_MANIFEST_FILENAME, StoreDir, register_project};
use std::fs;

#[test]
fn prune_keeps_loader_blobs_until_the_manifest_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let store = StoreDir::new(root.path().join("store"));
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(store.root()).unwrap();
    register_project(&store, &project).unwrap();
    let hash = "a".repeat(128);
    let blob = store.file_path_by_hex_str(&hash, "");
    fs::create_dir_all(blob.parent().unwrap()).unwrap();
    fs::write(&blob, "module.exports = 42").unwrap();
    let manifest = serde_json::json!({
        "version": 1, "storeDir": store.root(),
        "packages": { "example@1": { "files": { "index.js": hash } } }
    });
    fs::write(project.join(CAS_MANIFEST_FILENAME), manifest.to_string()).unwrap();
    store.prune().unwrap();
    assert!(blob.is_file());
    fs::remove_file(project.join(CAS_MANIFEST_FILENAME)).unwrap();
    store.prune().unwrap();
    assert!(!blob.exists());
}

#[test]
fn invalid_loader_manifest_aborts_prune_before_deleting_files() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let store = StoreDir::new(root.path().join("store"));
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(store.root()).unwrap();
    register_project(&store, &project).unwrap();
    let blob = store.file_path_by_hex_str(&"a".repeat(128), "");
    fs::create_dir_all(blob.parent().unwrap()).unwrap();
    fs::write(&blob, "retained").unwrap();
    for contents in [
        "{".to_string(),
        serde_json::json!({
            "version": 1, "storeDir": store.root(),
            "packages": { "example@1": { "files": { "index.js": "../../outside" } } }
        })
        .to_string(),
    ] {
        fs::write(project.join(CAS_MANIFEST_FILENAME), contents).unwrap();
        assert!(store.prune().is_err());
        assert!(blob.is_file());
    }
}

#[test]
fn loader_blob_paths_reject_absolute_traversal_and_invalid_digests() {
    let files = std::path::Path::new("store/files");
    for hash in ["/tmp/package.json", "../../package.json", "a", "é", &"A".repeat(128)] {
        assert!(super::loader_blob_path(files, hash).is_err(), "{hash}");
    }
    for suffix in ["", "-exec"] {
        let hash = format!("{}{suffix}", "a".repeat(128));
        assert_eq!(
            super::loader_blob_path(files, &hash).unwrap(),
            files.join("aa").join(&hash[2..]),
        );
    }
}
