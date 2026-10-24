use crate::{PruneError, StoreDir};
use pnpm_fs::OpenDir;
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
    /// for a `cacheDir` that encloses them. `cache_dir` is held open and
    /// each layout directory is opened and emptied through it, so on Unix a
    /// link put in either's place meanwhile does not redirect the removal
    /// (see [`OpenDir`]).
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
        let cache_dir = match OpenDir::open(cache_dir) {
            Ok(dir) => dir,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(read_error(cache_dir, error)),
        };
        let names = child_names(&cache_dir)?;
        remove_metadata_dirs(&cache_dir, &names, &protected)?;
        for name in names.iter().filter(|name| is_layout_version(name)) {
            let Some(layout_dir) = cache_dir
                .open_child_dir(name.as_ref())
                .map_err(|error| read_error(&cache_dir.path().join(name), error))?
            else {
                continue;
            };
            let nested = child_names(&layout_dir)?;
            remove_metadata_dirs(&layout_dir, &nested, &protected)?;
        }
        pnpm_diagnostics::eprintln!("Removed all cached metadata files");
        Ok(())
    }
}

/// Remove the directories among `names` whose name starts with `metadata`
/// from `dir`, keeping those that are or hold a `protected` directory.
fn remove_metadata_dirs(
    dir: &OpenDir,
    names: &[String],
    protected: &[PathBuf],
) -> Result<(), PruneError> {
    let Some(canonical_dir) =
        dir.canonical_path().map_err(|error| read_error(dir.path(), error))?
    else {
        tracing::warn!(
            path = %dir.path().display(),
            "a cache directory was replaced while pruning, its metadata is kept",
        );
        return Ok(());
    };
    for name in names
        .iter()
        .filter(|name| name.starts_with("metadata"))
    {
        if holds_any(&canonical_dir.join(name), protected) {
            continue;
        }
        dir.remove_child_dir(name.as_ref())
            .map_err(|error| PruneError::RemoveCachedMetadata {
                path: dir.path().join(name),
                error,
            })?;
    }
    Ok(())
}

fn holds_any(dir: &Path, protected: &[PathBuf]) -> bool {
    protected
        .iter()
        .any(|path| path.starts_with(dir))
}

/// The names of the entries of `dir`, none when its path no longer leads
/// anywhere. Entries whose names are not UTF-8 are skipped.
fn child_names(dir: &OpenDir) -> Result<Vec<String>, PruneError> {
    let read_error = |error| read_error(dir.path(), error);
    let entries = match fs::read_dir(dir.path()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(read_error(error)),
    };
    let mut names = Vec::new();
    for entry in entries {
        if let Ok(name) = entry
            .map_err(read_error)?
            .file_name()
            .into_string()
        {
            names.push(name);
        }
    }
    Ok(names)
}

fn read_error(path: &Path, error: io::Error) -> PruneError {
    PruneError::ReadCacheDir { path: path.to_path_buf(), error }
}

fn is_layout_version(name: &str) -> bool {
    name.strip_prefix('v')
        .is_some_and(|version| {
            !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests;
