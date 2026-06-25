#[cfg(unix)]
use super::DirectoryFetcher;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
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
