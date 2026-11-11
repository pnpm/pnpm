//! Windows directory pinning for [`super::OpenDir`].
//!
//! Windows lacks descriptor-relative `openat`/`unlinkat` in `std::fs`. Holding
//! every opened directory component without `FILE_SHARE_DELETE` prevents
//! renaming, deleting, or replacing any validated ancestor while child
//! operations run against `OpenDir::path`.

use crate::{
    open_dir::{ChildFileStat, OpenDir},
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

pub(super) type PinnedChain = Vec<Arc<fs::File>>;

pub(super) fn open_root(path: &Path) -> io::Result<PinnedChain> {
    if !fs::metadata(path)?.is_dir() {
        return Err(io::Error::from(ErrorKind::NotADirectory));
    }
    let target = open_dir_handle(path, FILE_FLAG_BACKUP_SEMANTICS)?;
    if !target.metadata()?.is_dir() {
        return Err(io::Error::from(ErrorKind::NotADirectory));
    }
    let mut pinned = Vec::with_capacity(2);
    if let Ok(raw) =
        open_dir_handle(path, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        && raw.metadata().is_ok_and(|meta| is_reparse_point(&meta))
    {
        pinned.push(Arc::new(raw));
    }
    pinned.push(Arc::new(target));
    Ok(pinned)
}

pub(super) fn open_child_dir(
    parent_path: &Path,
    parent_pinned: &PinnedChain,
    name: &OsStr,
) -> io::Result<Option<(PathBuf, PinnedChain)>> {
    if !is_real_child_dir(parent_path, name)? {
        return Ok(None);
    }
    let child_path = parent_path.join(name);
    let handle = match open_dir_handle(
        &child_path,
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
    let mut pinned = parent_pinned.clone();
    pinned.push(Arc::new(handle));
    Ok(Some((child_path, pinned)))
}

pub(super) fn remove_child_dir(parent: &OpenDir, name: &OsStr) -> io::Result<bool> {
    let Some(child) = parent.open_child_dir(name)? else {
        return Ok(false);
    };
    empty_pinned_dir(&child)?;
    let child_path = child.path().to_path_buf();
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
            match remove_dirent(&dir.path().join(&entry_name)) {
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
