use crate::{
    CheckoutOptions, GitFetcherError, GitSource, checkout_commit,
    fetcher::{checkout_submodules_with, should_use_shallow},
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tempfile::TempDir;

#[derive(Debug)]
pub(crate) struct CachedSource {
    checkout: TempDir,
    submodules: OnceLock<Result<(), Arc<GitFetcherError>>>,
}

impl CachedSource {
    pub(crate) fn path(&self) -> &Path {
        self.checkout.path()
    }

    pub(crate) fn ensure_submodules(
        &self,
        git_bin: Option<&Path>,
    ) -> Result<(), Arc<GitFetcherError>> {
        self.submodules
            .get_or_init(|| {
                if has_submodules(self.path()).map_err(Arc::new)? {
                    checkout_submodules_with(
                        git_bin.unwrap_or_else(|| Path::new("git")),
                        self.path(),
                    )
                    .map_err(Arc::new)?;
                }
                Ok(())
            })
            .clone()
    }
}

type SourceResult = Result<Arc<CachedSource>, Arc<GitFetcherError>>;
type SourceCell = Arc<OnceLock<SourceResult>>;

/// Shares verified Git checkouts for one installation. Two requests share a
/// checkout only when `git` would produce it identically: the same
/// repository URL compared verbatim, including credentials and transport,
/// the same commit, the same shallow-fetch decision, and the same `git`
/// executable. Failures are retained until the cache is dropped; a new
/// installation gets a new cache.
#[derive(Default)]
pub struct GitSourceCache {
    sources: Mutex<HashMap<SourceKey, SourceCell>>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct SourceKey {
    repo: String,
    commit: String,
    shallow: bool,
    git_bin: Option<PathBuf>,
}

impl GitSourceCache {
    pub(crate) fn get(&self, source: &GitSource<'_>) -> SourceResult {
        self.get_with_config(source, &[])
    }

    /// [`Self::get`], checking the source out with `git -c` settings
    /// ([`CheckoutOptions::git_config`]). They choose how git reaches the
    /// remote, not what it checks out, so they are not part of the key.
    pub(crate) fn get_with_config(
        &self,
        source: &GitSource<'_>,
        git_config: &[String],
    ) -> SourceResult {
        let cell = {
            let mut sources = self.sources.lock().expect("git source cache lock poisoned");
            Arc::clone(
                sources
                    .entry(SourceKey::new(source))
                    .or_default(),
            )
        };
        cell.get_or_init(|| {
            let checkout = tempfile::tempdir().map_err(GitFetcherError::Io).map_err(Arc::new)?;
            checkout_commit(&CheckoutOptions {
                repo: source.repo,
                commit: source.commit,
                git_shallow_hosts: source.shallow_hosts,
                git_bin: source.git_bin,
                dest: checkout.path(),
                git_config,
            })
            .map_err(Arc::new)?;
            Ok(Arc::new(CachedSource { checkout, submodules: OnceLock::new() }))
        })
        .clone()
    }
}

fn has_submodules(checkout: &Path) -> Result<bool, GitFetcherError> {
    match fs::metadata(checkout.join(".gitmodules")) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(GitFetcherError::Io(err)),
    }
}

impl SourceKey {
    fn new(source: &GitSource<'_>) -> Self {
        SourceKey {
            repo: source.repo.to_owned(),
            commit: source.commit.to_owned(),
            shallow: should_use_shallow(source.repo, source.shallow_hosts),
            git_bin: source.git_bin.map(Path::to_path_buf),
        }
    }
}

#[cfg(test)]
mod tests;
