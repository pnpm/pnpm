//! Recovery of the peers a hashed peer suffix stands for.

use super::{HashSet, dependency_edges};
use pnpm_lockfile::PkgName;

/// The peers a hashed suffix stands for, each with the snapshot key of
/// its provider: the package's declared peers, read from its own edges,
/// and the peers its subtree resolved, listed as its
/// `transitivePeerDependencies` and read from the descendants that
/// declare them.
pub(super) fn hashed_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    metadata: Option<&pnpm_lockfile::PackageMetadata>,
) -> Vec<(String, pnpm_lockfile::PkgNameVerPeer)> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    if let Some(metadata) = metadata {
        let peer_names = metadata.peer_dependencies
            .iter()
            .flatten()
            .filter_map(|(name, _)| name.parse::<PkgName>().ok())
            .collect::<HashSet<_>>();
        keys.extend(peer_edge_keys(snapshot, |name| peer_names.contains(name)));
    }
    for name in snapshot.transitive_peer_dependencies.iter().flatten() {
        keys.extend(
            transitive_peer_keys(lockfile, snapshot, name)
                .into_iter()
                .map(|key| (name.clone(), key)),
        );
    }
    keys
}

/// The providers the subtree under `snapshot` resolved `peer_name` to.
///
/// Descends only into children that declare the peer, which hold the
/// edge, or that list it as a transitive peer themselves.
fn transitive_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    peer_name: &str,
) -> Vec<pnpm_lockfile::PkgNameVerPeer> {
    let Ok(peer) = peer_name.parse::<PkgName>() else {
        return Vec::new();
    };
    let mut providers = Vec::new();
    let mut visited = HashSet::default();
    let mut pending = vec![snapshot];
    while let Some(snapshot) = pending.pop() {
        let children: Vec<_> = child_snapshots(lockfile, snapshot)
            .filter(|(child_key, _)| visited.insert(child_key.clone()))
            .collect();
        for (child_key, child) in children {
            if declares_peer(lockfile, &child_key, peer_name) {
                providers.extend(peer_providers(child, &peer));
            } else if lists_transitive_peer(child, peer_name) {
                pending.push(child);
            }
        }
    }
    providers
}

fn peer_providers<'a>(
    snapshot: &'a pnpm_lockfile::SnapshotEntry,
    peer: &'a PkgName,
) -> impl Iterator<Item = pnpm_lockfile::PkgNameVerPeer> + 'a {
    peer_edge_keys(snapshot, move |name| name == peer).map(|(_, provider)| provider)
}

fn child_snapshots<'a>(
    lockfile: &'a pnpm_lockfile::Lockfile,
    snapshot: &'a pnpm_lockfile::SnapshotEntry,
) -> impl Iterator<Item = (pnpm_lockfile::PkgNameVerPeer, &'a pnpm_lockfile::SnapshotEntry)> + 'a {
    dependency_edges(snapshot)
        .filter_map(|(name, reference)| {
            let key = reference.resolve(name)?;
            let child = lockfile.snapshots.as_ref()?.get(&key)?;
            Some((key, child))
        })
}

fn lists_transitive_peer(snapshot: &pnpm_lockfile::SnapshotEntry, peer_name: &str) -> bool {
    snapshot.transitive_peer_dependencies
        .iter()
        .flatten()
        .any(|name| name == peer_name)
}

fn declares_peer(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    peer_name: &str,
) -> bool {
    lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref())
        .is_some_and(|peers| peers.contains_key(peer_name))
}

fn peer_edge_keys<'a>(
    snapshot: &'a pnpm_lockfile::SnapshotEntry,
    is_peer: impl Fn(&PkgName) -> bool + 'a,
) -> impl Iterator<Item = (String, pnpm_lockfile::PkgNameVerPeer)> + 'a {
    dependency_edges(snapshot)
        .filter(move |(name, _)| is_peer(name))
        .filter_map(|(name, reference)| {
            reference
                .resolve(name)
                .map(|key| (name.to_string(), key))
        })
}
