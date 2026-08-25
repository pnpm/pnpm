use pipe_trait::Pipe;
use pnpm_workspace_state::load_workspace_state;
use std::{
    fs, io,
    path::Path,
    time::{Duration, SystemTime},
};
use walkdir::WalkDir;

#[must_use]
pub fn get_filenames_in_folder(path: &Path) -> Vec<String> {
    let mut files = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    files.sort();
    files
}

fn normalized_suffix(path: &Path, prefix: &Path) -> String {
    path.strip_prefix(prefix)
        .expect("strip prefix from path")
        .to_str()
        .expect("convert suffix to UTF-8")
        .replace('\\', "/")
}

#[must_use]
pub fn get_all_folders(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.expect("access entry"))
        .filter(|entry| entry.file_type().is_dir() || entry.file_type().is_symlink())
        .map(|entry| normalized_suffix(entry.path(), root))
        .filter(|suffix| !suffix.is_empty())
        .collect()
}

#[must_use]
pub fn get_all_files(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.expect("access entry"))
        .filter(|entry| !entry.file_type().is_dir())
        .map(|entry| normalized_suffix(entry.path(), root))
        .filter(|suffix| !suffix.is_empty())
        .collect()
}

pub fn is_symlink_or_junction(path: &Path) -> io::Result<bool> {
    pnpm_fs::is_symlink_or_junction(path)
}

/// Check if a file is executable.
#[cfg(unix)]
#[must_use]
pub fn is_path_executable(path: &Path) -> bool {
    use std::{fs::File, os::unix::prelude::*};
    let mode = File::open(path)
        .expect("open the file")
        .metadata()
        .expect("get metadata of the file")
        .mode();
    mode & 0b001_001_001 != 0
}

/// The gap that separates two mtimes on every filesystem the tests run on.
///
/// A full second, because the coarsest supported filesystems (HFS+, ext4
/// with 128-byte inodes, some CI runner disks) keep no sub-second component
/// at all, and the freshness check treats such an mtime as covering its
/// whole second.
pub const MTIME_STEP_MS: i64 = 1_000;

/// Milliseconds since the Unix epoch of `path`'s mtime, matching how the
/// workspace state records `lastValidatedTimestamp`.
#[must_use]
pub fn mtime_ms(path: &Path) -> i64 {
    let modified = path
        .pipe(fs::metadata)
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|error| panic!("stat {path:?}: {error}"));
    modified.duration_since(SystemTime::UNIX_EPOCH).map_or_else(
        |error| panic!("mtime of {path:?} predates the Unix epoch: {error}"),
        |elapsed| {
            let millis = elapsed.as_millis();
            millis.pipe(i64::try_from).unwrap_or_else(|_| {
                panic!("mtime of {path:?} is {millis} ms past the epoch, beyond an i64 timestamp")
            })
        },
    )
}

/// Set `path`'s mtime to `ms` milliseconds since the Unix epoch.
///
/// A filesystem that keeps only whole-second mtimes rounds the stored value
/// down, so callers must leave at least [`MTIME_STEP_MS`] of headroom rather
/// than assume the exact value survives.
pub fn set_mtime_ms(path: &Path, ms: i64) {
    let ms = u64::try_from(ms).expect("an mtime at or after the Unix epoch");
    set_mtime(path, SystemTime::UNIX_EPOCH + Duration::from_millis(ms));
}

/// Set `path`'s mtime to `modified`, keeping whatever sub-millisecond
/// precision the filesystem stores — the granularity [`set_mtime_ms`]
/// cannot express, and the one a same-millisecond mtime collision needs.
pub fn set_mtime(path: &Path, modified: SystemTime) {
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(modified)))
        .unwrap_or_else(|error| panic!("set the mtime of {path:?}: {error}"));
}

/// Rewrite the mtime of every regular file under `root` to a point in the
/// past, and return the `lastValidatedTimestamp` that matches it.
///
/// The returned value is only meaningful for a tree this call just
/// backdated. Record it as the workspace state's timestamp and the backdated
/// files read as validated, while anything written afterwards reads as
/// modified, with no sleeping in between.
///
/// Both gaps are [`MTIME_STEP_MS`] wide, and the value comes from the newest
/// mtime in the tree, so it shares a clock with every file the freshness
/// check compares it against.
///
/// Panics when `root` holds no regular file to take that mtime from.
#[must_use]
pub fn backdate_existing_files(root: &Path) -> i64 {
    // Only regular files: the freshness check stats manifests, lockfiles,
    // patches, and pnpmfiles, and `set_times` cannot open a directory.
    let files: Vec<_> = root
        .pipe(WalkDir::new)
        .into_iter()
        .map(|entry| entry.expect("access entry"))
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect();
    let latest = files
        .iter()
        .map(|path| mtime_ms(path))
        .max()
        .unwrap_or_else(|| panic!("no file to date the validation from under {root:?}"));
    for path in &files {
        set_mtime_ms(path, latest - 2 * MTIME_STEP_MS);
    }
    latest - MTIME_STEP_MS
}

/// Push `path`'s mtime past the `lastValidatedTimestamp` the last install
/// recorded, so the optimistic-repeat-install fast path sees the rewrite
/// instead of short-circuiting. Call this after any post-install manifest or
/// lockfile rewrite.
///
/// The new mtime is [`MTIME_STEP_MS`] past whichever is later, the recorded
/// timestamp or `path`'s own mtime. An earlier bump can leave the recorded
/// timestamp ahead of the present, so it is the reference even when it
/// post-dates the file.
///
/// Panics when no install has recorded a `lastValidatedTimestamp` above
/// `path`.
pub fn bump_mtime(path: &Path) {
    let baseline = recorded_validation_timestamp(path);
    set_mtime_ms(path, baseline.max(mtime_ms(path)) + MTIME_STEP_MS);
}

/// `lastValidatedTimestamp` of the workspace state governing `path`, found
/// by walking up from its directory.
///
/// Panics when no install has written one: [`bump_mtime`] then has no
/// baseline to push past, and a test that carried on would assert against a
/// freshness verdict reached for the wrong reason.
fn recorded_validation_timestamp(path: &Path) -> i64 {
    path.ancestors()
        .skip(1)
        .find_map(|dir| load_workspace_state(dir).expect("read the workspace state"))
        .unwrap_or_else(|| panic!("no workspace state above {path:?} to bump the mtime past"))
        .last_validated_timestamp
}
