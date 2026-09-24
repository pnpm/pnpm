use super::{
    FsRemoveDirAll, FsRemoveNonDirDirent, Placement, clear_dir_blocking_file,
    clear_dirent_blocking_dir, dir_fits_at, entries_by_target_dir, file_fits_at, populate_dir,
    symlinks::{SymlinkRoots, final_link_target},
};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError, atomic::AtomicU8},
};
use tempfile::tempdir;

/// Keyed by absolute path, so tests sharing the process only ever read
/// their own directories.
static DIRECTORY_WRITERS: Mutex<BTreeMap<PathBuf, DirectoryWriters>> = Mutex::new(BTreeMap::new());

#[derive(Default)]
pub(super) struct DirectoryWriters {
    current: usize,
    max: usize,
}

impl DirectoryWriters {
    /// Count one worker into the directory `cleaned_entry` lands in until
    /// the guard drops.
    pub(super) fn enter(dir_path: &Path, cleaned_entry: &str) -> DirectoryWriter {
        let directory =
            dir_path.join(Path::new(cleaned_entry).parent().unwrap_or_else(|| Path::new("")));
        let mut writers = DIRECTORY_WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        let writers = writers.entry(directory.clone()).or_default();
        writers.current += 1;
        writers.max = writers.max.max(writers.current);
        DirectoryWriter { directory }
    }

    fn max_at_once(directory: &Path) -> usize {
        DIRECTORY_WRITERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(directory)
            .map_or(0, |writers| writers.max)
    }
}

pub(super) struct DirectoryWriter {
    directory: PathBuf,
}

impl Drop for DirectoryWriter {
    fn drop(&mut self) {
        let mut writers = DIRECTORY_WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(writers) = writers.get_mut(&self.directory) {
            writers.current -= 1;
        }
    }
}

#[test]
fn a_directory_has_one_writer_at_a_time() {
    let root = tempdir().unwrap();
    let store = root.path().join("store");
    fs::create_dir(&store).unwrap();
    let mut cas_paths: HashMap<String, PathBuf> = HashMap::new();
    for directory in ["lib", "lib/esm"] {
        for index in 0..200 {
            let store_path = store.join(format!("{}-{index}", directory.replace('/', "-")));
            fs::write(&store_path, b"payload").unwrap();
            cas_paths.insert(format!("{directory}/{index}.js"), store_path);
        }
    }
    let manifest = store.join("package.json");
    fs::write(&manifest, b"{}").unwrap();
    cas_paths.insert("package.json".to_string(), manifest);
    let target = root.path().join("pkg");

    populate_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas_paths,
        Placement::Fresh,
        None,
    )
    .unwrap();

    for directory in ["lib", "lib/esm"] {
        assert_eq!(DirectoryWriters::max_at_once(&target.join(directory)), 1, "{directory}");
    }
    assert!(target.join("lib/199.js").is_file());
    assert!(target.join("lib/esm/199.js").is_file());
    assert!(target.join("package.json").is_file());
}

#[test]
fn entries_are_grouped_by_the_directory_they_land_in() {
    let cas_paths: HashMap<String, PathBuf> =
        ["package.json", "README.md", "lib/index.js", "lib/util.js", "lib/esm/index.js"]
            .into_iter()
            .map(|entry| (entry.to_string(), PathBuf::from("store").join(entry)))
            .collect();

    let groups: BTreeSet<BTreeSet<&str>> = entries_by_target_dir(&cas_paths, Some("package.json"))
        .iter()
        .map(|entries| {
            entries
                .iter()
                .map(|(entry, store_path)| {
                    assert_eq!(*store_path, cas_paths[*entry]);
                    *entry
                })
                .collect()
        })
        .collect();

    assert_eq!(
        groups,
        BTreeSet::from([
            BTreeSet::from(["README.md"]),
            BTreeSet::from(["lib/index.js", "lib/util.js"]),
            BTreeSet::from(["lib/esm/index.js"]),
        ]),
    );
}

