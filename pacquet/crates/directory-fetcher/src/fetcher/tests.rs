use super::DirectoryFetcher;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn confined_all_files_fetcher_rewrites_symlink_sources_to_real_paths() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{ "name": "x", "version": "0.0.0", "files": ["link.txt"] }"#,
    )
    .unwrap();
    fs::write(root.join("real.txt"), "content").unwrap();
    symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

    let output = DirectoryFetcher {
        directory: root.to_path_buf(),
        include_only_package_files: false,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .unwrap();

    assert_eq!(
        output.files_map.get("link.txt"),
        Some(&fs::canonicalize(root.join("real.txt")).unwrap()),
    );
}

#[cfg(any(unix, windows))]
#[test]
fn confined_package_files_fetcher_rejects_linked_root() {
    let dir = tempdir().unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("package.json"), r#"{ "name": "x", "version": "0.0.0" }"#).unwrap();
    fs::write(outside.join("index.js"), "content").unwrap();
    let root_link = dir.path().join("root-link");
    pacquet_fs::symlink_dir(&outside, &root_link).unwrap();

    let Err(err) = (DirectoryFetcher {
        directory: root_link,
        include_only_package_files: true,
        resolve_symlinks: false,
        allow_path_escape: false,
    })
    .run() else {
        panic!("linked root should be rejected before packlist walks it");
    };

    assert!(
        err.to_string().contains("resolves outside source directory"),
        "unexpected error: {err}",
    );
}
