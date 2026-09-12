//! Bring one directory tree in step with another by hardlinking, so an
//! injected copy of a workspace package can be refreshed in place
//! without re-running the installer.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_directory_fetcher::{DirectoryFetcher, DirectoryFetcherError};
use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

/// A file's identity. An inode number is only unique within one
/// filesystem, so the volume it came from is part of the identity:
/// without it two unrelated files on different volumes can collide and
/// be mistaken for the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileId {
    pub device: u64,
    pub inode: u64,
}

/// What a path in an [`InodeMap`] holds.
///
/// A file carries its identity rather than its content, because that is
/// all a hardlink comparison needs: two paths hold the same bytes
/// exactly when they are the same file, so an unchanged file costs no
/// filesystem work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Dir,
    File(FileId),
}

/// Relative path → inode type, for every file and directory in a tree.
///
/// Ordering matters and comes for free: a directory's path is a strict
/// prefix of every path beneath it, so an ancestor always sorts before
/// its descendants. [`apply_patch`] leans on that to put a directory in
/// place before what it holds, and to delete children before parents.
pub type InodeMap = BTreeMap<String, Value>;

/// A path the target must end up holding, paired with what it holds
/// now. `old_value` is `None` when the target has nothing there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub old_value: Option<Value>,
    pub new_value: Value,
}

/// The work needed to turn a target tree into a copy of a source tree.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DirDiff {
    pub changes: Vec<Change>,
    /// Paths the target holds that the source does not, deepest first
    /// so a directory is empty by the time it is removed.
    pub removed: Vec<String>,
}

