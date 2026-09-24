use super::{DirPatcher, FileEntry, InodeMap, Value, extend_files_map, file_id};
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt as _;
use std::{collections::HashMap, fs, path::PathBuf};
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
    let meta = fs::metadata(&index_js).expect("stat");
    let entry = FileEntry {
        id: file_id(&index_js, &meta).expect("file id"),
        size: meta.len(),
        modified: meta.modified().ok(),
    };
    assert_eq!(
        map,
        InodeMap::from([
            (".".to_string(), Value::Dir),
            ("distribution".to_string(), Value::Dir),
            ("distribution/index.js".to_string(), Value::File(entry)),
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

struct CrossDeviceHardLink;

impl pnpm_fs::FsHardLink for CrossDeviceHardLink {
    fn hard_link(_source: &std::path::Path, _target: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices))
    }
}

impl pnpm_fs::FsReflink for CrossDeviceHardLink {
    fn reflink(_source: &std::path::Path, _target: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

struct RawOsCrossDeviceHardLink;

impl pnpm_fs::FsHardLink for RawOsCrossDeviceHardLink {
    fn hard_link(_source: &std::path::Path, _target: &std::path::Path) -> std::io::Result<()> {
        #[cfg(unix)]
        return Err(std::io::Error::from_raw_os_error(18));
        #[cfg(windows)]
        return Err(std::io::Error::from_raw_os_error(17));
        #[cfg(not(any(unix, windows)))]
        return Err(std::io::Error::from(std::io::ErrorKind::CrossesDevices));
    }
}

impl pnpm_fs::FsReflink for RawOsCrossDeviceHardLink {
    fn reflink(_source: &std::path::Path, _target: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

#[test]
fn sync_falls_back_to_copy_on_cross_device_link() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("lib/index.js"), "built");
    create_file(&source.join("assets/logo.png"), "image-bytes");
    fs::create_dir_all(&target).expect("create target");

    let source_map = super::load_inode_map(&source).expect("source inode map");
    let target_map = super::load_inode_map(&target).expect("target inode map");
    let patch = super::diff_dir(&target_map, &source_map);
    super::apply_patch_with_link::<CrossDeviceHardLink>(&patch, &source, &target)
        .expect("apply patch with cross-device link fallback");

    let (source_path, target_path) = (source.join("lib/index.js"), target.join("lib/index.js"));
    assert_eq!(fs::read_to_string(&target_path).expect("read target file"), "built");
    assert_eq!(
        fs::read_to_string(target.join("assets/logo.png")).expect("read target image"),
        "image-bytes",
    );

    let source_stat = fs::metadata(&source_path).expect("stat source");
    let target_stat = fs::metadata(&target_path).expect("stat target");
    assert_ne!(
        file_id(&source_path, &source_stat).expect("source file id"),
        file_id(&target_path, &target_stat).expect("target file id"),
    );
}

#[test]
fn sync_falls_back_to_copy_on_raw_os_cross_device_error() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("main.js"), "content");
    fs::create_dir_all(&target).expect("create target");

    let source_map = super::load_inode_map(&source).expect("source inode map");
    let target_map = super::load_inode_map(&target).expect("target inode map");
    let patch = super::diff_dir(&target_map, &source_map);
    super::apply_patch_with_link::<RawOsCrossDeviceHardLink>(&patch, &source, &target)
        .expect("apply patch with raw os cross device error");

    assert_eq!(fs::read_to_string(target.join("main.js")).expect("read copied file"), "content");
}

#[test]
fn sync_repeatedly_does_not_recopy_unchanged_fallback_files() {
    let dir = TempDir::new().expect("temp dir");
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    create_file(&source.join("lib/index.js"), "built");
    fs::create_dir_all(&target).expect("create target");

    let source_map = super::load_inode_map(&source).expect("source inode map");
    let target_map = super::load_inode_map(&target).expect("target inode map");
    let patch = super::diff_dir(&target_map, &source_map);
    super::apply_patch_with_link::<CrossDeviceHardLink>(&patch, &source, &target)
        .expect("initial sync");

    let source_map = super::load_inode_map(&source).expect("source inode map");
    let target_map = super::load_inode_map(&target).expect("target inode map");
    let repeated_patch = super::diff_dir(&target_map, &source_map);
    assert!(
        repeated_patch.changes.is_empty(),
        "repeated sync should produce no changes for unchanged files, got: {:?}",
        repeated_patch.changes,
    );
    assert!(repeated_patch.removed.is_empty());
}
