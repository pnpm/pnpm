//! Read a directory tree into an [`InodeMap`].

use super::{FileId, InodeMap, PatchError, Value};
use pnpm_directory_fetcher::DirectoryFetcher;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub(super) fn load_inode_map(
    dir: &Path,
    include_only_package_files: bool,
) -> Result<InodeMap, PatchError> {
    let files_map = list_files(dir, include_only_package_files)?;
    let mut inode_map = extend_files_map(&files_map)?;
    if include_only_package_files {
        add_files_under_listed_directories(dir, &files_map, &mut inode_map)?;
    }
    Ok(inode_map)
}

fn list_files(
    dir: &Path,
    include_only_package_files: bool,
) -> Result<HashMap<String, PathBuf>, PatchError> {
    DirectoryFetcher {
        directory: dir.to_path_buf(),
        include_only_package_files,
        resolve_symlinks: false,
        preserve_symlinks: false,
        allow_path_escape: false,
    }
    .list_files()
    .map_err(|error| PatchError::ReadDir { dir: dir.to_path_buf(), error })
}

/// The packlist names a symlinked directory as one entry, where the walk of
/// every file lists the files under it. Replace the entry with those files,
/// with links confined to `dir` as that walk confines them, so the copy gets
/// the same tree whichever walk read the source.
fn add_files_under_listed_directories(
    dir: &Path,
    files_map: &HashMap<String, PathBuf>,
    inode_map: &mut InodeMap,
) -> Result<(), PatchError> {
    let directories: Vec<&String> = files_map
        .keys()
        .filter(|path| inode_map.get(*path) == Some(&Value::Dir))
        .collect();
    for directory in directories {
        inode_map.remove(directory);
        let nested: HashMap<String, PathBuf> = list_all_files_within(dir, directory)?
            .into_iter()
            .map(|(path, real_path)| (format!("{directory}/{path}"), real_path))
            .collect();
        inode_map.extend(extend_files_map(&nested)?);
    }
    Ok(())
}

fn list_all_files_within(
    dir: &Path,
    directory: &str,
) -> Result<HashMap<String, PathBuf>, PatchError> {
    let directory = dir.join(directory);
    DirectoryFetcher {
        directory: directory.clone(),
        include_only_package_files: false,
        resolve_symlinks: false,
        preserve_symlinks: false,
        allow_path_escape: false,
    }
    .list_all_files_within(dir)
    .map_err(|error| PatchError::ReadDir { dir: directory, error })
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
        result => result
            .map(Some)
            .map_err(|error| PatchError::Stat { path: path.to_path_buf(), error }),
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
pub(super) fn file_id(path: &Path, metadata: &fs::Metadata) -> Result<FileId, PatchError> {
    let len = metadata.len();
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let _ = path;
        Ok(FileId { device: metadata.dev(), inode: metadata.ino(), len, modified })
    }
    #[cfg(windows)]
    {
        let (device, inode) = windows_file_index(path)
            .map_err(|error| PatchError::Stat { path: path.to_path_buf(), error })?;
        Ok(FileId { device, inode, len, modified })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(FileId { device: 0, inode: 0, len, modified })
    }
}

#[cfg(windows)]
fn windows_file_index(path: &Path) -> io::Result<(u64, u64)> {
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
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}
