use super::{HASH_ALGORITHM, calculate_diff, upload};
use crate::{
    CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex, StoreIndexWriter, add_files_from_dir,
};
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs as unix_fs;
use std::{collections::HashMap, fs, path::Path};
use tempfile::tempdir;

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) {
    unix_fs::symlink(target, link).expect("create directory symlink");
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) {
    junction::create(target, link).expect("create directory junction");
}

fn info(digest: &str, mode: u32, size: u64) -> CafsFileInfo {
    CafsFileInfo { digest: digest.to_string(), mode, size, checked_at: None }
}

fn map(entries: &[(&str, CafsFileInfo)]) -> HashMap<String, CafsFileInfo> {
    entries
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                CafsFileInfo {
                    digest: v.digest.clone(),
                    mode: v.mode,
                    size: v.size,
                    checked_at: v.checked_at,
                },
            )
        })
        .collect()
}

#[test]
fn identical_maps_yield_no_diff() {
    let files = map(&[("a", info("d-a", 0o644, 1))]);
    let diff = calculate_diff(&files, &files);
    assert_eq!(diff.added, None);
    assert_eq!(diff.deleted, None);
}

#[test]
fn added_only() {
    let base = HashMap::new();
    let current = map(&[("new", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert!(added.contains_key("new"));
}

#[test]
fn deleted_only() {
    let base = map(&[("gone", info("d-gone", 0o644, 1))]);
    let current = HashMap::new();
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.added, None);
    let deleted = diff.deleted.expect("deleted present");
    assert_eq!(deleted, vec!["gone".to_string()]);
}

#[test]
fn digest_change_appears_in_added() {
    let base = map(&[("f.txt", info("d-old", 0o644, 1))]);
    let current = map(&[("f.txt", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.txt").unwrap().digest, "d-new");
}

#[test]
fn mode_change_appears_in_added() {
    let base = map(&[("f.sh", info("d", 0o644, 1))]);
    let current = map(&[("f.sh", info("d", 0o755, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.sh").unwrap().mode, 0o755);
}

#[test]
fn mixed_changes() {
    let base = map(&[
        ("keep", info("d-keep", 0o644, 1)),
        ("gone", info("d-gone", 0o644, 1)),
        ("changed", info("d-old", 0o644, 1)),
    ]);
    let current = map(&[
        ("keep", info("d-keep", 0o644, 1)),
        ("changed", info("d-new", 0o644, 1)),
        ("fresh", info("d-fresh", 0o644, 1)),
    ]);
    let diff = calculate_diff(&base, &current);
    let added = diff.added.expect("added present");
    let mut added_keys: Vec<_> = added.keys().cloned().collect();
    added_keys.sort();
    assert_eq!(added_keys, vec!["changed".to_string(), "fresh".to_string()]);
    assert_eq!(diff.deleted, Some(vec!["gone".to_string()]));
}

#[tokio::test]
async fn symlinked_output_is_not_cached() {
    let store_root = tempdir().expect("create store root");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store dir");
    let pkg_dir = tempdir().expect("create package dir");
    fs::write(pkg_dir.path().join("package.json"), r#"{"name":"symlink-output"}"#)
        .expect("write package manifest");
    let target_dir = pkg_dir.path().join("generated");
    fs::create_dir(&target_dir).expect("create generated directory");
    fs::write(target_dir.join("index.js"), "module.exports = true").expect("write target");

    let base_files = add_files_from_dir(&store_dir, pkg_dir.path()).expect("hash base package");
    let files_index_file = "symlink-side-effects-pkg";
    let index = StoreIndex::open(store_dir.root()).expect("open store index");
    index
        .set(
            files_index_file,
            &PackageFilesIndex {
                manifest: None,
                requires_build: Some(true),
                algo: HASH_ALGORITHM.to_string(),
                files: base_files.files,
                side_effects: None,
            },
        )
        .expect("write base package index");
    drop(index);

    symlink_dir(&target_dir, &pkg_dir.path().join("generated-link"));
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);
    upload(&store_dir, pkg_dir.path(), files_index_file, "test-engine", writer.as_ref())
        .expect("upload side effects");
    drop(writer);
    writer_task.await.expect("join store writer").expect("flush store writer");

    let index = StoreIndex::open(store_dir.root()).expect("reopen store index");
    let files_index =
        index.get(files_index_file).expect("read package index").expect("package index exists");
    assert_eq!(files_index.side_effects, None);
}
