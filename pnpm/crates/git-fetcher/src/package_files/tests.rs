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

/// A manifest's `main` cannot make the build check look outside the
/// package.
#[test]
fn paths_outside_the_package_name_no_file() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("package");
    fs::create_dir_all(&package).unwrap();
    fs::write(root.path().join("outside.js"), "").unwrap();
    let files = HashMap::from([("outside.js".to_string(), root.path().join("outside.js"))]);
    let archive = CasPackageFiles { files: &files, dir: Some("package") };

    let absolute = root.path().join("outside.js");
    let absolute = absolute.to_string_lossy();
    let package = package.as_path();
    assert!(!package.contains("../outside.js"));
    assert!(!package.contains(&absolute));
    assert_eq!(package.read_head("../outside.js", 1), None);
    assert!(!archive.contains("../outside.js"));
    if cfg!(windows) {
        assert!(!package.contains(r"..\outside.js"));
    }
}

#[cfg(unix)]
#[test]
fn symlinked_main_cannot_escape_the_package() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("package");
    fs::create_dir_all(&package).unwrap();
    let outside = root.path().join("outside.js");
    fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, package.join("main.js")).unwrap();

    assert!(!package.as_path().contains("main.js"));
    assert_eq!(package.as_path().read_head("main.js", 10), None);
}
