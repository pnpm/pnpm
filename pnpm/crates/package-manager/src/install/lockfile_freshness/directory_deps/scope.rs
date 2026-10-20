use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashSet;

/// The snapshots already checked, each with the peers its ancestors supply:
/// what a package and its descendants must record depends on that scope.
pub(super) struct Visited {
    checked: HashSet<(pnpm_lockfile::PackageKey, Vec<String>)>,
    /// Every peer the lockfile records for a package, which bounds the part
    /// of a scope the descendants of a package can take.
    recorded_peers: HashSet<String>,
}

impl Visited {
    pub(super) fn new(lockfile: &pnpm_lockfile::Lockfile) -> Self {
        let recorded_peers = lockfile.packages
            .iter()
            .flat_map(|packages| packages.values())
            .filter_map(|metadata| metadata.peer_dependencies.as_ref())
            .flat_map(|peers| peers.keys().cloned())
            .collect();
        Visited { checked: HashSet::new(), recorded_peers }
    }

    /// Records the snapshot in `scope`. `false` when it was already checked
    /// there: a cycle or a shared dependency.
    pub(super) fn first_visit(
        &mut self,
        pkg_key: &pnpm_lockfile::PackageKey,
        manifest: &PackageManifest,
        scope: &AliasScope,
    ) -> bool {
        let mut peers: Vec<String> = declared_peer_names(manifest)
            .chain(self.recorded_peers.iter().map(String::as_str))
            .filter(|name| scope.contains(*name))
            .map(ToString::to_string)
            .collect();
        peers.sort_unstable();
        peers.dedup();
        self.checked.insert((pkg_key.clone(), peers))
    }
}

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

fn declared_peer_names(manifest: &PackageManifest) -> impl Iterator<Item = &str> {
    manifest
        .value()
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|peers| peers.keys().map(String::as_str))
}
