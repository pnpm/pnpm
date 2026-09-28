//! Cycles in a lockfile's peer bindings.

use std::collections::{HashMap, HashSet};

use crate::{
    PackageKey, PackageMetadata, SnapshotEntry,
    peer_edges::{all_entries, declares_peer},
};

/// Whether a peer binding of `snapshots` leads a snapshot back to itself.
///
/// The graph has one node per snapshot key and one edge per entry whose
/// alias the dependent declares in `packages` as a peer. `packages` is the
/// lockfile's `packages:` map, which is where the peer declarations live.
#[must_use]
pub fn has_circular_peers(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> bool {
    PeerCycleWalk::new(snapshots, packages).run()
}

/// Depth-first search over the peer bindings, iterative because a lockfile
/// chain can outgrow the stack a recursive walk needs.
struct PeerCycleWalk<'a> {
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    packages: &'a HashMap<PackageKey, PackageMetadata>,
    /// Every node a walk has reached. One that is no longer in `on_path`
    /// has been searched, so reaching it again settles nothing.
    explored: HashSet<PackageKey>,
    /// The nodes the walk in progress is inside. Reaching one of these
    /// closes a cycle.
    on_path: HashSet<PackageKey>,
}

impl<'a> PeerCycleWalk<'a> {
    fn new(
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        packages: &'a HashMap<PackageKey, PackageMetadata>,
    ) -> Self {
        Self { snapshots, packages, explored: HashSet::new(), on_path: HashSet::new() }
    }

    fn run(mut self) -> bool {
        // The walk only reads the snapshots, so holding the reference
        // separately leaves the borrow of `self` for the search.
        let snapshots = self.snapshots;
        snapshots
            .keys()
            .any(|node| self.walk_from(node))
    }

    /// Whether a walk out of `start` reaches a node already on it.
    fn walk_from(&mut self, start: &PackageKey) -> bool {
        self.on_path.insert(start.clone());
        let mut walk = vec![(start.clone(), self.peer_targets(start))];
        while let Some((key, mut targets)) = walk.pop() {
            let Some(target) = targets.pop() else {
                self.on_path.remove(&key);
                continue;
            };
            // The frame goes back below its child's, so `key` stays on the
            // path until every target under it has been searched.
            walk.push((key, targets));
            if self.on_path.contains(&target) {
                return true;
            }
            if !self.explored.insert(target.clone()) {
                continue;
            }
            self.on_path.insert(target.clone());
            walk.push((target.clone(), self.peer_targets(&target)));
        }
        false
    }

    /// The keys the peer bindings of `key` lead to, skipping the entries
    /// that bind no peer. A target the lockfile holds no snapshot for has
    /// no bindings of its own, so the chain ends there.
    fn peer_targets(&self, key: &PackageKey) -> Vec<PackageKey> {
        let Some(snapshot) = self.snapshots.get(key) else { return Vec::new() };
        let Some(metadata) = self.packages.get(&key.without_peer()) else { return Vec::new() };
        all_entries(snapshot)
            .filter(|(alias, _)| declares_peer(metadata, &alias.to_string()))
            .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
            .collect()
    }
}

#[cfg(test)]
mod tests;
