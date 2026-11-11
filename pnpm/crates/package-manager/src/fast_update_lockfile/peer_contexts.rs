use super::{
    DroppedEdges, Lockfile, PkgNameVerPeer, peer_id_of, reachable_from_keys, snapshot_child_keys,
};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) fn peer_sensitive_packages(lockfile: &Lockfile) -> HashSet<PkgNameVerPeer> {
    let mut parents: HashMap<PkgNameVerPeer, Vec<PkgNameVerPeer>> = HashMap::new();
    let mut queue = VecDeque::new();
    for (key, snapshot) in lockfile.snapshots.iter().flatten() {
        if has_declared_peers(lockfile, key) {
            queue.push_back(key.clone());
        }
        for child in snapshot_child_keys(snapshot, |_| true) {
            parents
                .entry(child)
                .or_default()
                .push(key.clone());
        }
    }
    let mut sensitive = HashSet::new();
    while let Some(key) = queue.pop_front() {
        if sensitive.insert(key.clone()) {
            queue.extend(
                parents
                    .remove(&key)
                    .into_iter()
                    .flatten(),
            );
        }
    }
    sensitive
        .into_iter()
        .map(|key| key.without_peer())
        .collect()
}

impl DroppedEdges {
    pub(super) fn including_subtrees(&self, lockfile: &Lockfile) -> Self {
        let mut affected = Self(self.0.clone());
        let Some(snapshots) = &lockfile.snapshots else { return affected };
        let roots = snapshots
            .keys()
            .filter(|key| self.contains_package(key))
            .cloned()
            .collect();
        for key in reachable_from_keys(lockfile, roots, |_, _| true).into_iter().flatten() {
            affected.0.insert(peer_id_of(&key).unwrap_or_else(|| format!("{}@", key.name)));
        }
        affected
    }
}

pub(super) fn has_declared_peers(lockfile: &Lockfile, key: &PkgNameVerPeer) -> bool {
    lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref())
        .is_some_and(|peers| !peers.is_empty())
}
