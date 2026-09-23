//! Which snapshot edges only satisfy an optional peer.

use std::collections::{HashMap, HashSet};

use crate::{Lockfile, PackageKey, PackageMetadata, PkgName, SnapshotDepRef, SnapshotEntry};

/// How [`PeerSatisfactionEdges::of_lockfile`] decides which importers
/// provide a peer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PeerEdgeOptions {
    /// `resolvePeersFromWorkspaceRoot`: a `devDependencies` entry of the root
    /// importer counts as listed by every importer.
    pub resolve_peers_from_workspace_root: bool,
}

/// The dependency graph [`PeerSatisfactionEdges::of_graph`] classifies.
pub struct PeerEdgeGraph<'a> {
    /// The snapshot keys each importer depends on directly, in any group.
    pub importers: Vec<HashSet<PackageKey>>,
    /// The snapshot keys every importer counts as listing: the root
    /// importer's `devDependencies` under `resolvePeersFromWorkspaceRoot`.
    /// A root production dependency is not among them, because a walk that
    /// leaves the root out would then drop a peer nothing else provides.
    pub listed_by_every_importer: HashSet<PackageKey>,
    pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub packages: &'a HashMap<PackageKey, PackageMetadata>,
}

/// The snapshot edges that only satisfy an optional peer.
///
/// Peer resolution records the package it picked for a peer in the
/// dependent's snapshot `dependencies` or `optionalDependencies`, so a walk
/// follows that entry like a real dependency. An entry `P -> T` under alias
/// `a` only satisfies a peer when `a` is an optional peer of `P` and every
/// importer that reaches `P` lists `T` as a direct dependency (or, with
/// `resolvePeersFromWorkspaceRoot`, the root importer lists it as a
/// devDependency). Each of those
/// importers then decides through its own dependency field whether `T` is
/// installed, so a walk that leaves out that field must not reach `T` through
/// `P`.
///
/// The entry is followed as soon as one importer reaches `P` without listing
/// `T`: for that importer the entry is what provides `T`. An edge for a
/// required peer is always followed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PeerSatisfactionEdges {
    aliases_by_snapshot: HashMap<PackageKey, HashSet<PkgName>>,
}

impl PeerSatisfactionEdges {
    /// Classify every edge of `lockfile` over all of its importers.
    #[must_use]
    pub fn of_lockfile(lockfile: &Lockfile, options: PeerEdgeOptions) -> Self {
        let (Some(snapshots), Some(packages)) =
            (lockfile.snapshots.as_ref(), lockfile.packages.as_ref())
        else {
            return Self::default();
        };
        let listed_by_every_importer = lockfile
            .root_project()
            .filter(|_| options.resolve_peers_from_workspace_root)
            .and_then(|root| root.dev_dependencies.as_ref())
            .into_iter()
            .flatten()
            .filter_map(|(alias, spec)| spec.version.resolved_key(alias))
            .collect();
        let importers = lockfile.importers
            .values()
            .map(direct_keys)
            .collect();
        Self::of_graph(&PeerEdgeGraph { importers, listed_by_every_importer, snapshots, packages })
    }

    /// Classify every edge of `graph`.
    #[must_use]
    pub fn of_graph(graph: &PeerEdgeGraph<'_>) -> Self {
        let candidates = optional_peer_edges(graph.snapshots, graph.packages);
        let listing_by_target = candidates
            .values()
            .flatten()
            .map(|(_, target)| (target, importers_listing(graph, target)))
            .collect::<HashMap<_, _>>();
        let reached_by_listing = listing_by_target
            .values()
            .collect::<HashSet<_>>()
            .into_iter()
            .map(|listing| (listing, reach_from_importers_not_in(graph, listing)))
            .collect::<HashMap<_, _>>();
        let aliases_by_snapshot = candidates
            .iter()
            .filter_map(|(key, edges)| {
                let aliases = edges
                    .iter()
                    .filter(|(_, target)| {
                        !reached_by_listing[&listing_by_target[target]].contains(*key)
                    })
                    .map(|(alias, _)| (*alias).clone())
                    .collect::<HashSet<_>>();
                (!aliases.is_empty()).then(|| ((*key).clone(), aliases))
            })
            .collect();
        PeerSatisfactionEdges { aliases_by_snapshot }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.aliases_by_snapshot.is_empty()
    }

    /// Whether the entry `alias` of the snapshot `key` only satisfies a peer.
    #[must_use]
    pub fn contains(&self, key: &PackageKey, alias: &PkgName) -> bool {
        self.aliases_by_snapshot
            .get(key)
            .is_some_and(|aliases| aliases.contains(alias))
    }

    /// Every snapshot with such edges, and their aliases.
    pub fn iter(&self) -> impl Iterator<Item = (&PackageKey, &HashSet<PkgName>)> {
        self.aliases_by_snapshot.iter()
    }

