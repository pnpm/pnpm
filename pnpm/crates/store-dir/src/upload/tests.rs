use super::{HASH_ALGORITHM, calculate_diff, upload, upload_with_diff};
#[cfg(unix)]
use crate::SYMLINK_MODE;
use crate::{
    CafsFileInfo, PackageFilesIndex, SideEffectsDiff, StoreDir, StoreIndex, StoreIndexWriter,
    add_files_from_dir,
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

#[cfg(unix)]
#[test]
fn mode_change_appears_in_added() {
    let base = map(&[("f.sh", info("d", 0o644, 1))]);
    let current = map(&[("f.sh", info("d", 0o755, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("f.sh").unwrap().mode, 0o755);
}

/// Off Unix a mode-only difference is not observable build output.
///
/// Regression for <https://github.com/pnpm/pnpm/issues/15667>.
#[cfg(not(unix))]
#[test]
fn mode_change_alone_yields_no_diff() {
    let base = map(&[("cli.js", info("d", 0o755, 1))]);
    let current = map(&[("cli.js", info("d", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.added, None);
    assert_eq!(diff.deleted, None);
}

#[test]
fn digest_change_appears_in_added_when_mode_also_changes() {
    let base = map(&[("cli.js", info("d-old", 0o755, 1))]);
    let current = map(&[("cli.js", info("d-new", 0o644, 1))]);
    let diff = calculate_diff(&base, &current);
    assert_eq!(diff.deleted, None);
    let added = diff.added.expect("added present");
    assert_eq!(added.get("cli.js").unwrap().digest, "d-new");
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
async fn unrecordable_symlink_output_is_not_cached() {
    for with_diff in [false, true] {
        check_unrecordable_symlink_output(with_diff, &[]).await;
    }
}

#[tokio::test]
async fn unrecordable_symlink_output_removes_only_matching_cached_entry() {
    for with_diff in [false, true] {
        check_unrecordable_symlink_output(with_diff, &["test-engine", "other-engine"]).await;
    }
}

#[tokio::test]
async fn unrecordable_symlink_output_removes_last_cached_entry() {
    for with_diff in [false, true] {
        check_unrecordable_symlink_output(with_diff, &["test-engine"]).await;
    }
}

async fn check_unrecordable_symlink_output(with_diff: bool, cached_keys: &[&str]) {
    let store_root = tempdir().expect("create store root");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store dir");
    let pkg_dir = tempdir().expect("create package dir");
    let mut base_index = create_base_index(&store_dir, pkg_dir.path());
    let cached_diff = SideEffectsDiff {
        added: Some(map(&[("cached.js", info("cached", 0o644, 6))])),
        deleted: None,
        remote_origin: None,
    };
    base_index.side_effects = (!cached_keys.is_empty()).then(|| {
        cached_keys
            .iter()
            .map(|key| ((*key).to_string(), cached_diff.clone()))
            .collect()
    });
    let files_index_file = "symlink-side-effects-pkg";
    let index = StoreIndex::open(store_dir.root()).expect("open store index");
    index.set(files_index_file, &base_index).expect("write base package index");
    drop(index);

    symlink_dir(&pkg_dir.path().join("generated"), &pkg_dir.path().join("generated-link"));
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);
    if cached_keys.contains(&"other-engine") {
        writer.queue_side_effects_upload(
            files_index_file.to_string(),
            "queued-engine".to_string(),
            map(&[("queued.js", info("queued", 0o644, 6))]),
        );
    }
    if with_diff {
        let diff = upload_with_diff(
            &store_dir,
            pkg_dir.path(),
            files_index_file,
            "test-engine",
            writer.as_ref(),
        )
        .expect("upload side effects with diff");
        assert_eq!(diff, None);
    } else {
        upload(&store_dir, pkg_dir.path(), files_index_file, "test-engine", writer.as_ref())
            .expect("upload side effects");
    }
    drop(writer);
    writer_task.await.expect("join store writer").expect("flush store writer");

    let index = StoreIndex::open(store_dir.root()).expect("reopen store index");
    let files_index = index
        .get(files_index_file)
        .expect("read package index")
        .expect("package index exists");
    assert_eq!(files_index.files, base_index.files);
    assert_eq!(files_index.requires_build, Some(true));
    if cached_keys.contains(&"other-engine") {
        let side_effects = files_index.side_effects.expect("other cache entries preserved");
        assert!(!side_effects.contains_key("test-engine"));
        assert_eq!(side_effects.len(), 2);
        assert_eq!(side_effects["other-engine"], cached_diff);
        assert_eq!(
            side_effects["queued-engine"].added,
            Some(map(&[("queued.js", info("queued", 0o644, 6))])),
        );
    } else {
        assert_eq!(files_index.side_effects, None);
    }
}

#[tokio::test]
async fn upload_caches_changes_to_indexed_bundled_dependencies() {
    for base_row_committed in [true, false] {
        check_bundled_dependency_upload(base_row_committed).await;
    }
}

async fn check_bundled_dependency_upload(base_row_committed: bool) {
    let store_root = tempdir().expect("create store root");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store dir");
    let pkg_dir = tempdir().expect("create package dir");
    for (path, content) in [
        ("node_modules/abbrev/package.json", "patched"),
        ("node_modules/abbrev/helper.js", "added by patch"),
        ("node_modules/@scope/bundled/index.js", "pristine"),
        ("node_modules/unbundled/index.js", "ignored"),
    ] {
        let path = pkg_dir.path().join(path);
        fs::create_dir_all(path.parent().expect("parent dir")).expect("create dependency dir");
        fs::write(path, content).expect("write dependency file");
    }
    let patched = store_dir.write_cas_file(b"pristine", false).expect("write pristine file");
    let scoped = store_dir.write_cas_file(b"pristine", false).expect("write scoped file");
    let index_key = "bundled-dependency-side-effects-pkg";
    let base_index = PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        requires_prepare: None,
        algo: HASH_ALGORITHM.to_string(),
        files: map(&[
            ("node_modules/abbrev/package.json", patched.index_entry(0o644, None)),
            ("node_modules/@scope/bundled/index.js", scoped.index_entry(0o644, None)),
        ]),
        side_effects: None,
        remote_side_effects_quarantine: None,
        tar_integrity: None,
    };
    if base_row_committed {
        StoreIndex::open(store_dir.root())
            .expect("open store index")
            .set(index_key, &base_index)
            .expect("write base package index");
    }
    let (writer, task) = StoreIndexWriter::spawn(&store_dir);
    if !base_row_committed {
        writer.queue(index_key.to_string(), base_index);
    }

    let diff = upload_with_diff(&store_dir, pkg_dir.path(), index_key, "test-engine", &writer)
        .expect("upload side effects")
        .expect("base index entry");

    let mut added: Vec<_> = diff.added
        .expect("bundled changes recorded")
        .into_keys()
        .collect();
    added.sort();
    assert_eq!(
        (added, diff.deleted),
        (
            vec![
                "node_modules/abbrev/helper.js".to_string(),
                "node_modules/abbrev/package.json".to_string(),
            ],
            None,
        ),
        "base row committed: {base_row_committed}",
    );
    drop(writer);
    task.await.expect("join store index writer").expect("flush side effects");
}

// The disabled writer drains its channel on the runtime, so the blocking
// read needs a second worker thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_is_skipped_when_the_bundled_paths_cannot_be_read() {
    let store_root = tempdir().expect("create store root");
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    store_dir.init().expect("init store dir");
    let pkg_dir = tempdir().expect("create package dir");
    let bundled_file = pkg_dir.path().join("node_modules/abbrev/package.json");
    fs::create_dir_all(bundled_file.parent().expect("parent dir")).expect("create bundled dir");
    fs::write(&bundled_file, "patched").expect("write bundled file");
    let (writer, task) = StoreIndexWriter::spawn_disabled();

    let diff = upload_with_diff(&store_dir, pkg_dir.path(), "unreadable-row", "engine", &writer)
        .expect("upload side effects");

    assert!(diff.is_none());
    drop(writer);
    task.await.expect("join store index writer").expect("drain writer");
}

#[cfg(unix)]
#[tokio::test]
async fn relative_symlink_output_is_cached_as_symlink_entry() {
    for with_diff in [false, true] {
        let store_root = tempdir().expect("create store root");
        let store_dir = StoreDir::from(store_root.path().to_path_buf());
        store_dir.init().expect("init store dir");
        let pkg_dir = tempdir().expect("create package dir");
        let base_index = create_base_index(&store_dir, pkg_dir.path());
        let files_index_file = "recorded-symlink-pkg";
        let index = StoreIndex::open(store_dir.root()).expect("open store index");
        index.set(files_index_file, &base_index).expect("write base package index");
        drop(index);

        unix_fs::symlink("generated", pkg_dir.path().join("generated-link"))
            .expect("create relative symlink");

        let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);
        let returned_diff = if with_diff {
            upload_with_diff(
                &store_dir,
                pkg_dir.path(),
                files_index_file,
                "test-engine",
                writer.as_ref(),
            )
            .expect("upload side effects with diff")
        } else {
            upload(&store_dir, pkg_dir.path(), files_index_file, "test-engine", writer.as_ref())
                .expect("upload side effects");
            None
        };
        drop(writer);
        writer_task.await.expect("join store writer").expect("flush store writer");

        let index = StoreIndex::open(store_dir.root()).expect("reopen store index");
        let files_index = index
            .get(files_index_file)
            .expect("read package index")
            .expect("package index exists");
        let diff = &files_index.side_effects.expect("side effects cached")["test-engine"];
        let added = diff.added.as_ref().expect("symlink added");
        assert_eq!(added.keys().collect::<Vec<_>>(), ["generated-link"]);
        let link = &added["generated-link"];
        assert_eq!(link.mode, SYMLINK_MODE);
        let cas_path =
            store_dir.cas_file_path_by_mode(&link.digest, link.mode).expect("symlink CAFS path");
        assert_eq!(fs::read_to_string(cas_path).expect("read symlink target"), "generated");
        if let Some(returned_diff) = returned_diff {
            assert_eq!(diff, &returned_diff);
        }
    }
}

#[tokio::test]
async fn top_level_node_modules_link_does_not_prevent_caching() {
    for with_diff in [false, true] {
        let store_root = tempdir().expect("create store root");
        let store_dir = StoreDir::from(store_root.path().to_path_buf());
        store_dir.init().expect("init store dir");
        let pkg_dir = tempdir().expect("create package dir");
        let base_index = create_base_index(&store_dir, pkg_dir.path());
        let files_index_file = "node-modules-side-effects-pkg";
        let index = StoreIndex::open(store_dir.root()).expect("open store index");
        index.set(files_index_file, &base_index).expect("write base package index");
        drop(index);

        let dependency_dir = tempdir().expect("create dependency dir");
        fs::write(dependency_dir.path().join("dependency.js"), "module.exports = true")
            .expect("write dependency");
        symlink_dir(dependency_dir.path(), &pkg_dir.path().join("node_modules"));
        fs::write(pkg_dir.path().join("built.js"), "module.exports = 'built'")
            .expect("write build output");

        let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);
        let returned_diff = if with_diff {
            let diff = upload_with_diff(
                &store_dir,
                pkg_dir.path(),
                files_index_file,
                "test-engine",
                writer.as_ref(),
            )
            .expect("upload side effects with diff");
            assert!(diff.is_some());
            diff
        } else {
            upload(&store_dir, pkg_dir.path(), files_index_file, "test-engine", writer.as_ref())
                .expect("upload side effects");
            None
        };
        drop(writer);
        writer_task.await.expect("join store writer").expect("flush store writer");

        let index = StoreIndex::open(store_dir.root()).expect("reopen store index");
        let files_index = index
            .get(files_index_file)
            .expect("read package index")
            .expect("package index exists");
        let side_effects = files_index.side_effects.expect("side effects cached");
        let diff = &side_effects["test-engine"];
        let added = diff.added.as_ref().expect("built file added");
        assert_eq!(added.len(), 1);
        assert!(added.contains_key("built.js"));
        assert_eq!(diff.deleted, None);
        if let Some(returned_diff) = returned_diff {
            assert_eq!(diff, &returned_diff);
        }
    }
}

fn create_base_index(store_dir: &StoreDir, pkg_dir: &Path) -> PackageFilesIndex {
    fs::write(pkg_dir.join("package.json"), r#"{"name":"symlink-output"}"#)
        .expect("write package manifest");
    let target_dir = pkg_dir.join("generated");
    fs::create_dir(&target_dir).expect("create generated directory");
    fs::write(target_dir.join("index.js"), "module.exports = true").expect("write target");

    let base_files = add_files_from_dir(store_dir, pkg_dir).expect("hash base package");
    PackageFilesIndex {
        manifest: None,
        requires_build: Some(true),
        requires_prepare: None,
        algo: HASH_ALGORITHM.to_string(),
        files: base_files.files,
        side_effects: None,
        remote_side_effects_quarantine: None,
        tar_integrity: None,
    }
}
