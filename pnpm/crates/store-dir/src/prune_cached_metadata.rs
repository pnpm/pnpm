use crate::{PruneError, StoreDir};
use std::{
    fs,
    io::{self, ErrorKind},
    iter,
    path::{Path, PathBuf},
};

impl StoreDir {
    /// Remove the registry metadata cached under `cache_dir`: every
    /// directory whose name starts with `metadata`, directly under
    /// `cache_dir` or under one of its cache layout directories such as
    /// `v12`. The rest of a layout directory is kept.
    ///
    /// A symbolic link is not a directory here, so it is kept. So is a
    /// directory that is or holds this store or one of `protected_dirs`,
    /// for a `cacheDir` that encloses them.
    ///
    /// The caller holds the store's prune lock, which keeps out the installs
    /// that read the cache.
    pub(crate) fn remove_cached_metadata(
        &self,
        cache_dir: &Path,
        protected_dirs: &[&Path],
    ) -> Result<(), PruneError> {
        let protected: Vec<PathBuf> = iter::once(self.root())
            .chain(protected_dirs.iter().copied())
            .map(|dir| dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()))
            .collect();
        for path in cached_metadata_dirs(cache_dir)? {
            if holds_any(&path, &protected) {
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

fn holds_any(dir: &Path, protected: &[PathBuf]) -> bool {
    dunce::canonicalize(dir)
        .is_ok_and(|dir| {
            protected
                .iter()
                .any(|path| path.starts_with(&dir))
        })
}

fn cached_metadata_dirs(cache_dir: &Path) -> Result<Vec<PathBuf>, PruneError> {
    let mut metadata_dirs = Vec::new();
    for (path, name) in child_dirs(cache_dir)? {
        if name.starts_with("metadata") {
            metadata_dirs.push(path);
        } else if is_layout_version(&name) {
            let nested = child_dirs(&path)?
                .into_iter()
                .filter(|(_, name)| name.starts_with("metadata"))
                .map(|(path, _)| path);
            metadata_dirs.extend(nested);
        }
    }
    Ok(metadata_dirs)
}

/// The directories directly under `dir` with their names, none when `dir`
/// is missing. Entries whose names are not UTF-8 are skipped.
fn child_dirs(dir: &Path) -> Result<Vec<(PathBuf, String)>, PruneError> {
    let read_error = |error| PruneError::ReadCacheDir { path: dir.to_path_buf(), error };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(read_error(error)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(read_error)?;
        if is_dir(&entry).map_err(read_error)?
            && let Ok(name) = entry.file_name().into_string()
        {
            dirs.push((entry.path(), name));
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

fn is_layout_version(name: &str) -> bool {
    name.strip_prefix('v')
        .is_some_and(|version| {
            !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests;
