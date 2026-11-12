//! Windows directory pinning for [`super::OpenDir`].
//!
//! Windows lacks descriptor-relative `openat`/`unlinkat` in `std::fs`. Resolving
//! the root directory's canonical path and holding the target handle (plus every
//! opened child directory) without `FILE_SHARE_DELETE` prevents renaming,
//! deleting, or replacing any real ancestor while child operations run against
//! the pinned path.

use crate::{
    open_dir::{ChildFileStat, ChildWrite, OpenDir, write_child_file_in_pinned_dir},
    remove_dirent,
    retry::retry_transient_removal_locks,
};
use std::{
    ffi::OsStr,
    fs,
    io::{self, ErrorKind},
    os::windows::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

#[derive(Debug, Clone)]
pub(super) struct PinnedChain {
    pinned_path: PathBuf,
    handles: Vec<Arc<fs::File>>,
}

impl PinnedChain {
    pub(super) fn path(&self) -> &Path {
        &self.pinned_path
    }
}

pub(super) fn open_root(path: &Path) -> io::Result<PinnedChain> {
    if !fs::metadata(path)?.is_dir() {
        return Err(io::Error::from(ErrorKind::NotADirectory));
    }
    let raw = open_dir_handle(path, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)?;
    let raw_meta = raw.metadata()?;
    let pinned_path = dunce::canonicalize(path)?;
    let target =
        open_dir_handle(&pinned_path, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)?;
    let target_meta = target.metadata()?;
    if !target_meta.is_dir() || is_reparse_point(&target_meta) {
        return Err(io::Error::from(ErrorKind::NotADirectory));
    }
    if dunce::canonicalize(&pinned_path)? != pinned_path {
        return Err(io::Error::other("directory path changed during open"));
    }
    let mut handles = Vec::with_capacity(2);
    if is_reparse_point(&raw_meta) {
        handles.push(Arc::new(raw));
    }
    handles.push(Arc::new(target));
    Ok(PinnedChain { pinned_path, handles })
}

pub(super) fn open_child_dir(
    parent_path: &Path,
    parent_pinned: &PinnedChain,
    name: &OsStr,
) -> io::Result<Option<(PathBuf, PinnedChain)>> {
    if !is_real_child_dir(parent_pinned.path(), name)? {
        return Ok(None);
    }
    let child_pinned_path = parent_pinned.path().join(name);
    let handle = match open_dir_handle(
        &child_pinned_path,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    ) {
        Ok(handle) => handle,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let meta = handle.metadata()?;
    if !meta.is_dir() || is_reparse_point(&meta) {
        return Ok(None);
    }
    let mut handles = parent_pinned.handles.clone();
    handles.push(Arc::new(handle));
    Ok(Some((parent_path.join(name), PinnedChain { pinned_path: child_pinned_path, handles })))
}

pub(super) fn remove_child_dir(parent: &OpenDir, name: &OsStr) -> io::Result<bool> {
    let Some(child) = parent.open_child_dir(name)? else {
        return Ok(false);
    };
    empty_pinned_dir(&child)?;
    let child_path = child.pinned.path().to_path_buf();
    drop(child);
    match retry_transient_removal_locks(|| fs::remove_dir(&child_path)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn empty_pinned_dir(dir: &OpenDir) -> io::Result<()> {
    for entry_name in dir.entry_names()? {
        if !remove_child_dir(dir, &entry_name)? {
            match remove_dirent(&dir.pinned.path().join(&entry_name)) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

pub(super) fn stat_child_file(
    parent_path: &Path,
    name: &OsStr,
) -> io::Result<Option<ChildFileStat>> {
    let metadata = match fs::symlink_metadata(parent_path.join(name)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() || is_reparse_point(&metadata) {
        return Ok(None);
    }
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let accessed = metadata.accessed().unwrap_or(modified);
    Ok(Some(ChildFileStat { len: metadata.len(), modified, accessed }))
}

pub(super) fn read_child_file(
    parent_pinned: &PinnedChain,
    name: &OsStr,
) -> io::Result<Option<Vec<u8>>> {
    if stat_child_file(parent_pinned.path(), name)?.is_none() {
        return Ok(None);
    }
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(parent_pinned.path().join(name))
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || is_reparse_point(&metadata) {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut file, &mut bytes)?;
    Ok(Some(bytes))
}

pub(super) fn remove_child_file(parent_pinned: &PinnedChain, name: &OsStr) -> io::Result<bool> {
    if stat_child_file(parent_pinned.path(), name)?.is_none() {
        return Ok(false);
    }
    match fs::remove_file(parent_pinned.path().join(name)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn remove_empty_child_dir(
    parent_pinned: &PinnedChain,
    name: &OsStr,
) -> io::Result<bool> {
    if !is_real_child_dir(parent_pinned.path(), name)? {
        return Ok(false);
    }
    match fs::remove_dir(parent_pinned.path().join(name)) {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn write_child_file(
    parent_pinned: &PinnedChain,
    name: &OsStr,
    bytes: &[u8],
    mode: ChildWrite,
) -> io::Result<()> {
    if matches!(mode, ChildWrite::Replace) && is_reparse_child(parent_pinned.path(), name)? {
        return Err(io::Error::new(ErrorKind::InvalidInput, "refusing to replace reparse child"));
    }
    write_child_file_in_pinned_dir(parent_pinned.path(), name, bytes, mode, || {
        is_reparse_child(parent_pinned.path(), name)
    })
}

pub(super) fn is_reparse_child(parent_path: &Path, name: &OsStr) -> io::Result<bool> {
    match fs::symlink_metadata(parent_path.join(name)) {
        Ok(metadata) => Ok(metadata.file_type().is_symlink() || is_reparse_point(&metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn is_real_child_dir(parent_path: &Path, name: &OsStr) -> io::Result<bool> {
    match fs::symlink_metadata(parent_path.join(name)) {
        Ok(metadata) => Ok(metadata.is_dir() && !is_reparse_point(&metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn open_dir_handle(path: &Path, custom_flags: u32) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(custom_flags)
        .open(path)
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
