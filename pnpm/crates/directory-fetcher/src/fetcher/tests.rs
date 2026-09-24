use super::DirectoryFetcher;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn confined_all_files_fetcher_keeps_symlink_sources_without_resolve_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "x", "version": "0.0.0", "files": ["link.txt"] }"#,
    )
    .unwrap();
    fs::write(root.join("real.txt"), "content").unwrap();
    symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

    let output = DirectoryFetcher {
        directory: root.clone(),
        include_only_package_files: false,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .unwrap();

    assert_eq!(output.files_map.get("link.txt"), Some(&root.join("link.txt")));

    let output_resolved = DirectoryFetcher {
        directory: root.clone(),
        include_only_package_files: false,
        resolve_symlinks: true,
        allow_path_escape: false,
    }
    .run()
    .unwrap();

    assert_eq!(output_resolved.files_map.get("link.txt"), Some(&root.join("real.txt")));
}

#[cfg(any(unix, windows))]
#[test]
fn confined_package_files_fetcher_packs_a_linked_root() {
    let dir = tempdir().unwrap();
    let real_root = dir.path().join("real-root");
    fs::create_dir_all(&real_root).unwrap();
    fs::write(real_root.join("package.json"), r#"{ "name": "x", "version": "0.0.0" }"#).unwrap();
    fs::write(real_root.join("index.js"), "content").unwrap();
    let root_link = dir.path().join("root-link");
    pnpm_fs::symlink_dir(&real_root, &root_link).unwrap();

    let output = DirectoryFetcher {
        directory: root_link,
        include_only_package_files: true,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .unwrap();

    assert_eq!(
        output.files_map.get("index.js"),
        Some(&fs::canonicalize(real_root.join("index.js")).unwrap()),
    );
}
