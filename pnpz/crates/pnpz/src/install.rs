use crate::{
    binary_cache::{
        BinaryCache,
        CacheError,
    },
    lockfile::{
        Closure,
        LockedPath,
    },
    store::Store,
};
use derive_more::{
    Display,
    Error,
};
use futures_util::{
    StreamExt as _,
    TryStreamExt as _,
    stream,
};
use miette::Diagnostic;
use pnpz_nix_store::StorePath;
use std::io;

/// How many archives download and unpack at once.
const FETCH_CONCURRENCY: usize = 8;

#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallError {
    #[diagnostic(transparent)]
    Cache(CacheError),
    #[display(
        "the binary cache describes {store_path} as {found}, and the lockfile records {expected}"
    )]
    #[diagnostic(
        code(ERR_PNPZ_LOCKFILE_HASH_MISMATCH),
        help("the lockfile or the cache was changed since the path was locked")
    )]
    LockedHashMismatch { store_path: String, expected: String, found: String },
    #[display("failed to move {store_path} into the store: {source}")]
    #[diagnostic(code(ERR_PNPZ_STORE_COMMIT))]
    Commit { store_path: String, source: io::Error },
}

/// What an install did, for the caller to report.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InstallSummary {
    pub fetched_paths: usize,
    pub fetched_bytes: u64,
    pub reused_paths: usize,
}

/// The paths of `closure` the store lacks.
#[must_use]
pub fn missing_paths<'closure>(
    store: &Store,
    closure: &'closure Closure,
) -> Vec<(&'closure StorePath, &'closure LockedPath)> {
    closure
        .iter()
        .filter(|(path, _)| !store.contains(path))
        .collect()
}

/// Downloads every path of `closure` that `store` lacks.
///
/// `on_fetched` is called with each path once it is in the store.
pub async fn install_closure(
    cache: &BinaryCache,
    store: &Store,
    closure: &Closure,
    on_fetched: impl Fn(&StorePath, &LockedPath) + Send + Sync,
) -> Result<InstallSummary, InstallError> {
    let missing = missing_paths(store, closure);
    let summary = InstallSummary {
        fetched_paths: missing.len(),
        fetched_bytes: missing
            .iter()
            .map(|(_, locked)| locked.nar_size)
            .sum(),
        reused_paths: closure.len() - missing.len(),
    };
    stream::iter(missing)
        .map(|(path, locked)| install_path(cache, store, path, locked))
        .buffer_unordered(FETCH_CONCURRENCY)
        .try_for_each(|(path, locked)| {
            on_fetched(path, locked);
            async { Ok(()) }
        })
        .await?;
    Ok(summary)
}

async fn install_path<'closure>(
    cache: &BinaryCache,
    store: &Store,
    path: &'closure StorePath,
    locked: &'closure LockedPath,
) -> Result<(&'closure StorePath, &'closure LockedPath), InstallError> {
    let narinfo = cache.narinfo(path).await.map_err(InstallError::Cache)?;
    if (narinfo.nar_hash, narinfo.nar_size) != (locked.nar_hash, locked.nar_size) {
        return Err(InstallError::LockedHashMismatch {
            store_path: path.to_string(),
            expected: locked.nar_hash.to_sri(),
            found: narinfo.nar_hash.to_sri(),
        });
    }
    let staging = store.staging_location_of(path);
    cache.fetch(&narinfo, &staging).await.map_err(InstallError::Cache)?;
    store
        .commit(&staging, path)
        .map_err(|source| InstallError::Commit { store_path: path.to_string(), source })?;
    Ok((path, locked))
}
