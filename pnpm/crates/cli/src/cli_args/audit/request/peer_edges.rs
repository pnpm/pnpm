//! Which snapshot edges only satisfy a peer.

use super::{
    GraphImporter, HashMap, HashSet, PackageKey, PackageMetadata, PkgName, SnapshotDepRef,
    SnapshotEntry,
};

const WORKSPACE_ROOT: &str = ".";

/// Finds the snapshot edges that only satisfy a peer.
///
/// An entry whose alias is one of the package's own `peerDependencies` is the
/// concrete package peer resolution picked for that peer. When every importer
/// that reaches the snapshot lists that package as a direct dependency (or the
/// workspace root does, since peers resolve from the root's dependencies), the
/// entry only satisfies the peer, and each importer's dependency field decides
/// whether the package is there: following the entry would make a peer
/// satisfied by a devDependency reachable under `--prod`. The entry is followed
/// as soon as one importer reaches the snapshot without listing the package:
/// for that importer the peer was auto-installed (`autoInstallPeers`) or
/// resolved from an ancestor package, and the entry is what provides it.
pub(super) fn peer_satisfaction_edges(
    importers: &[GraphImporter],
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<PackageKey, HashSet<PkgName>> {
    let peer_edges = peer_edges_by_snapshot(snapshots, packages);
    let direct = importers
        .iter()
        .map(direct_keys)
        .collect::<Vec<_>>();
    let root = importers
        .iter()
        .position(|importer| importer.path_segment == WORKSPACE_ROOT);
    let reached_without_listing = peer_edges
        .values()
        .flatten()
        .map(|(_, target)| target)
        .filter(|target| !root.is_some_and(|root| direct[root].contains(target)))
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|target| {
            (target, reach_from_importers_not_listing(importers, &direct, target, snapshots))
        })
        .collect::<HashMap<_, _>>();
    peer_edges
        .iter()
        .filter_map(|(key, edges)| {
            let satisfied = edges
                .iter()
                .filter(|(_, target)| {
                    reached_without_listing
                        .get(target)
                        .is_none_or(|reached| !reached.contains(*key))
                })
                .map(|(name, _)| (*name).clone())
                .collect::<HashSet<_>>();
            (!satisfied.is_empty()).then(|| ((*key).clone(), satisfied))
        })
        .collect()
}

fn peer_edges_by_snapshot<'a>(
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<&'a PackageKey, Vec<(&'a PkgName, PackageKey)>> {
    snapshots
        .iter()
        .filter_map(|(key, snapshot)| {
            let peers = packages
                .get(&key.without_peer())?
                .peer_dependencies
                .as_ref()?;
            let edges = snapshot_dependency_entries(snapshot)
                .filter(|(name, _)| peers.contains_key(&name.to_string()))
                .filter_map(|(name, dep_ref)| Some((name, dep_ref.resolve(name)?)))
                .collect::<Vec<_>>();
            (!edges.is_empty()).then_some((key, edges))
        })
        .collect()
}

fn direct_keys(importer: &GraphImporter) -> HashSet<&PackageKey> {
    importer.roots
        .iter()
        .map(|(_, edge)| &edge.key)
        .collect()
}

fn reach_from_importers_not_listing(
    importers: &[GraphImporter],
    direct: &[HashSet<&PackageKey>],
    target: &PackageKey,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> HashSet<PackageKey> {
    let mut reached = HashSet::new();
    let mut stack = importers
        .iter()
        .zip(direct)
        .filter(|(_, direct)| !direct.contains(target))
        .flat_map(|(importer, _)| importer.roots.iter().map(|(_, edge)| edge.key.clone()))
        .collect::<Vec<_>>();
    while let Some(key) = stack.pop() {
        if !reached.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        stack.extend(
            snapshot_dependency_entries(snapshot)
                .filter_map(|(name, dep_ref)| dep_ref.resolve(name)),
        );
    }
    reached
}

fn snapshot_dependency_entries(
    snapshot: &SnapshotEntry,
) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    snapshot.dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten())
}