/// Error type for [`DirPatcher`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PatchError {
    #[display("Failed to read the directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_READ_DIR))]
    ReadDir {
        dir: PathBuf,
        #[error(source)]
        error: DirectoryFetcherError,
    },

    #[display("Failed to stat {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_STAT))]
    Stat {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to create the directory at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_CREATE_DIR))]
    CreateDir {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to hardlink {source:?} to {target:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_LINK))]
    Link {
        source: PathBuf,
        target: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_INJECTED_DEPS_SYNC_REMOVE))]
    Remove {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// One source directory paired with one target that must mirror it.
pub struct DirPatcher {
    source_dir: PathBuf,
    target_dir: PathBuf,
    patch: DirDiff,
}

impl DirPatcher {
    /// Diff `source_dir` against each of `target_dirs`, reading each
    /// tree once.
    pub fn from_multiple_targets(
        source_dir: &Path,
        target_dirs: &[PathBuf],
    ) -> Result<Vec<Self>, PatchError> {
        let source_map = load_inode_map(source_dir)?;
        target_dirs
            .iter()
            .map(|target_dir| {
                Ok(DirPatcher {
                    source_dir: source_dir.to_path_buf(),
                    target_dir: target_dir.clone(),
                    patch: diff_dir(&load_inode_map(target_dir)?, &source_map),
                })
            })
            .collect()
    }

    pub fn apply(&self) -> Result<(), PatchError> {
        apply_patch(&self.patch, &self.source_dir, &self.target_dir)
    }
}

/// The difference between two trees: what `new_index` has that
/// `old_index` does not or holds differently, and what only
/// `old_index` has.
#[must_use]
pub fn diff_dir(old_index: &InodeMap, new_index: &InodeMap) -> DirDiff {
    let changes = new_index
        .iter()
        .filter(|(path, new_value)| old_index.get(*path) != Some(*new_value))
        .map(|(path, new_value)| Change {
            path: path.clone(),
            old_value: old_index.get(path).copied(),
            new_value: *new_value,
        })
        .collect();
    let removed =
        old_index.keys().filter(|path| !new_index.contains_key(*path)).rev().cloned().collect();
    DirDiff { changes, removed }
}

/// Apply a diff produced by [`diff_dir`] to `target_dir`.
///
/// The phase order is load-bearing: removals before changes so that a
/// path the source turned into a file still has its old children under
/// a directory, and directories before files so that displacing a
/// blocking inode never takes a populated directory with it.
pub fn apply_patch(
    patch: &DirDiff,
    source_dir: &Path,
    target_dir: &Path,
) -> Result<(), PatchError> {
    for path in &patch.removed {
        remove_recursive(&target_dir.join(path))?;
    }
    let (new_dirs, new_files): (Vec<_>, Vec<_>) =
        patch.changes.iter().partition(|change| change.new_value == Value::Dir);
    for change in new_dirs.into_iter().chain(new_files) {
        apply_change(change, source_dir, target_dir)?;
    }
    Ok(())
}

fn apply_change(change: &Change, source_dir: &Path, target_dir: &Path) -> Result<(), PatchError> {
    let target_path = target_dir.join(&change.path);
    if change.old_value.is_some() {
        remove_recursive(&target_path)?;
    }
    match change.new_value {
        Value::Dir => retry_over_blocking_inode(&target_path, || {
            fs::create_dir_all(&target_path)
                .map_err(|error| PatchError::CreateDir { path: target_path.clone(), error })
        }),
        Value::File(_) => {
            let source_path = source_dir.join(&change.path);
            retry_over_blocking_inode(&target_path, || {
                fs::hard_link(&source_path, &target_path).map_err(|error| PatchError::Link {
                    source: source_path.clone(),
                    target: target_path.clone(),
                    error,
                })
            })
        }
    }
}

/// The target may hold an inode that [`extend_files_map`] skips — a
/// FIFO, a socket, a device. No diff can see it, so it is never
/// scheduled for removal, and creating over it fails with `EEXIST`.
/// Clear that path and retry once instead of failing the sync.
fn retry_over_blocking_inode(
    target_path: &Path,
    add: impl Fn() -> Result<(), PatchError>,
) -> Result<(), PatchError> {
    match add() {
        Err(error) if is_already_exists(&error) => {
            remove_recursive(target_path)?;
            add()
        }
        result => result,
    }
}

fn is_already_exists(error: &PatchError) -> bool {
    matches!(
        error,
        PatchError::CreateDir { error, .. } | PatchError::Link { error, .. }
            if error.kind() == io::ErrorKind::AlreadyExists,
    )
}

fn remove_recursive(target_path: &Path) -> Result<(), PatchError> {
    match pnpm_fs::remove_dirent(target_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => {
            result.map_err(|error| PatchError::Remove { path: target_path.to_path_buf(), error })
        }
    }
}

fn load_inode_map(dir: &Path) -> Result<InodeMap, PatchError> {
    let output = DirectoryFetcher {
        directory: dir.to_path_buf(),
        include_only_package_files: false,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .map_err(|error| PatchError::ReadDir { dir: dir.to_path_buf(), error })?;
    extend_files_map(&output.files_map)
}

/// Expand a relative-path → real-path map into an [`InodeMap`] that
/// also names every ancestor directory.
///
/// An inode that is neither a file nor a directory — a FIFO, a socket,
/// a device — cannot be hardlinked into the injected copy, so it is
/// left out of the map.
pub fn extend_files_map(files_map: &HashMap<String, PathBuf>) -> Result<InodeMap, PatchError> {
    let mut result = InodeMap::from([(".".to_string(), Value::Dir)]);
    for (relative_path, real_path) in files_map {
        let Some(metadata) = stat_skipping_missing(real_path)? else {
            continue;
        };
        let value = if metadata.is_file() {
            Value::File(file_id(real_path, &metadata)?)
        } else if metadata.is_dir() {
            Value::Dir
        } else {
            continue;
        };
        add_inode_and_ancestors(&mut result, relative_path, value);
    }
    Ok(result)
}

/// A path the walker listed can be gone by the time it is stat'd — a
/// build script that deletes as it goes, or a symlink that broke.
fn stat_skipping_missing(path: &Path) -> Result<Option<fs::Metadata>, PatchError> {
    match fs::metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        result => {
            result.map(Some).map_err(|error| PatchError::Stat { path: path.to_path_buf(), error })
        }
    }
}

fn add_inode_and_ancestors(result: &mut InodeMap, relative_path: &str, value: Value) {
    let mut path = relative_path;
    let mut value = value;
    while !path.is_empty() && path != "." && !result.contains_key(path) {
        result.insert(path.to_string(), value);
        path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        value = Value::Dir;
    }
}

/// Read a file's [`FileId`].
///
/// Windows has no stable way to read the file index from
/// [`fs::Metadata`] — `MetadataExt::file_index` is still unstable — so
/// it comes from a handle instead, the same call libuv makes to fill
/// Node's `Stats.ino`.
fn file_id(path: &Path, metadata: &fs::Metadata) -> Result<FileId, PatchError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let _ = path;
        Ok(FileId { device: metadata.dev(), inode: metadata.ino() })
    }
    #[cfg(windows)]
    {
        let _ = metadata;
        windows_file_id(path).map_err(|error| PatchError::Stat { path: path.to_path_buf(), error })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, metadata);
        Ok(FileId { device: 0, inode: 0 })
    }
}

#[cfg(windows)]
fn windows_file_id(path: &Path) -> io::Result<FileId> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = fs::File::open(path)?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` owns a valid handle for this call and `info` points to
    // writable storage of the exact structure the API initializes.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful `GetFileInformationByHandle` initializes `info`.
    let info = unsafe { info.assume_init() };
    Ok(FileId {
        device: u64::from(info.dwVolumeSerialNumber),
        inode: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

#[cfg(test)]
mod tests;