#[test]
fn nothing_at_a_path_fits_either_kind() {
    let root = tempdir().unwrap();
    let absent = root.path().join("absent");

    assert!(dir_fits_at(&absent));
    assert!(file_fits_at(&absent));
}

#[test]
fn a_directory_fits_only_where_a_directory_belongs() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    assert!(dir_fits_at(&target));
    assert!(!file_fits_at(&target));
}

#[test]
fn a_file_fits_only_where_a_file_belongs() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"payload").unwrap();

    assert!(file_fits_at(&target));
    assert!(!dir_fits_at(&target));
}

// Each fake stands in for the installer sharing the slot, which can finish
// the same work between this one's inspection and its removal. The removal
// always fails, because that is what the loser of the race sees; what the
// path holds by then is what the fakes vary.
struct ReplacedByWhatBelongs;
struct RemovedByTheOtherInstaller;
struct LeavesTheBlocker;

impl FsRemoveDirAll for ReplacedByWhatBelongs {
    fn remove_dir_all(path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)?;
        fs::write(path, b"payload")?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveDirAll for RemovedByTheOtherInstaller {
    fn remove_dir_all(path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveDirAll for LeavesTheBlocker {
    fn remove_dir_all(_: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveNonDirDirent for ReplacedByWhatBelongs {
    fn remove_non_dir_dirent(path: &Path, _: fs::FileType) -> io::Result<()> {
        fs::remove_file(path)?;
        fs::create_dir(path)?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveNonDirDirent for RemovedByTheOtherInstaller {
    fn remove_non_dir_dirent(path: &Path, _: fs::FileType) -> io::Result<()> {
        fs::remove_file(path)?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveNonDirDirent for LeavesTheBlocker {
    fn remove_non_dir_dirent(_: &Path, _: fs::FileType) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

#[test]
fn a_directory_replaced_by_the_file_counts_as_cleared() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    clear_dir_blocking_file::<ReplacedByWhatBelongs>(&target)
        .expect("the path holds the file the clearing was for");
}

#[test]
fn a_directory_removed_by_the_other_installer_counts_as_cleared() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    clear_dir_blocking_file::<RemovedByTheOtherInstaller>(&target)
        .expect("nothing stands where the file belongs");
}

#[test]
fn a_directory_still_standing_fails_the_clearing() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    clear_dir_blocking_file::<LeavesTheBlocker>(&target)
        .expect_err("the blocker is still in the way");
}

#[test]
fn a_file_replaced_by_the_directory_counts_as_cleared() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("nested"), b"blocker").unwrap();

    clear_dirent_blocking_dir::<ReplacedByWhatBelongs>(root.path(), "nested")
        .expect("the path holds the directory the clearing was for");
}

#[test]
fn a_file_removed_by_the_other_installer_counts_as_cleared() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("nested"), b"blocker").unwrap();

    clear_dirent_blocking_dir::<RemovedByTheOtherInstaller>(root.path(), "nested")
        .expect("nothing stands where the directory belongs");
}

#[test]
fn a_file_still_standing_fails_the_clearing() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("nested"), b"blocker").unwrap();

    clear_dirent_blocking_dir::<LeavesTheBlocker>(root.path(), "nested")
        .expect_err("the blocker is still in the way");
}

#[test]
fn final_link_target_points_into_the_final_directory() {
    let imported = HashSet::new();
    let roots = SymlinkRoots {
        written_dir: Path::new("/stage"),
        final_dir: Path::new("/pkg"),
        imported: &imported,
    };
    assert_eq!(
        final_link_target(Path::new("/stage/lib/link"), Path::new("../sub"), roots),
        PathBuf::from("/pkg/sub"),
    );
    assert_eq!(
        final_link_target(Path::new("/stage/link"), Path::new("sub/dir"), roots),
        PathBuf::from("/pkg/sub/dir"),
    );
}
