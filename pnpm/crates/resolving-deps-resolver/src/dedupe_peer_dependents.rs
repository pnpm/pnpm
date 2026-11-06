//! Collapse of duplicate peer-dependent variants.
//!
//! Runs before [`fn@crate::dedupe_injected_deps::dedupe_injected_deps`]
//! in the multi-importer [`fn@crate::resolve_peers_workspace`] pass. When
//! the same `pkgIdWithPatchHash` resolved into several peer-suffixed variants
//! that differ only by which optional peers they picked up, a smaller
//! variant whose children + resolved peers are a subset of a larger,
//! compatible variant collapses into it: every reference to the smaller
//! depPath (graph child edges and importer direct deps) is rewritten to
//! the larger one, and the orphaned variant drops out of the lockfile.
//!
//! The collapse target is chosen by a total order over `(dep count, dep
//! path)` so it never depends on the order importers were resolved in —
//! the determinism fix from
//! [pnpm/pnpm#12179](https://github.com/pnpm/pnpm/pull/12179). A variant
//! that is a subset of two mutually incompatible larger variants of
//! equal size would otherwise collapse into whichever happened to be
//! visited first, producing machine-dependent lockfiles.

pub(crate) use survivor_names::PeerSuffixes;
pub(crate) use unbound_optional_peers::collect_unbound_optional_peers;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::collections::BTreeMap;

use pnpm_deps_path::DepPath;
use survivor_names::rename_survivors;

mod cycles;
mod survivor_names;
mod unbound_optional_peers;
use cycles::collapse_cycles;

use crate::{
    PeerDependentsDedupe,
    dedupe_injected_deps::{DirectByImporter, prune_unreachable},
    dep_path_compatibility::{is_compatible_and_has_more_deps, node_deps_count},
    dependencies_graph::DependenciesGraph,
    peer_dependents_dedupe::{CollapseGuard, OptionalPeerScope, UnboundOptionalPeers},
};

/// Collapse peer-dependent duplicate variants in `graph` into their
/// largest compatible sibling, rewriting every collapsed depPath in the
/// graph's child edges and in each importer's `direct` map.
///
/// A collapse retargets the peers of the nodes that depended on the
/// collapsed variant, and a depPath's peer suffix names its peers, so
/// those nodes are renamed after the peers they now resolve to.
pub(crate) fn dedupe_peer_dependents(
    graph: &mut DependenciesGraph,
    direct_by_importer: &mut DirectByImporter,
    peer_suffixes: &PeerSuffixes<'_>,
    dedupe: PeerDependentsDedupe,
    mut unbound_optional_peers: UnboundOptionalPeers,
) {
    if !dedupe.dedupes() {
        return;
    }
    let duplicates = collect_duplicates(graph);
    if duplicates.is_empty() {
        return;
    }
    let dep_paths_map = deduplicate_all(
        graph,
        &duplicates,
        dedupe.optional_peer_scope(),
        &mut unbound_optional_peers,
    );
    if dep_paths_map.is_empty() {
        return;
    }
    for direct in direct_by_importer.values_mut() {
        for dep_path in direct.values_mut() {
            if let Some(target) = dep_paths_map.get(dep_path) {
                *dep_path = target.clone();
            }
        }
    }
    // The collapsed variants now have no incoming edge (every graph child
    // edge and importer direct dep was rewritten above). Drop them so they
    // don't surface in the lockfile as orphans.
    prune_unreachable(graph, direct_by_importer);
    rename_survivors(graph, direct_by_importer, &dep_paths_map, peer_suffixes);
}

/// Group the graph's depPaths by their `pkgIdWithPatchHash` and keep the
/// groups with more than one variant. The grouping is reconstructed from
/// the finished graph (each node carries its `resolved_package_id`)
/// instead of threading a parallel map through the walk.
///
/// Groups are keyed through a `BTreeMap` only to make the outer order
/// stable for readers — group order does not affect the collapse, since
/// each `pkgIdWithPatchHash` group is processed independently. The order
/// *within* a group is left as the graph yields it; determinism comes
/// from the total-order sorter in [`deduplicate_dep_paths`], the fix this
/// module exists to port.
fn collect_duplicates(graph: &DependenciesGraph) -> Vec<Vec<DepPath>> {
    let mut by_pkg: BTreeMap<&str, Vec<DepPath>> = BTreeMap::new();
    for (dep_path, node) in graph {
        by_pkg
            .entry(node.resolved_package_id.as_str())
            .or_default()
            .push(dep_path.clone());
    }
    by_pkg
        .into_values()
        .filter(|variants| variants.len() > 1)
        .collect()
}

/// Run [`deduplicate_dep_paths`] in rounds: after each round, rewrite the
/// graph's child edges through the collapse map so the next round can see
/// newly-compatible variants in the next round.
///
/// A variant that absorbs one listed in the [`UnboundOptionalPeers`]
/// inherits its entry, so a later round cannot collapse it into a variant
/// that resolves the listed peer.
fn deduplicate_all(
    graph: &mut DependenciesGraph,
    duplicates: &[Vec<DepPath>],
    optional_peers: OptionalPeerScope,
    unbound_optional_peers: &mut UnboundOptionalPeers,
) -> HashMap<DepPath, DepPath> {
    let mut incoming = incoming_edges(graph);
    let mut remaining = duplicates.to_vec();
    let mut merged: HashMap<DepPath, DepPath> = HashMap::default();
    loop {
        let guard =
            CollapseGuard { optional_peers, unbound_optional_peers: Some(unbound_optional_peers) };
        let (next, survivors) = deduplicate_round(&remaining, graph, guard);
        if next.is_empty() {
            return merged;
        }
        rewrite_children(graph, &next, &mut incoming);
        transfer_unbound_optional_peers(unbound_optional_peers, &next);
        for target in merged.values_mut() {
            if let Some(replacement) = next.get(target) {
                *target = replacement.clone();
            }
        }
        merged.extend(next);
        remaining = survivors;
    }
}

