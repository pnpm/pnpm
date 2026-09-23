//! Which snapshot edges only satisfy a peer.

use super::{
    GraphImporter, HashMap, HashSet, PackageKey, PackageMetadata, PkgName, SnapshotDepRef,
    SnapshotEntry,
};

/// Finds the snapshot edges that only satisfy a peer.
///
/// An entry whose alias is one of the package's own `peerDependencies` is the
/// concrete package peer resolution picked for that peer. When that package is
/// a direct dependency of an importer that reaches the snapshot, the entry only
/// satisfies the peer with the importer's own dependency, and whether that
/// dependency is present is decided by the importer's dependency field:
/// following the entry would make a peer satisfied by a devDependency reachable
/// under `--prod`. Any other peer entry is followed: the peer was
/// auto-installed (`autoInstallPeers`) or resolved from an ancestor package,
/// and following it can only over-report.
pub(super) fn peer_satisfaction_edges(
    importers: &[GraphImporter],
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<PackageKey, HashSet<PkgName>> {
    let peer_edges = peer_edges_by_snapshot(snapshots, packages);
    let listing = importers_listing(importers);
    let candidates = peer_edges
        .values()
        .flatten()
        .filter_map(|(_, target)| listing.get(target))
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    let reach = candidates
        .into_iter()
        .map(|index| (index, reach_from_importer(&importers[index], snapshots)))
        .collect::<HashMap<_, _>>();
    peer_edges
        .into_iter()
        .filter_map(|(key, edges)| {
            let satisfied = edges
                .into_iter()
                .filter(|(_, target)| listing_importer_reaches(&listing, &reach, target, key))
                .map(|(name, _)| name.clone())
                .collect::<HashSet<_>>();
            (!satisfied.is_empty()).then(|| (key.clone(), satisfied))
        })
        .collect()
}

/// Whether an importer listing `target` as a direct dependency reaches `key`.
fn listing_importer_reaches(
    listing: &HashMap<&PackageKey, Vec<usize>>,
    reach: &HashMap<usize, HashSet<PackageKey>>,
    target: &PackageKey,
    key: &PackageKey,
) -> bool {
    listing
        .get(target)
        .is_some_and(|importers| {
            importers
                .iter()
                .any(|index| reach[index].contains(key))
        })
}

/// Each snapshot's entries whose alias is a peer of its package, with the
/// entry's target.
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

/// The indexes of the importers listing each package as a direct dependency.
fn importers_listing(importers: &[GraphImporter]) -> HashMap<&PackageKey, Vec<usize>> {
    let mut listing = HashMap::<_, Vec<_>>::new();
    for (index, importer) in importers.iter().enumerate() {
        for (_, edge) in &importer.roots {
            listing
                .entry(&edge.key)
                .or_default()
                .push(index);
        }
    }
    listing
}

fn reach_from_importer(
    importer: &GraphImporter,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> HashSet<PackageKey> {
    let mut reached = HashSet::new();
    let mut stack = importer.roots
        .iter()
        .map(|(_, edge)| edge.key.clone())
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
