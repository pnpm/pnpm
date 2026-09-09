use crate::{CheckoutOptions, GitFetcherError, checkout_commit};
use std::{
    collections::HashMap,
    fs, io,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
use tempfile::TempDir;

type SourceResult = Result<Arc<TempDir>, Arc<GitFetcherError>>;
type SourceCell = Arc<OnceLock<SourceResult>>;

/// Shares verified Git checkouts for one installation. Repository URLs are
/// compared verbatim, including credentials and transport. Failures are retained
/// until the cache is dropped; a new installation gets a new cache.
#[derive(Default)]
pub struct GitSourceCache {
    sources: Mutex<HashMap<(String, String), SourceCell>>,
}

pub(crate) struct GitSourceOptions<'a> {
    pub repo: &'a str,
    pub commit: &'a str,
    pub git_shallow_hosts: &'a [String],
    pub git_bin: Option<&'a Path>,
}

impl GitSourceCache {
    pub(crate) fn get(&self, opts: &GitSourceOptions<'_>) -> SourceResult {
        let cell = {
            let mut sources = self.sources.lock().expect("git source cache lock poisoned");
            Arc::clone(sources.entry((opts.repo.to_owned(), opts.commit.to_owned())).or_default())
        };
        cell.get_or_init(|| {
            let source = tempfile::tempdir().map_err(GitFetcherError::Io).map_err(Arc::new)?;
            checkout_commit(&CheckoutOptions {
                repo: opts.repo,
                commit: opts.commit,
                git_shallow_hosts: opts.git_shallow_hosts,
                git_bin: opts.git_bin,
                dest: source.path(),
            })
            .map_err(Arc::new)?;
            Ok(Arc::new(source))
        })
        .clone()
    }
}

/// Copy the complete checkout without following symlinks or sharing writable
/// file inodes. Git metadata must remain available to package prepare scripts.
pub(crate) fn copy_checkout(source: &Path, dest: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let link = fs::read_link(entry.path())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, target)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::FileTypeExt;
                if file_type.is_symlink_dir() {
                    std::os::windows::fs::symlink_dir(link, target)?;
                } else {
                    std::os::windows::fs::symlink_file(link, target)?;
                }
            }
        } else if file_type.is_dir() {
            fs::create_dir(&target)?;
            copy_checkout(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
