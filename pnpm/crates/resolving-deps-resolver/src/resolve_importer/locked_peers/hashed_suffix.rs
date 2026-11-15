//! Peer-provider candidates from recorded edges and resolved manifests.

use super::{HashMap, HashSet, dependency_edges};
use pnpm_lockfile::{BoundProviders, Lockfile, PkgName, PkgNameVerPeer};
use std::{cell::RefCell, sync::Arc};

pub(super) struct ProviderContexts {
    source: PeerSource,
    bindings: RefCell<HashMap<PkgNameVerPeer, Arc<BoundProviders>>>,
    names: RefCell<HashMap<PkgNameVerPeer, Arc<HashSet<PkgName>>>>,
}

enum PeerSource {
    Workspace(Arc<crate::WorkspaceTreeCtx>),
    #[cfg(test)]
    Legacy,
}

impl ProviderContexts {
    pub(super) fn workspace(workspace: Arc<crate::WorkspaceTreeCtx>) -> Self {
        Self::new(PeerSource::Workspace(workspace))
    }

    #[cfg(test)]
    pub(super) fn legacy() -> Self {
        Self::new(PeerSource::Legacy)
    }

    fn new(source: PeerSource) -> Self {
        Self { source, bindings: RefCell::default(), names: RefCell::default() }
    }

    pub(super) fn bound(&self, lockfile: &Lockfile, key: &PkgNameVerPeer) -> Arc<BoundProviders> {
        Arc::clone(
            self.bindings
                .borrow_mut()
                .entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(match &self.source {
                        PeerSource::Workspace(_) => {
                            BoundProviders::from_recorded_snapshot(lockfile.into(), key)
                        }
                        #[cfg(test)]
                        PeerSource::Legacy => BoundProviders::of_snapshot(lockfile.into(), key),
                    })
                }),
        )
    }

    pub(super) fn has_verified_context(&self, lockfile: &Lockfile, key: &PkgNameVerPeer) -> bool {
        !self.bound(lockfile, key).is_hashed()
    }

    fn peer_names(&self, lockfile: &Lockfile, key: &PkgNameVerPeer) -> Arc<HashSet<PkgName>> {
        let package = PkgNameVerPeer::new(key.name.clone(), key.suffix.without_peer_bindings());
        Arc::clone(
            self.names
                .borrow_mut()
                .entry(package)
                .or_insert_with_key(|package| Arc::new(self.read_peer_names(lockfile, package))),
        )
    }

    fn read_peer_names(&self, lockfile: &Lockfile, package: &PkgNameVerPeer) -> HashSet<PkgName> {
        match &self.source {
            PeerSource::Workspace(workspace) => {
                if let Some(names) = workspace.inspect_package(&package.to_string(), |package| {
                    package.peer_set
                        .taken()
                        .keys()
                        .filter_map(|name| name.parse().ok())
                        .collect()
                }) {
                    return names;
                }
            }
            #[cfg(test)]
            PeerSource::Legacy => {}
        }
        // A partial resolve can leave a prior snapshot outside the current tree.
        lockfile.packages
            .as_ref()
            .and_then(|packages| packages.get(&package.without_peer()))
            .and_then(|metadata| metadata.peer_dependencies.as_ref())
            .into_iter()
            .flatten()
            .filter_map(|(name, _)| name.parse().ok())
            .collect()
    }
}

/// Candidate providers for the package's taken peers and its transitive peers.
/// Explicit or recorded bindings constrain the candidates when available.
pub(super) fn hashed_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    key: &pnpm_lockfile::PkgNameVerPeer,
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    contexts: &ProviderContexts,
) -> Vec<(String, pnpm_lockfile::PkgNameVerPeer)> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    let bound = contexts.bound(lockfile, key);
    let mut keys = Vec::new();
    let peer_names = contexts.peer_names(lockfile, key);
    keys.extend(
        peer_edge_keys(snapshot, |name| peer_names.contains(name))
            .filter(|(_, provider)| bound.binds(provider)),
    );
    for name in snapshot.transitive_peer_dependencies.iter().flatten() {
        keys.extend(
            transitive_peer_keys(lockfile, snapshot, name, contexts)
                .into_iter()
                .map(|key| (name.clone(), key)),
        );
    }
    restore_bound_provider_keys(keys, &bound)
}

fn restore_bound_provider_keys(
    keys: Vec<(String, pnpm_lockfile::PkgNameVerPeer)>,
    bound: &pnpm_lockfile::BoundProviders,
) -> Vec<(String, pnpm_lockfile::PkgNameVerPeer)> {
    if bound.is_hashed() {
        return keys;
    }
    let mut restored = Vec::new();
    for (name, provider) in keys {
        for key in bound
            .provider_keys(&provider)
            .into_iter()
            .flatten()
        {
            restored.push((name.clone(), key.clone()));
        }
    }
    restored
}

/// The providers the subtree under `snapshot` resolved `peer_name` to.
///
/// Descends only into children that bind the peer, which hold the
/// edge, or that list it as a transitive peer themselves.
pub(super) fn transitive_peer_keys(
    lockfile: &pnpm_lockfile::Lockfile,
    snapshot: &pnpm_lockfile::SnapshotEntry,
    peer_name: &str,
    contexts: &ProviderContexts,
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
                bound_peer_providers(lockfile, &child_key, child, &peer, contexts);
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
    contexts: &ProviderContexts,
) -> Vec<pnpm_lockfile::PkgNameVerPeer> {
    if !contexts.peer_names(lockfile, key).contains(peer) {
        return Vec::new();
    }
    let bound = contexts.bound(lockfile, key);
    peer_edge_keys(snapshot, move |name| name == peer)
        .map(|(_, provider)| provider)
        .filter(|provider| bound.binds(provider))
        .collect()
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
