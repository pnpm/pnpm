//! Ask the pnpmfile's custom resolvers whether any lockfile entry must
//! be re-resolved. Port of pnpm's
//! [`checkCustomResolverForceResolve`](https://github.com/pnpm/pnpm/blob/1627943d2a/installing/deps-installer/src/install/checkCustomResolverForceResolve.ts).

use std::{path::Path, sync::Arc};

use futures_util::{StreamExt, stream::FuturesUnordered};
use serde_json::Value;

use pacquet_hooks::{CustomResolver, HookError, finder};
use pacquet_lockfile::{Lockfile, PackageKey, SnapshotEntry};

/// Load the pnpmfile at `lockfile_dir` (if any) and report whether its
/// custom resolvers force re-resolution of `lockfile`. Used by the
/// install dispatch to keep the frozen-path optimization from skipping
/// a forced re-resolve — pnpm folds the hook's verdict into
/// `needsFullResolution`, which blocks `isFrozenInstallPossible`.
pub(crate) async fn force_resolve_from_pnpmfile(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
) -> Result<bool, HookError> {
    let Some(hook) = finder::load_pnpmfile(lockfile_dir) else {
        return Ok(false);
    };
    let custom_resolvers = hook.get_custom_resolvers().await?;
    check_custom_resolver_force_resolve(&custom_resolvers, lockfile).await
}

/// Whether any custom resolver's `shouldRefreshResolution` returns true
/// for any package in `lockfile`. The hook is called independently of
/// `canResolve` — each resolver does its own filtering.
pub(crate) async fn check_custom_resolver_force_resolve(
    custom_resolvers: &[Arc<dyn CustomResolver>],
    lockfile: &Lockfile,
) -> Result<bool, HookError> {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return Ok(false);
    };
    let hooks: Vec<&Arc<dyn CustomResolver>> = custom_resolvers
        .iter()
        .filter(|resolver| resolver.has_should_refresh_resolution())
        .collect();
    if hooks.is_empty() {
        return Ok(false);
    }

    // Fire every (package, hook) check up front so the Node worker can
    // interleave the async hooks, mirroring upstream's `anyTrue` over
    // eagerly created promises; the first `true` wins and the rest are
    // dropped.
    let mut checks: FuturesUnordered<_> = snapshots
        .iter()
        .flat_map(|(dep_path, entry)| {
            let snapshot_json = merged_package_snapshot_json(lockfile, dep_path, entry);
            hooks.iter().map(move |hook| {
                let snapshot_json = snapshot_json.clone();
                async move { hook.should_refresh_resolution(dep_path, snapshot_json).await }
            })
        })
        .collect();
    while let Some(refresh) = checks.next().await {
        if refresh? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The hook receives pnpm's in-memory `PackageSnapshot` shape: the
/// `packages:` entry (resolution, engines, ...) merged with the
/// `snapshots:` entry (dependencies, optional, ...) for the dep path.
fn merged_package_snapshot_json(
    lockfile: &Lockfile,
    dep_path: &PackageKey,
    entry: &SnapshotEntry,
) -> Value {
    let mut merged = serde_json::Map::new();
    if let Some(Value::Object(fields)) = lockfile
        .packages
        .as_ref()
        .and_then(|packages| packages.get(&dep_path.without_peer()))
        .and_then(|metadata| serde_json::to_value(metadata).ok())
    {
        merged.extend(fields);
    }
    if let Ok(Value::Object(fields)) = serde_json::to_value(entry) {
        merged.extend(fields);
    }
    Value::Object(merged)
}

#[cfg(test)]
mod tests;
