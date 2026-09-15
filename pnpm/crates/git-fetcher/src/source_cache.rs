use crate::{
    CheckoutOptions, GitFetcherError, GitSource, checkout_commit, fetcher::should_use_shallow,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tempfile::TempDir;

type SourceResult = Result<Arc<TempDir>, Arc<GitFetcherError>>;
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
            })
            .map_err(Arc::new)?;
            Ok(Arc::new(checkout))
        })
        .clone()
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
