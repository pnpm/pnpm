use super::{needs_directory, source_needs_directory};
use std::collections::HashMap;

#[test]
fn filesystem_and_location_access_require_a_directory() {
    for source in [
        "const fs = require('fs')",
        "import { readFile } from 'node:fs/promises'",
        "const { readFile } = await import('fs/promises')",
        "require('path').join(__dirname, 'asset.txt')",
        "new URL('./asset.txt', import.meta.url)",
        "process.dlopen(module, binding)",
    ] {
        assert!(source_needs_directory(source), "{source}");
    }
    assert!(!source_needs_directory("module.exports = require('child')"));
}

#[test]
fn data_files_are_not_scanned_as_source_and_blobs_are_shared() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    std::fs::write(&data, "node:fs").unwrap();
    let mut inspected = HashMap::new();
    assert!(
        !needs_directory(&HashMap::from([("asset.txt".to_string(), data.clone())]), &mut inspected)
            .unwrap(),
    );
    assert!(inspected.is_empty());
    assert!(
        needs_directory(&HashMap::from([("index.cjs".to_string(), data.clone())]), &mut inspected)
            .unwrap(),
    );
    assert_eq!(inspected.len(), 1);
    assert!(
        needs_directory(&HashMap::from([("bin/tool".to_string(), data.clone())]), &mut inspected)
            .unwrap(),
    );
    std::fs::remove_file(&data).unwrap();
    assert!(
        needs_directory(&HashMap::from([("index.mjs".to_string(), data)]), &mut inspected).unwrap(),
    );
}

#[test]
fn source_read_failures_include_the_blob_path() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("missing-source");
    let error = needs_directory(
        &HashMap::from([("index.js".to_string(), path.clone())]),
        &mut HashMap::new(),
    )
    .unwrap_err();
    let crate::CreateVirtualStoreError::LoadedBuildIo { path: failed_path, source } = error else {
        panic!("unexpected error: {error:?}");
    };
    assert_eq!(failed_path, path);
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
}
