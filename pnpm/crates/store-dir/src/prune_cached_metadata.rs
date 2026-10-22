use crate::{PruneError, StoreDir};
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

impl StoreDir {
    /// Remove the registry metadata cached under `cache_dir`: every
    /// directory directly under it whose name starts with `metadata` or is a
    /// cache layout version such as `v12`.
    ///
    /// A symbolic link is not a directory here, so it is kept. A directory
    /// that holds this store is kept too, for a `cacheDir` that encloses the
    /// store.
    ///
    /// The caller holds the store's prune lock, which keeps out the installs
    /// that read the cache.
    pub(crate) fn remove_cached_metadata(&self, cache_dir: &Path) -> Result<(), PruneError> {
        let store_root = dunce::canonicalize(self.root()).unwrap_or_else(|_| self.root().into());
        for path in cached_metadata_dirs(cache_dir)? {
            if dunce::canonicalize(&path).is_ok_and(|path| store_root.starts_with(path)) {
                continue;
            }
            match pnpm_fs::remove_dirent(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(PruneError::RemoveCachedMetadata { path, error }),
            }
        }
        pnpm_diagnostics::eprintln!("Removed all cached metadata files");
        Ok(())
    }
}

fn cached_metadata_dirs(cache_dir: &Path) -> Result<Vec<PathBuf>, PruneError> {
    let read_error = |error| PruneError::ReadCacheDir { path: cache_dir.to_path_buf(), error };
    let entries = match fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(read_error(error)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(read_error)?;
        if is_dir(&entry).map_err(read_error)? && is_cached_metadata_name(&entry.file_name()) {
            dirs.push(entry.path());
        }
    }
    Ok(dirs)
}

fn is_dir(entry: &fs::DirEntry) -> io::Result<bool> {
    match entry.file_type() {
        Ok(file_type) => Ok(file_type.is_dir()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_cached_metadata_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    name.starts_with("metadata")
        || name
            .strip_prefix('v')
            .is_some_and(|version| {
                !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
            })
}

#[cfg(test)]
mod tests;