fn deduplicate_round(
    duplicates: &[Vec<DepPath>],
    graph: &DependenciesGraph,
    guard: CollapseGuard<'_>,
) -> (HashMap<DepPath, DepPath>, Vec<Vec<DepPath>>) {
    let (next, mut survivors) = deduplicate_dep_paths(duplicates, graph, guard);
    if !next.is_empty() {
        return (next, survivors);
    }
    let next = collapse_cycles(graph, duplicates, guard);
    for group in &mut survivors {
        group.retain(|path| !next.contains_key(path));
    }
    (next, survivors)
}

fn transfer_unbound_optional_peers(
    unbound_optional_peers: &mut UnboundOptionalPeers,
    replacements: &HashMap<DepPath, DepPath>,
) {
    for (collapsed, target) in replacements {
        if let Some(names) = unbound_optional_peers.get(collapsed).cloned() {
            unbound_optional_peers
                .entry(target.clone())
                .or_default()
                .extend(names);
        }
    }
}

type IncomingEdges = HashMap<DepPath, Vec<(DepPath, String)>>;

fn incoming_edges(graph: &DependenciesGraph) -> IncomingEdges {
    let mut incoming = IncomingEdges::default();
    for (parent, node) in graph {
        for (alias, child) in &node.edges.children {
            incoming
                .entry(child.clone())
                .or_default()
                .push((parent.clone(), alias.clone()));
        }
    }
    incoming
}

fn rewrite_children(
    graph: &mut DependenciesGraph,
    replacements: &HashMap<DepPath, DepPath>,
    incoming: &mut IncomingEdges,
) {
    for (collapsed, target) in replacements {
        let Some(edges) = incoming.remove(collapsed) else { continue };
        for (parent, alias) in &edges {
            graph
                .get_mut(parent)
                .unwrap()
                .edges
                .children
                .insert(alias.clone(), target.clone());
        }
        incoming
            .entry(target.clone())
            .or_default()
            .extend(edges);
    }
}

/// One round of greedy collapse over each duplicate group.
///
/// Returns the `collapsed → target` map plus the groups that still hold
/// more than one un-collapsed variant, for the next round.
fn deduplicate_dep_paths(
    duplicates: &[Vec<DepPath>],
    graph: &DependenciesGraph,
    guard: CollapseGuard<'_>,
) -> (HashMap<DepPath, DepPath>, Vec<Vec<DepPath>>) {
    let mut dep_paths_map: HashMap<DepPath, DepPath> = HashMap::default();
    let mut remaining_duplicates: Vec<Vec<DepPath>> = Vec::new();

    for dep_paths in duplicates {
        let mut survivors: HashSet<DepPath> = dep_paths.iter().cloned().collect();
        let mut current = dep_paths.clone();
        sort_variants(graph, &mut current);

        while let Some(largest) = current.pop() {
            absorb_compatible(
                graph,
                guard,
                &largest,
                &mut current,
                &mut dep_paths_map,
                &mut survivors,
            );
            sort_variants(graph, &mut current);
        }

        if survivors.len() > 1 {
            let mut leftover: Vec<DepPath> = survivors.into_iter().collect();
            leftover.sort();
            remaining_duplicates.push(leftover);
        }
    }

    (dep_paths_map, remaining_duplicates)
}

fn sort_variants(graph: &DependenciesGraph, paths: &mut [DepPath]) {
    // Total order over `(dep count, dep path)`: the dep-count sort alone
    // is not a total order, so a variant that is a subset of several
    // incompatible larger variants of equal size would collapse into
    // whichever the unstable position happened to surface first. The
    // dep-path tie-break makes the winner machine-independent
    // (pnpm/pnpm#12179).
    paths.sort_by(|dep_path1, dep_path2| {
        node_deps_count(&graph[dep_path1])
            .cmp(&node_deps_count(&graph[dep_path2]))
            .then_with(|| dep_path1.cmp(dep_path2))
    });
}

/// Absorb every remaining variant that `largest` subsumes, leaving the rest
/// in `current` for the next round.
fn absorb_compatible(
    graph: &DependenciesGraph,
    guard: CollapseGuard<'_>,
    largest: &DepPath,
    current: &mut Vec<DepPath>,
    dep_paths_map: &mut HashMap<DepPath, DepPath>,
    survivors: &mut HashSet<DepPath>,
) {
    let mut next = Vec::new();
    while let Some(candidate) = current.pop() {
        // Child variants must agree before a parent can collapse into its sibling.
        let same_children = graph[&candidate].edges.children
            .iter()
            .all(|(alias, child)| graph[largest].edges.children.get(alias) == Some(child));
        if same_children && is_compatible_and_has_more_deps(graph, largest, &candidate, guard) {
            dep_paths_map.insert(candidate.clone(), largest.clone());
            survivors.remove(&candidate);
        } else {
            next.push(candidate);
        }
    }
    *current = next;
}

#[cfg(test)]
mod tests;
