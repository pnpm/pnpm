//! Read a directory tree into an [`InodeMap`].

use super::{FileId, InodeMap, PatchError, Value};
use pnpm_directory_fetcher::DirectoryFetcher;
use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub(super) fn load_inode_map(
    dir: &Path,
    include_only_package_files: bool,
) -> Result<InodeMap, PatchError> {
    let files_map = list_files(dir, include_only_package_files)?;
    let listed = listed_paths(files_map.keys());
    let mut inode_map = InodeMap::from([(".".to_string(), Value::Dir)]);
    for (relative_path, path) in &files_map {
        let Some(metadata) = stat_skipping_missing(path, |path| fs::symlink_metadata(path))? else {
            continue;
        };
        let value = if metadata.file_type().is_symlink() {
            link_value(dir, path, &listed)?
        } else {
            value_of(path, &metadata)?
        };
        if let Some(value) = value {
            add_inode_and_ancestors(&mut inode_map, relative_path, value);
        }
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
        preserve_symlinks: true,
        allow_path_escape: false,
    }
    .list_files()
    .map_err(|error| PatchError::ReadDir { dir: dir.to_path_buf(), error })
}

/// Every listed path and each directory above it, the root included as `""`.
fn listed_paths<'a>(paths: impl Iterator<Item = &'a String>) -> HashSet<&'a str> {
    let mut listed = HashSet::from([""]);
    for entry in paths {
        let mut path = entry.as_str();
        while !path.is_empty() && listed.insert(path) {
            path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    listed
}

/// What the install writes for the link at `path`, as the importer decides
/// it: the link itself when it points at a listed path, the file it points
/// at when that file is not listed, and nothing for an unlisted directory.
fn link_value(
    dir: &Path,
    path: &Path,
    listed: &HashSet<&str>,
) -> Result<Option<Value>, PatchError> {
    let Some(destination) = link_destination(dir, path)? else {
        return Ok(None);
    };
    if listed.contains(destination.as_str()) {
        let mut hasher = DefaultHasher::new();
        destination.hash(&mut hasher);
        return Ok(Some(Value::Symlink(hasher.finish())));
    }
    match stat_skipping_missing(path, |path| fs::metadata(path))? {
        Some(metadata) if metadata.is_file() => Ok(Some(Value::File(file_id(path, &metadata)?))),
        _ => Ok(None),
    }
}

/// Where the link at `path` points, as a `/`-separated path relative to
/// `dir`, or `None` when it points outside `dir`.
fn link_destination(dir: &Path, path: &Path) -> Result<Option<String>, PatchError> {
    let stat_error = |error| PatchError::Stat { path: path.to_path_buf(), error };
    let link = pnpm_fs::read_symlink_dir(path).map_err(stat_error)?;
    let link = pnpm_fs::relative_link_target(path, link);
    let root = fs::canonicalize(dir).map_err(stat_error)?;
    let link_dir = path
        .parent()
        .map_or_else(|| Ok(root.clone()), fs::canonicalize)
        .map_err(stat_error)?;
    let destination = pnpm_fs::lexical_normalize(&link_dir.join(link));
    Ok(destination
        .strip_prefix(&root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/")))
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
        let Some(metadata) = stat_skipping_missing(real_path, |path| fs::metadata(path))? else {
            continue;
        };
        if let Some(value) = value_of(real_path, &metadata)? {
            add_inode_and_ancestors(&mut result, relative_path, value);
        }
    }
    Ok(result)
}

/// A file or a directory. Anything else is left out, see
/// [`extend_files_map`].
fn value_of(path: &Path, metadata: &fs::Metadata) -> Result<Option<Value>, PatchError> {
    if metadata.is_file() {
        Ok(Some(Value::File(file_id(path, metadata)?)))
    } else if metadata.is_dir() {
        Ok(Some(Value::Dir))
    } else {
        Ok(None)
    }
}

/// A path the walker listed can be gone by the time it is stat'd — a
/// build script that deletes as it goes, or a symlink that broke.
fn stat_skipping_missing(
    path: &Path,
    stat: fn(&Path) -> io::Result<fs::Metadata>,
) -> Result<Option<fs::Metadata>, PatchError> {
    match stat(path) {
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
