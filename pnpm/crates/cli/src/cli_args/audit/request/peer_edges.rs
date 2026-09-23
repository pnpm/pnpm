//! Which snapshot edges only satisfy a peer.

use super::{
    GraphImporter, HashMap, HashSet, PackageKey, PackageMetadata, PkgName, SnapshotDepRef,
    SnapshotEntry,
};

/// Finds the snapshot edges that only satisfy a peer.
///
/// An entry whose alias is one of the package's own `peerDependencies` is the
/// concrete package peer resolution picked for that peer. When every importer
/// that reaches the snapshot lists that package as a direct dependency, the
/// entry only satisfies the peer, and each importer's dependency field decides
/// whether the package is there: following the entry would make a peer
/// satisfied by a devDependency reachable under `--prod`. The entry is followed
/// as soon as one importer reaches the snapshot without listing the package:
/// for that importer the peer was auto-installed (`autoInstallPeers`), or
/// resolved from an ancestor package or from the workspace root, and the entry
/// is what provides it. The root is not taken as the provider because the
/// lockfile doesn't record whether `resolvePeersFromWorkspaceRoot` was on.
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
    let listing_by_target = peer_edges
        .values()
        .flatten()
        .map(|(_, target)| (target, importers_listing(&direct, target)))
        .collect::<HashMap<_, _>>();
    let reached_by_listing = listing_by_target
        .values()
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|listing| (listing, reach_from_importers_not_in(importers, listing, snapshots)))
        .collect::<HashMap<_, _>>();
    peer_edges
        .iter()
        .filter_map(|(key, edges)| {
            let satisfied = edges
                .iter()
                .filter(|(_, target)| {
                    !reached_by_listing[&listing_by_target[target]].contains(*key)
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

/// The sorted indices of the importers that list `target` as a direct
/// dependency. Targets with the same listing share one reachability walk.
fn importers_listing(direct: &[HashSet<&PackageKey>], target: &PackageKey) -> Vec<usize> {
    direct
        .iter()
        .enumerate()
        .filter(|(_, keys)| keys.contains(target))
        .map(|(index, _)| index)
        .collect()
}

/// Walks every edge from the importers whose index is not in the sorted
/// `listing`.
fn reach_from_importers_not_in(
    importers: &[GraphImporter],
    listing: &[usize],
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> HashSet<PackageKey> {
    let mut reached = HashSet::new();
    let mut stack = importers
        .iter()
        .enumerate()
        .filter(|(index, _)| listing.binary_search(index).is_err())
        .flat_map(|(_, importer)| importer.roots.iter().map(|(_, edge)| edge.key.clone()))
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
