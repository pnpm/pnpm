use super::{DirPatcher, InodeMap, Value, extend_files_map, file_id, publish_edits};
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt as _;
use std::{
    collections::HashMap,
    fs, io,
    path::PathBuf,
    time::{Duration, SystemTime},
};
use tempfile::TempDir;

fn create_file(path: &std::path::Path, content: &str) {
    fs::create_dir_all(path.parent().expect("file has a parent")).expect("create parent");
    fs::write(path, content).expect("write file");
}

#[cfg(unix)]
fn create_fifo(path: &std::path::Path) {
    fs::create_dir_all(path.parent().expect("fifo has a parent")).expect("create parent");
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("run mkfifo");
    assert!(status.success(), "mkfifo failed for {path:?}");
}

fn files_map(root: &std::path::Path, relative_paths: &[&str]) -> HashMap<String, PathBuf> {
    relative_paths
        .iter()
        .map(|relative| ((*relative).to_string(), root.join(relative)))
        .collect()
}

fn sync(source: &std::path::Path, target: &std::path::Path) {
    let patchers = DirPatcher::from_multiple_targets(source, &[target.to_path_buf()])
        .expect("diff source against target");
    for patcher in patchers {
        patcher.apply().expect("apply patch");
    }
}

#[test]
fn extend_files_map_names_every_ancestor() {
    let dir = TempDir::new().expect("temp dir");
    create_file(&dir.path().join("distribution/index.js"), "");

    let map = extend_files_map(&files_map(dir.path(), &["distribution/index.js"]))
        .expect("build inode map");

    let index_js = dir.path().join("distribution/index.js");
    let id = file_id(&index_js, &fs::metadata(&index_js).expect("stat")).expect("file id");
    assert_eq!(
        map,
        InodeMap::from([
            (".".to_string(), Value::Dir),
            ("distribution".to_string(), Value::Dir),
            ("distribution/index.js".to_string(), Value::File(id)),
        ]),
    );
}

#[cfg(unix)]
#[test]
fn extend_files_map_skips_an_inode_that_cannot_be_hardlinked() {
    let dir = TempDir::new().expect("temp dir");
    create_file(&dir.path().join("distribution/index.js"), "");
    create_fifo(&dir.path().join(".env"));

    let map = extend_files_map(&files_map(dir.path(), &["distribution/index.js", ".env"]))
        .expect("build inode map");

    assert!(!map.contains_key(".env"), "a FIFO belongs in no inode map: {map:?}");
    assert!(map.contains_key("distribution/index.js"));
}

#[test]
fn sync_replaces_a_target_entry_whose_inode_type_changed_in_the_source() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("became-a-dir/index.js"), "inner");
    create_file(&source.join("became-a-file"), "now a file");
    create_file(&target.join("became-a-dir"), "was a file");
    create_file(&target.join("became-a-file/index.js"), "was a dir");

    sync(&source, &target);

    assert_eq!(
        fs::read_to_string(target.join("became-a-dir/index.js")).expect("read replaced dir"),
        "inner",
    );
    assert_eq!(
        fs::read_to_string(target.join("became-a-file")).expect("read replaced file"),
        "now a file",
    );
}

#[test]
fn sync_removes_what_the_source_no_longer_has() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("keep.txt"), "kept");
    create_file(&target.join("keep.txt"), "kept");
    create_file(&target.join("stale/nested.txt"), "stale");

    sync(&source, &target);

    assert!(target.join("keep.txt").exists());
    assert!(!target.join("stale").exists(), "a directory the source dropped is removed");
}

#[cfg(unix)]
#[test]
fn sync_replaces_a_skipped_inode_the_target_holds() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("config.env"), "real");
    create_file(&source.join("other.txt"), "");
    // The diff cannot see a FIFO, so it never schedules one for
    // removal and creating over it would fail with EEXIST.
    create_fifo(&target.join("config.env"));

    sync(&source, &target);

    assert_eq!(fs::read_to_string(target.join("config.env")).expect("read replacement"), "real");
    assert!(target.join("other.txt").exists());
}

#[cfg(unix)]
#[test]
fn sync_leaves_a_skipped_inode_the_source_does_not_cover() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("keep.txt"), "kept");
    create_fifo(&target.join("own.env"));

    sync(&source, &target);

    let metadata = fs::symlink_metadata(target.join("own.env")).expect("stat the FIFO");
    assert!(metadata.file_type().is_fifo(), "a FIFO of the target's own is left alone");
}

#[test]
fn sync_shares_inodes_with_the_source() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("lib/index.js"), "built");
    fs::create_dir_all(&target).expect("create target");

    sync(&source, &target);

    let (source_path, target_path) = (source.join("lib/index.js"), target.join("lib/index.js"));
    let source_stat = fs::metadata(&source_path).expect("stat source");
    let target_stat = fs::metadata(&target_path).expect("stat target");
    assert_eq!(
        file_id(&source_path, &source_stat).expect("source file id"),
        file_id(&target_path, &target_stat).expect("target file id"),
    );
}