    /// The entries of `snapshot`, stored under `key`, that a walk follows:
    /// its `dependencies`, its `optionalDependencies` when `include_optional`,
    /// and none of its peer-satisfaction edges.
    pub fn followed_entries<'a>(
        &'a self,
        key: &PackageKey,
        snapshot: &'a SnapshotEntry,
        include_optional: bool,
    ) -> impl Iterator<Item = (&'a PkgName, &'a SnapshotDepRef)> + 'a {
        let skipped = self.aliases_by_snapshot.get(key);
        let optional =
            include_optional.then_some(snapshot.optional_dependencies.as_ref()).flatten();
        snapshot.dependencies
            .iter()
            .chain(optional)
            .flatten()
            .filter(move |(alias, _)| skipped.is_none_or(|skipped| !skipped.contains(*alias)))
    }

    /// Remove from each snapshot in `snapshots` its peer-satisfaction entries
    /// whose target `snapshots` does not hold. An entry whose target is kept
    /// through another path stays, because that package is installed and the
    /// dependent must link it.
    pub fn prune_dangling(&self, snapshots: &mut HashMap<PackageKey, SnapshotEntry>) {
        let dropped = self.aliases_by_snapshot
            .iter()
            .filter_map(|(key, aliases)| {
                let snapshot = snapshots.get(key)?;
                let aliases = aliases
                    .iter()
                    .filter(|alias| !target_retained(snapshot, alias, snapshots))
                    .cloned()
                    .collect::<Vec<_>>();
                (!aliases.is_empty()).then(|| (key.clone(), aliases))
            })
            .collect::<Vec<_>>();
        for (key, aliases) in dropped {
            if let Some(snapshot) = snapshots.get_mut(&key) {
                remove_entries(snapshot, &aliases);
            }
        }
    }
}

impl FromIterator<(PackageKey, HashSet<PkgName>)> for PeerSatisfactionEdges {
    fn from_iter<Edges: IntoIterator<Item = (PackageKey, HashSet<PkgName>)>>(edges: Edges) -> Self {
        PeerSatisfactionEdges { aliases_by_snapshot: edges.into_iter().collect() }
    }
}

fn direct_keys(importer: &crate::ProjectSnapshot) -> HashSet<PackageKey> {
    [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .filter_map(|(alias, spec)| spec.version.resolved_key(alias))
    .collect()
}

/// Each snapshot's entries whose alias is one of its package's optional
/// peers, with the key each resolves to.
fn optional_peer_edges<'a>(
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<&'a PackageKey, Vec<(&'a PkgName, PackageKey)>> {
    snapshots
        .iter()
        .filter_map(|(key, snapshot)| {
            let package = packages.get(&key.without_peer())?;
            let edges = all_entries(snapshot)
                .filter(|(alias, _)| is_optional_peer(package, alias))
                .filter_map(|(alias, dep_ref)| Some((alias, dep_ref.resolve(alias)?)))
                .collect::<Vec<_>>();
            (!edges.is_empty()).then_some((key, edges))
        })
        .collect()
}

fn is_optional_peer(package: &PackageMetadata, alias: &PkgName) -> bool {
    let alias = alias.to_string();
    let declared = package.peer_dependencies
        .as_ref()
        .is_some_and(|peers| peers.contains_key(&alias));
    declared
        && package.peer_dependencies_meta
            .as_ref()
            .and_then(|meta| meta.get(&alias))
            .is_some_and(|meta| meta.optional)
}

/// The sorted indices of the importers that list `target`. Targets with the
/// same listing share one reachability walk.
fn importers_listing(graph: &PeerEdgeGraph<'_>, target: &PackageKey) -> Vec<usize> {
    let root_lists = graph.listed_by_every_importer.contains(target);
    graph.importers
        .iter()
        .enumerate()
        .filter(|(_, keys)| root_lists || keys.contains(target))
        .map(|(index, _)| index)
        .collect()
}

/// Every snapshot the importers whose index is not in the sorted `listing`
/// reach through any snapshot entry.
fn reach_from_importers_not_in(
    graph: &PeerEdgeGraph<'_>,
    listing: &[usize],
) -> HashSet<PackageKey> {
    let mut reached = HashSet::new();
    let mut stack = graph.importers
        .iter()
        .enumerate()
        .filter(|(index, _)| listing.binary_search(index).is_err())
        .flat_map(|(_, keys)| keys.iter().cloned())
        .collect::<Vec<_>>();
    while let Some(key) = stack.pop() {
        if !reached.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = graph.snapshots.get(&key) else { continue };
        stack.extend(all_entries(snapshot).filter_map(|(alias, dep_ref)| dep_ref.resolve(alias)));
    }
    reached
}

fn all_entries(snapshot: &SnapshotEntry) -> impl Iterator<Item = (&PkgName, &SnapshotDepRef)> {
    snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
}

fn target_retained(
    snapshot: &SnapshotEntry,
    alias: &PkgName,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> bool {
    all_entries(snapshot)
        .filter(|(entry_alias, _)| *entry_alias == alias)
        .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
        .all(|target| snapshots.contains_key(&target))
}

fn remove_entries(snapshot: &mut SnapshotEntry, aliases: &[PkgName]) {
    for entries in [&mut snapshot.dependencies, &mut snapshot.optional_dependencies] {
        let Some(map) = entries.as_mut() else { continue };
        for alias in aliases {
            map.remove(alias);
        }
        if map.is_empty() {
            *entries = None;
        }
    }
}

#[cfg(test)]
mod tests;
