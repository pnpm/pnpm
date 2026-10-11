use super::{CasPackageFiles, PackageFiles};
use std::{collections::HashMap, fs, path::PathBuf};

#[test]
fn cas_files_resolve_paths_under_the_package_directory() {
    let store = tempfile::tempdir().expect("create store dir");
    let lockfile = store.path().join("lockfile-blob");
    fs::write(&lockfile, "# yarn lockfile v1\n").expect("write blob");
    let files: HashMap<String, PathBuf> = [
        ("packages/foo/package.json".to_string(), store.path().join("manifest-blob")),
        ("packages/foo/yarn.lock".to_string(), lockfile),
    ]
    .into_iter()
    .collect();
    let package = CasPackageFiles { files: &files, dir: Some("/packages/foo") };

    assert!(package.contains("package.json"));
    assert!(package.contains("./package.json"));
    assert!(!package.contains("binding.gyp"));
    assert_eq!(package.read_head("yarn.lock", 6).as_deref(), Some(&b"# yarn"[..]));
}

#[test]
fn cas_files_at_the_archive_root() {
    let files = HashMap::from([("bun.lock".to_string(), PathBuf::from("/nowhere"))]);
    let root = CasPackageFiles { files: &files, dir: None };

    assert!(root.contains("bun.lock"));
    assert_eq!(root.read_head("bun.lock", 10), None);
}

#[test]
fn cas_files_contain_directories() {
    let files = HashMap::from([("pkg/lib/index.js".to_string(), PathBuf::from("/nowhere"))]);
    let package = CasPackageFiles { files: &files, dir: Some("pkg") };

    assert!(package.contains("lib"));
    assert!(package.contains("./lib/"));
    assert!(!package.contains("li"));
}