/// Hardlinks fail with the error each platform raises when the target
/// is on another filesystem than the source.
struct CrossDeviceLinks;

impl pnpm_fs::FsHardLink for CrossDeviceLinks {
    fn hard_link(_: &std::path::Path, _: &std::path::Path) -> io::Result<()> {
        #[cfg(windows)]
        return Err(io::Error::from_raw_os_error(17));
        #[cfg(not(windows))]
        return Err(io::Error::from_raw_os_error(18));
    }
}

struct DeniedLinks;

impl pnpm_fs::FsHardLink for DeniedLinks {
    fn hard_link(_: &std::path::Path, _: &std::path::Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

fn sync_with<Sys: pnpm_fs::FsHardLink>(
    source: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), super::PatchError> {
    let patch = super::diff_dir(
        &super::load_inode_map(target).expect("target inode map"),
        &super::load_inode_map(source).expect("source inode map"),
    );
    super::apply_patch_with_link::<Sys>(&patch, source, target)
}

#[test]
fn sync_copies_when_the_target_is_on_another_filesystem() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("lib/index.js"), "built");
    fs::create_dir_all(&target).expect("create target");

    sync_with::<CrossDeviceLinks>(&source, &target).expect("sync by copying");

    assert_eq!(fs::read_to_string(target.join("lib/index.js")).expect("read copy"), "built");
}

/// A copy never shares its source's identity, so nothing short of
/// recopying tells a same-length rewrite apart from an unchanged file.
#[test]
fn sync_refreshes_a_copy_whose_source_changed_in_place() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("index.js"), "old");
    fs::create_dir_all(&target).expect("create target");
    sync_with::<CrossDeviceLinks>(&source, &target).expect("first sync");

    fs::write(source.join("index.js"), "new").expect("rewrite source");
    sync_with::<CrossDeviceLinks>(&source, &target).expect("second sync");

    assert_eq!(fs::read_to_string(target.join("index.js")).expect("read copy"), "new");
}

#[test]
fn sync_reports_a_link_error_that_is_not_cross_device() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("index.js"), "built");
    fs::create_dir_all(&target).expect("create target");

    let error = sync_with::<DeniedLinks>(&source, &target).expect_err("the link error surfaces");

    assert!(
        matches!(&error, super::PatchError::Link { error, .. } if error.kind() == io::ErrorKind::PermissionDenied),
        "{error:?}",
    );
    assert!(!target.join("index.js").exists(), "nothing was copied");
}

fn device_and_inode(path: &std::path::Path) -> (u64, u64) {
    let metadata = fs::metadata(path).expect("stat");
    let id = file_id(path, &metadata).expect("file id");
    (id.device, id.inode)
}

#[test]
fn publish_replaces_an_edited_hardlink_with_its_own_file() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("index.js"), "old");
    fs::create_dir_all(&target).expect("create target");
    fs::hard_link(source.join("index.js"), target.join("index.js")).expect("hardlink");
    fs::write(source.join("index.js"), "new").expect("rewrite the hardlink in place");

    publish_edits(&source, &target, SystemTime::UNIX_EPOCH).expect("publish");

    assert_eq!(fs::read_to_string(target.join("index.js")).expect("read published file"), "new");
    assert_ne!(
        device_and_inode(&source.join("index.js")),
        device_and_inode(&target.join("index.js")),
        "a published file has its own inode, so a watcher on the injected directory sees the write",
    );
}

#[test]
fn publish_leaves_a_hardlink_that_was_not_edited_in_this_watch() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("index.js"), "same");
    fs::create_dir_all(&target).expect("create target");
    fs::hard_link(source.join("index.js"), target.join("index.js")).expect("hardlink");
    let edited_since = SystemTime::now()
        .checked_add(Duration::from_secs(86_400))
        .expect("a deadline past every current mtime");

    publish_edits(&source, &target, edited_since).expect("publish");

    assert_eq!(
        device_and_inode(&source.join("index.js")),
        device_and_inode(&target.join("index.js")),
        "an untouched hardlink stays shared",
    );
}

#[test]
fn publish_does_not_recopy_a_file_whose_length_and_mtime_match() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("index.js"), "built");
    fs::create_dir_all(&target).expect("create target");

    publish_edits(&source, &target, SystemTime::UNIX_EPOCH).expect("first publish");
    let published = device_and_inode(&target.join("index.js"));
    publish_edits(&source, &target, SystemTime::UNIX_EPOCH).expect("second publish");

    assert_eq!(published, device_and_inode(&target.join("index.js")));
    assert_eq!(fs::read_to_string(target.join("index.js")).expect("read copy"), "built");
}
