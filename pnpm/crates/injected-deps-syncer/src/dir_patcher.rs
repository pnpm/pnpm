//! Bring one directory tree in step with another by hardlinking, so an
//! injected copy of a workspace package can be refreshed in place
//! without re-running the installer. A target on another filesystem
//! than its source gets copies instead.

pub use inode_map::extend_files_map;

use derive_more::{Display, Error};
use inode_map::load_inode_map;
use miette::Diagnostic;
use pnpm_directory_fetcher::DirectoryFetcherError;
use pnpm_fs::{FsHardLink, Host, is_cross_device};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// A file's identity. An inode number is only unique within one
/// filesystem, so the volume it came from is part of the identity:
/// without it two unrelated files on different volumes can collide and
/// be mistaken for the same one. Length and modification time travel
/// with that identity so a watcher publish can tell an in-place edit
/// of a hardlink from a copy that already matches the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileId {
    pub device: u64,
    pub inode: u64,
    pub len: u64,
    pub modified: SystemTime,
}

/// What a path in an [`InodeMap`] holds.
///
/// A file carries its identity rather than its content, because that is
/// all a hardlink comparison needs: two paths hold the same bytes
/// exactly when they are the same file, so an unchanged file costs no
/// filesystem work. A copy made because the target is on another
/// filesystem never shares its source's identity, so every sync copies
/// it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Dir,
    File(FileId),
    /// A link to something inside the tree, by a hash of the path it points
    /// at relative to the tree's root. The source and a copy hold the same
    /// link exactly when their links point at the same relative path,
    /// whatever the link text, so a Windows junction's absolute target
    /// matches the relative one it was made from.
    Symlink(u64),
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

    #[display("Failed to hardlink or copy {source:?} to {target:?}: {error}")]
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
    /// tree once. `include_only_package_files` reads the source the way
    /// [`pnpm_directory_fetcher::DirectoryFetcher::include_only_package_files`] does.
    pub fn from_multiple_targets(
        source_dir: &Path,
        target_dirs: &[PathBuf],
        include_only_package_files: bool,
    ) -> Result<Vec<Self>, PatchError> {
        let source_map = load_inode_map(source_dir, include_only_package_files)?;
        target_dirs
            .iter()
            .map(|target_dir| {
                Ok(DirPatcher {
                    source_dir: source_dir.to_path_buf(),
                    target_dir: target_dir.clone(),
                    patch: diff_dir(&load_inode_map(target_dir, false)?, &source_map),
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
    let removed = old_index
        .keys()
        .filter(|path| !new_index.contains_key(*path))
        .rev()
        .cloned()
        .collect();
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
    apply_patch_with_link::<Host>(patch, source_dir, target_dir)
}

pub(crate) fn apply_patch_with_link<Sys: FsHardLink>(
    patch: &DirDiff,
    source_dir: &Path,
    target_dir: &Path,
) -> Result<(), PatchError> {
    for path in &patch.removed {
        remove_recursive(&target_dir.join(path))?;
    }
    let (new_dirs, new_files): (Vec<_>, Vec<_>) = patch.changes
        .iter()
        .partition(|change| change.new_value == Value::Dir);
    for change in new_dirs.into_iter().chain(new_files) {
        apply_change_with_link::<Sys>(change, source_dir, target_dir)?;
    }
    Ok(())
}

fn apply_change_with_link<Sys: FsHardLink>(
    change: &Change,
    source_dir: &Path,
    target_dir: &Path,
) -> Result<(), PatchError> {
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
            let source_path = linked_file(&source_dir.join(&change.path));
            retry_over_blocking_inode(&target_path, || {
                link_or_copy::<Sys>(&source_path, &target_path)
                    .map_err(|error| PatchError::Link {
                        source: source_path.clone(),
                        target: target_path.clone(),
                        error,
                    })
            })
        }
        Value::Symlink(_) => {
            let source_path = source_dir.join(&change.path);
            retry_over_blocking_inode(&target_path, || place_link(&source_path, &target_path))
        }
    }
}

/// The file at `path`, or the file it links to: a link the install imports as
/// a file is a file in the copy, and a hardlink of the link would be another
/// link, one the copy cannot resolve.
fn linked_file(path: &Path) -> PathBuf {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

/// Recreate the link at `source_path` at `target_path`, the way the install
/// recreates a package's link to its own content.
fn place_link(source_path: &Path, target_path: &Path) -> Result<(), PatchError> {
    let link_error = |error| PatchError::Link {
        source: source_path.to_path_buf(),
        target: target_path.to_path_buf(),
        error,
    };
    let contents = pnpm_fs::read_symlink_dir(source_path)
        .map(|link| pnpm_fs::relative_link_target(source_path, link))
        .map_err(link_error)?;
    let link_dir = target_path.parent().unwrap_or_else(|| Path::new(""));
    fs::create_dir_all(link_dir)
        .map_err(|error| PatchError::CreateDir { path: link_dir.to_path_buf(), error })?;
    if fs::metadata(source_path).is_ok_and(|metadata| metadata.is_dir()) {
        let original = pnpm_fs::lexical_normalize(&link_dir.join(&contents));
        pnpm_fs::symlink_dir_with_contents(&original, &contents, target_path)
    } else {
        pnpm_fs::symlink_file_or_copy(&contents, source_path, target_path)
    }
    .map_err(link_error)
}

/// A source directory's files, read once per poll and published to each of
/// its injected copies.
pub struct PublishSource<'a> {
    pub dir: &'a Path,
    pub map: InodeMap,
}

impl<'a> PublishSource<'a> {
    /// `include_only_package_files` reads `dir` the way
    /// [`pnpm_directory_fetcher::DirectoryFetcher::include_only_package_files`] does.
    pub fn load(dir: &'a Path, include_only_package_files: bool) -> Result<Self, PatchError> {
        Ok(PublishSource { dir, map: load_inode_map(dir, include_only_package_files)? })
    }
}

/// Copy changed files from `source.dir` into `target_dir` as independent
/// files, so a watcher on `target_dir` sees the write.
///
/// Hardlink sync shares inodes, and an in-place write through the source
/// path does not notify a watcher on the injected directory. A plain copy
/// followed by a rename does. `edited_since` is fixed for one watch: a
/// hardlink is republished once when its modification time is at least
/// that instant, and a copy whose length and modification time already
/// match the source is left alone.
pub fn publish_edits(
    source: &PublishSource<'_>,
    target_dir: &Path,
    edited_since: SystemTime,
) -> Result<(), PatchError> {
    let PublishSource { dir: source_dir, map: source_map } = source;
    let target_map = load_inode_map(target_dir, false)?;
    let patch = diff_dir(&target_map, source_map);
    for path in &patch.removed {
        remove_recursive(&target_dir.join(path))?;
    }
    for (path, value) in source_map {
        if *value != Value::Dir || path == "." {
            continue;
        }
        ensure_directory(&target_dir.join(path), target_map.get(path))?;
    }
    publish_links(source, target_dir, &target_map)?;
    for (path, value) in source_map {
        let Value::File(source_id) = value else {
            continue;
        };
        if !should_publish(source_id, target_map.get(path), edited_since) {
            continue;
        }
        publish_file(&source_dir.join(path), &target_dir.join(path), source_id.modified)?;
    }
    Ok(())
}

/// Recreate each link of `source` that `target_map` does not hold.
fn publish_links(
    source: &PublishSource<'_>,
    target_dir: &Path,
    target_map: &InodeMap,
) -> Result<(), PatchError> {
    for (path, value) in &source.map {
        if !matches!(value, Value::Symlink(_)) || target_map.get(path) == Some(value) {
            continue;
        }
        let target_path = target_dir.join(path);
        remove_recursive(&target_path)?;
        place_link(&source.dir.join(path), &target_path)?;
    }
    Ok(())
}

fn should_publish(source: &FileId, target: Option<&Value>, edited_since: SystemTime) -> bool {
    let Some(Value::File(target_id)) = target else {
        return true;
    };
    if source.device == target_id.device && source.inode == target_id.inode {
        return source.modified >= edited_since;
    }
    source.len != target_id.len || source.modified != target_id.modified
}

fn ensure_directory(target_path: &Path, old: Option<&Value>) -> Result<(), PatchError> {
    if old.is_some_and(|value| *value != Value::Dir) {
        remove_recursive(target_path)?;
    }
    fs::create_dir_all(target_path)
        .map_err(|error| PatchError::CreateDir { path: target_path.to_path_buf(), error })
}

fn publish_file(
    source_path: &Path,
    target_path: &Path,
    modified: SystemTime,
) -> Result<(), PatchError> {
    if let Some(parent) = target_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| PatchError::CreateDir { path: parent.to_path_buf(), error })?;
    }
    if fs::symlink_metadata(target_path).is_ok_and(|metadata| metadata.is_dir()) {
        remove_recursive(target_path)?;
    }
    pnpm_fs::copy_file_atomic(source_path, target_path)
        .map_err(|error| PatchError::Link {
            source: source_path.to_path_buf(),
            target: target_path.to_path_buf(),
            error,
        })?;
    if let Err(error) = fs::File::options()
        .write(true)
        .open(target_path)
        .and_then(|file| file.set_modified(modified))
    {
        tracing::debug!(
            target: "pacquet::sync_injected_deps",
            path = ?target_path,
            "Failed to preserve the modification time of a published injected file: {error}",
        );
    }
    Ok(())
}

fn link_or_copy<Sys: FsHardLink>(source_path: &Path, target_path: &Path) -> io::Result<()> {
    match Sys::hard_link(source_path, target_path) {
        Err(error) if is_cross_device(&error) => {
            pnpm_fs::copy_file_atomic(source_path, target_path)
        }
        result => result,
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

mod inode_map;
#[cfg(test)]
mod tests;
