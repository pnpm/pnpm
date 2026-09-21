use crate::{StoreDir, StoreIndex, StoreIndexError, decode_package_files_index};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

#[derive(Debug, Display, Error, Diagnostic)]
pub enum PruneCasError {
    #[display("Failed to read content-addressable store directory at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_READ_CAS_DIR))]
    ReadDir {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to inspect content-addressable store file at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_INSPECT_CAS_FILE))]
    InspectFile {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove unreferenced content-addressable store file at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_REMOVE_CAS_FILE))]
    RemoveFile {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove the temporary store directory at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_REMOVE_TMP))]
    RemoveTmp {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to update the package store index: {_0}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_PRUNE_STORE_INDEX))]
    StoreIndex(#[error(source)] StoreIndexError),
}

impl From<StoreIndexError> for PruneCasError {
    fn from(error: StoreIndexError) -> Self {
        Self::StoreIndex(error)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PruneCasStats {
    pub files: usize,
    pub bytes: u64,
    pub packages: usize,
}

pub(crate) fn prune_cas(store_dir: &StoreDir) -> Result<PruneCasStats, PruneCasError> {
    remove_tmp(store_dir)?;

    let mut stats = PruneCasStats::default();
    let mut removed_hashes = HashSet::new();
    for shard in read_entries(store_dir.files_dir())? {
        if !shard
            .file_type()
            .map_err(|error| PruneCasError::ReadDir { path: shard.path(), error })?
            .is_dir()
        {
            continue;
        }
        prune_shard(&shard.path(), &shard.file_name(), &mut removed_hashes, &mut stats)?;
    }

    let mut index = StoreIndex::open_in(store_dir).map_err(PruneCasError::StoreIndex)?;
    let mut rows_to_delete = Vec::new();
    index.for_each_raw(|key, data| {
        let package = decode_package_files_index(&data).map_err(PruneCasError::StoreIndex)?;
        if package.files
            .get("package.json")
            .is_some_and(|file| removed_hashes.contains(&file.digest))
        {
            rows_to_delete.push(key);
        }
        Ok::<_, PruneCasError>(())
    })?;
    stats.packages = rows_to_delete.len();
    index.delete_many(&rows_to_delete).map_err(PruneCasError::StoreIndex)?;
    Ok(stats)
}

fn prune_shard(
    shard_dir: &Path,
    shard_name: &OsStr,
    removed_hashes: &mut HashSet<String>,
    stats: &mut PruneCasStats,
) -> Result<(), PruneCasError> {
    for entry in read_entries(shard_dir)? {
        let path = entry.path();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(PruneCasError::InspectFile { path, error }),
        };
        if !metadata.is_file() || hard_link_count(&path, &metadata)? != 1 {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => {
                stats.files += 1;
                stats.bytes += metadata.len();
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(PruneCasError::RemoveFile { path, error }),
        }
        let shard = shard_name.to_string_lossy();
        let file = entry.file_name();
        let file = file.to_string_lossy();
        removed_hashes.insert(format!("{shard}{}", file.strip_suffix("-exec").unwrap_or(&file)));
    }
    Ok(())
}

fn read_entries(path: &Path) -> Result<Vec<fs::DirEntry>, PruneCasError> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(PruneCasError::ReadDir { path: path.to_path_buf(), error }),
    };
    entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PruneCasError::ReadDir { path: path.to_path_buf(), error })
}

fn remove_tmp(store_dir: &StoreDir) -> Result<(), PruneCasError> {
    let path = store_dir.tmp();
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PruneCasError::RemoveTmp { path, error }),
    }
}

#[cfg(unix)]
fn hard_link_count(_path: &Path, metadata: &fs::Metadata) -> Result<u64, PruneCasError> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(metadata.nlink())
}

#[cfg(windows)]
fn hard_link_count(path: &Path, _metadata: &fs::Metadata) -> Result<u64, PruneCasError> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = fs::File::open(path)
        .map_err(|error| PruneCasError::InspectFile { path: path.to_path_buf(), error })?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` owns a valid handle and `info` points to writable storage
    // of the structure initialized by `GetFileInformationByHandle`.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) } == 0 {
        return Err(PruneCasError::InspectFile {
            path: path.to_path_buf(),
            error: io::Error::last_os_error(),
        });
    }
    // SAFETY: a successful `GetFileInformationByHandle` initializes `info`.
    let info = unsafe { info.assume_init() };
    Ok(u64::from(info.nNumberOfLinks))
}

#[cfg(not(any(unix, windows)))]
fn hard_link_count(_path: &Path, _metadata: &fs::Metadata) -> Result<u64, PruneCasError> {
    Ok(2)
}

#[cfg(test)]
mod tests;
