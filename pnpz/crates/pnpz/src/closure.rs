use crate::{
    binary_cache::{
        BinaryCache,
        CacheError,
    },
    lockfile::{
        Closure,
        LockedPath,
    },
};
use futures_util::{
    StreamExt as _,
    TryStreamExt as _,
    stream,
};
use pnpz_nix_store::StorePath;

/// How many narinfo requests run at once.
const NARINFO_CONCURRENCY: usize = 32;

/// Finds every store path `roots` need, by following the references the
/// binary cache lists for each path.
///
/// Each level of the reference graph is one round of requests, because a
/// path's references are only known once its narinfo arrives.
pub async fn resolve_closure(
    cache: &BinaryCache,
    roots: &[StorePath],
) -> Result<Closure, CacheError> {
    let mut closure = Closure::new();
    let mut frontier: Vec<StorePath> = roots.to_vec();
    while !frontier.is_empty() {
        frontier.sort();
        frontier.dedup();
        let narinfos: Vec<_> = stream::iter(&frontier)
            .map(|path| cache.narinfo(path))
            .buffer_unordered(NARINFO_CONCURRENCY)
            .try_collect()
            .await?;
        for narinfo in &narinfos {
            let locked = LockedPath {
                nar_hash: narinfo.nar_hash,
                nar_size: narinfo.nar_size,
                references: narinfo.references.clone(),
            };
            closure.insert(narinfo.store_path.clone(), locked);
        }
        frontier = narinfos
            .iter()
            .flat_map(|narinfo| narinfo.references.iter())
            .filter(|reference| !closure.contains_key(*reference))
            .cloned()
            .collect();
    }
    Ok(closure)
}
