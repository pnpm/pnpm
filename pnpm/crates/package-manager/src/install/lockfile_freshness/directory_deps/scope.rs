use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashSet;

/// The snapshots already checked, each with the peers it took from its
/// ancestors: what a package must record depends on that scope.
pub(super) type Visited = HashSet<(pnpm_lockfile::PackageKey, Vec<String>)>;

/// The aliases of every ancestor of a package, which supply its peers.
pub(super) type AliasScope = HashSet<String>;

pub(super) fn importer_aliases(importer: &pnpm_lockfile::ProjectSnapshot) -> AliasScope {
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .into_iter()
        .filter_map(|group| importer.get_map_by_group(group))
        .flat_map(|dependencies| dependencies.keys().map(ToString::to_string))
        .collect()
}

pub(super) fn snapshot_aliases(
    snapshot: &pnpm_lockfile::SnapshotEntry,
) -> impl Iterator<Item = String> + '_ {
    [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|dependencies| dependencies.keys().map(ToString::to_string))
}

/// The declared peers an ancestor supplies, sorted.
pub(super) fn peers_in_scope(manifest: &PackageManifest, scope: &AliasScope) -> Vec<String> {
    let mut peers: Vec<String> = manifest
        .value()
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|peers| peers.keys())
        .filter(|name| scope.contains(name.as_str()))
        .cloned()
        .collect();
    peers.sort_unstable();
    peers
}
