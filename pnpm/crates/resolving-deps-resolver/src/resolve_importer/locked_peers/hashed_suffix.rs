//! Recovery of the peers a hashed peer suffix stands for.

use super::{HashSet, dependency_edges};
use pnpm_lockfile::PkgName;

pub(super) type BoundCache =
    super::HashMap<pnpm_lockfile::PkgNameVerPeer, pnpm_lockfile::BoundProviders>;

/// The peers a hashed suffix stands for, each with the snapshot key of
/// its provider: the package's declared peers, read from its own edges,
/// and the peers its subtree resolved, listed as its
/// `transitivePeerDependencies` and read from the descendants that
/// declare them.
pub(super) fn hashed_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    metadata: Option<&pnpm_lockfile::PackageMetadata>,
    bound_cache: &mut BoundCache,
) -> Vec<(String, pnpm_lockfile::PkgNameVerPeer)> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    let bound = cached_bound_providers(lockfile, key, bound_cache);
    let mut keys = Vec::new();
    if let Some(metadata) = metadata {
        let peer_names = metadata.peer_dependencies
            .iter()
            .flatten()
            .filter_map(|(name, _)| name.parse::<PkgName>().ok())
            .collect::<HashSet<_>>();
        keys.extend(
            peer_edge_keys(snapshot, |name| peer_names.contains(name))
                .filter(|(_, provider)| bound.binds(provider)),
        );
    }
    for name in snapshot.transitive_peer_dependencies.iter().flatten() {
        keys.extend(
            transitive_peer_keys(lockfile, snapshot, name, bound_cache)
                .into_iter()
                .map(|key| (name.clone(), key)),
        );
    }
    keys
}

/// The providers the subtree under `snapshot` resolved `peer_name` to.
///
/// Descends only into children that bind the peer, which hold the
/// edge, or that list it as a transitive peer themselves.
fn transitive_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    peer_name: &str,
    bound_cache: &mut BoundCache,
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
            let child_providers =
                bound_peer_providers(lockfile, &child_key, child, &peer, bound_cache);
            if !child_providers.is_empty() {
                providers.extend(child_providers);
            } else if lists_transitive_peer(child, peer_name) {
                pending.push(child);
            }
        }
    }
    providers
}

fn bound_peer_providers(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    peer: &PkgName,
    bound_cache: &mut BoundCache,
) -> Vec<pnpm_lockfile::PkgNameVerPeer> {
    if !declares_peer(lockfile, key, &peer.to_string()) {
        return Vec::new();
    }
    let bound = cached_bound_providers(lockfile, key, bound_cache);
    peer_edge_keys(snapshot, move |name| name == peer)
        .map(|(_, provider)| provider)
        .filter(|provider| bound.binds(provider))
        .collect()
}

fn cached_bound_providers(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    bound_cache: &mut BoundCache,
) -> pnpm_lockfile::BoundProviders {
    let providers = pnpm_lockfile::BoundProviders::from_key(key);
    if !providers.is_hashed() {
        return providers;
    }
    bound_cache
        .entry(key.clone())
        .or_insert_with(|| pnpm_lockfile::BoundProviders::of_snapshot(lockfile.into(), key))
        .clone()
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
