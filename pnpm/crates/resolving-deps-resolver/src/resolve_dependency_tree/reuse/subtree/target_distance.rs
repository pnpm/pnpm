//! How far each snapshot node is from the nearest `pacquet update`
//! target ([`TargetDistances`]).

use pnpm_lockfile::{Lockfile, PkgNameVerPeer, SnapshotEntry};
use rustc_hash::FxHashMap as HashMap;

use crate::{UpdateReuseScope, UpdateTargets};

use super::snapshot_children;

/// The shortest distance, in snapshot edges, from each node of the
/// snapshot graph to the nearest `pacquet update` target. A node visited
/// at depth `d` has a target within the `--depth` ceiling `n` iff its
/// distance `k` satisfies `d + k <= n`.
///
/// Computed once per update scope, by a breadth-first search from the
/// targets over the reversed snapshot graph.
pub(crate) enum TargetDistances {
    /// Every node is a target ([`UpdateReuseScope::None`]).
    EveryNode,
    /// The nodes that reach a target, by distance. A node absent from the
    /// map reaches none.
    ByNode(HashMap<PkgNameVerPeer, i32>),
}

impl TargetDistances {
    pub(crate) fn compute(lockfile: Option<&Lockfile>, scope: &UpdateReuseScope) -> Self {
        match scope {
            UpdateReuseScope::None => Self::EveryNode,
            UpdateReuseScope::All => Self::ByNode(HashMap::default()),
            UpdateReuseScope::Except(targets) => Self::ByNode(distances_to(lockfile, targets)),
        }
    }

    /// The distance from `key` to the nearest target, when it reaches one.
    #[must_use]
    pub(crate) fn to_target(&self, key: &PkgNameVerPeer) -> Option<i32> {
        match self {
            Self::EveryNode => Some(0),
            Self::ByNode(distances) => distances.get(key).copied(),
        }
    }
}

fn distances_to(
    lockfile: Option<&Lockfile>,
    targets: &UpdateTargets,
) -> HashMap<PkgNameVerPeer, i32> {
    let mut distances = HashMap::default();
    let Some(snapshots) = lockfile.and_then(|lockfile| lockfile.snapshots.as_ref()) else {
        return distances;
    };
    let mut frontier: Vec<&PkgNameVerPeer> = snapshots
        .keys()
        .filter(|key| targets.covers(&key.name.to_string(), key.suffix.version_semver()))
        .collect();
    if frontier.is_empty() {
        return distances;
    }
    let dependents = dependents_by_child(snapshots);
    let mut distance = 0;
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for node in frontier {
            if distances.contains_key(node) {
                continue;
            }
            distances.insert(node.clone(), distance);
            next.extend(
                dependents
                    .get(node)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
        }
        frontier = next;
        distance += 1;
    }
    distances
}

/// The snapshot nodes that depend on each child key.
fn dependents_by_child(
    snapshots: &std::collections::HashMap<PkgNameVerPeer, SnapshotEntry>,
) -> HashMap<PkgNameVerPeer, Vec<&PkgNameVerPeer>> {
    let mut dependents: HashMap<PkgNameVerPeer, Vec<&PkgNameVerPeer>> = HashMap::default();
    for (parent, snapshot) in snapshots {
        for child in snapshot_children(snapshot).flatten() {
            dependents
                .entry(child)
                .or_default()
                .push(parent);
        }
    }
    dependents
}

#[cfg(test)]
mod tests;
