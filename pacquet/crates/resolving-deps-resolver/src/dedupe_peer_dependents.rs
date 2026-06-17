//! Port of pnpm's `dedupePeerDependents` collapse, the
//! [tail block of `resolvePeers`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L214-L222)
//! and its helpers
//! [`deduplicateAll`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L235-L257)
//! /
//! [`deduplicateDepPaths`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L264-L310).
//!
//! Runs after [`fn@crate::dedupe_injected_deps::dedupe_injected_deps`]
//! in the multi-importer [`fn@crate::resolve_peers_workspace`] pass. When
//! the same
//! `pkgIdWithPatchHash` resolved into several peer-suffixed variants
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

use std::collections::{BTreeMap, HashMap, HashSet};

use pacquet_deps_path::{DepPath, index_of_dep_path_suffix};

use crate::{
    dedupe_injected_deps::{DirectByImporter, prune_unreachable},
    dependencies_graph::{DependenciesGraph, DependenciesGraphNode},
};

/// Collapse peer-dependent duplicate variants in `graph` into their
/// largest compatible sibling, rewriting every collapsed depPath in the
/// graph's child edges and in each importer's `direct` map.
pub fn dedupe_peer_dependents(
    graph: &mut DependenciesGraph,
    direct_by_importer: &mut DirectByImporter,
) {
    let duplicates = collect_duplicates(graph);
    if duplicates.is_empty() {
        return;
    }
    let dep_paths_map = deduplicate_all(graph, &duplicates);
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
}

/// Group the graph's depPaths by their `pkgIdWithPatchHash` and keep the
/// groups with more than one variant. Mirrors upstream's
/// [`depPathsByPkgId` filtered to `size > 1`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L215);
/// pacquet reconstructs the grouping from the finished graph (each node
/// carries its `resolved_package_id`) instead of threading a parallel
/// map through the walk.
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
        by_pkg.entry(node.resolved_package_id.as_str()).or_default().push(dep_path.clone());
    }
    by_pkg.into_values().filter(|variants| variants.len() > 1).collect()
}

/// Run [`deduplicate_dep_paths`] in rounds: after each round, rewrite the
/// graph's child edges through the collapse map so the next round can see
/// newly-compatible variants, then recurse on the duplicates that didn't
/// collapse. Mirrors upstream's
/// [`deduplicateAll`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L235-L257).
fn deduplicate_all(
    graph: &mut DependenciesGraph,
    duplicates: &[Vec<DepPath>],
) -> HashMap<DepPath, DepPath> {
    let duplicates_count = duplicates.len();
    let (dep_paths_map, remaining_duplicates) = deduplicate_dep_paths(duplicates, graph);
    if remaining_duplicates.len() == duplicates_count {
        return dep_paths_map;
    }
    let collapsed_target_peer_info = collapsed_target_peer_info(graph, dep_paths_map.values());
    for node in graph.values_mut() {
        let parent_peer_names = compatible_peer_names_for_parent(node);
        for child_dep_path in node.children.values_mut() {
            if let Some(target) = dep_paths_map.get(child_dep_path)
                && collapsed_target_matches_parent(
                    &collapsed_target_peer_info,
                    target,
                    &parent_peer_names,
                )
            {
                *child_dep_path = target.clone();
            }
        }
    }
    if dep_paths_map.is_empty() {
        return dep_paths_map;
    }
    let mut merged = dep_paths_map;
    for (collapsed, target) in deduplicate_all(graph, &remaining_duplicates) {
        merged.insert(collapsed, target);
    }
    merged
}

/// One round of greedy collapse over each duplicate group. Mirrors
/// upstream's
/// [`deduplicateDepPaths`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L264-L310).
///
/// Returns the `collapsed → target` map plus the groups that still hold
/// more than one un-collapsed variant, for the next round.
fn deduplicate_dep_paths(
    duplicates: &[Vec<DepPath>],
    graph: &DependenciesGraph,
) -> (HashMap<DepPath, DepPath>, Vec<Vec<DepPath>>) {
    // Total order over `(dep count, dep path)`: the dep-count sort alone
    // is not a total order, so a variant that is a subset of several
    // incompatible larger variants of equal size would collapse into
    // whichever the unstable position happened to surface first. The
    // dep-path tie-break makes the winner machine-independent
    // (pnpm/pnpm#12179).
    let dep_count_sorter = |dep_path1: &DepPath, dep_path2: &DepPath| {
        node_deps_count(&graph[dep_path1])
            .cmp(&node_deps_count(&graph[dep_path2]))
            .then_with(|| dep_path1.cmp(dep_path2))
    };

    let mut dep_paths_map: HashMap<DepPath, DepPath> = HashMap::new();
    let mut remaining_duplicates: Vec<Vec<DepPath>> = Vec::new();

    for dep_paths in duplicates {
        let mut unresolved: HashSet<DepPath> = dep_paths.iter().cloned().collect();
        let mut current = dep_paths.clone();
        current.sort_by(dep_count_sorter);

        while let Some(largest) = current.pop() {
            let mut next = Vec::new();
            while let Some(candidate) = current.pop() {
                if is_compatible_and_has_more_deps(graph, &largest, &candidate) {
                    dep_paths_map.insert(candidate.clone(), largest.clone());
                    unresolved.remove(&largest);
                    unresolved.remove(&candidate);
                } else {
                    next.push(candidate);
                }
            }
            current = next;
            current.sort_by(dep_count_sorter);
        }

        if !unresolved.is_empty() {
            let mut leftover: Vec<DepPath> = unresolved.into_iter().collect();
            leftover.sort();
            remaining_duplicates.push(leftover);
        }
    }

    (dep_paths_map, remaining_duplicates)
}

/// Number of edges a variant carries: its child dependencies plus the
/// peers it resolved against its ancestors. Mirrors upstream's
/// [`nodeDepsCount`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L231-L233).
fn node_deps_count(node: &DependenciesGraphNode) -> usize {
    node.children.len() + node.resolved_peer_names.len()
}

/// Whether `larger` can absorb `smaller`: it must have at least as many
/// deps, every one of `smaller`'s child depPaths must appear among
/// `larger`'s children, and every peer `smaller` resolved must also be
/// resolved by `larger`. Mirrors upstream's
/// [`isCompatibleAndHasMoreDeps`](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/src/resolvePeers.ts#L312-L329).
fn is_compatible_and_has_more_deps(
    graph: &DependenciesGraph,
    larger: &DepPath,
    smaller: &DepPath,
) -> bool {
    let larger_node = &graph[larger];
    let smaller_node = &graph[smaller];
    if node_deps_count(larger_node) < node_deps_count(smaller_node) {
        return false;
    }

    let larger_children: HashSet<&DepPath> = larger_node.children.values().collect();
    if !smaller_node.children.values().all(|child| larger_children.contains(child)) {
        return false;
    }

    smaller_node
        .resolved_peer_names
        .iter()
        .all(|peer| larger_node.resolved_peer_names.contains(peer))
}

fn collapsed_target_matches_parent(
    collapsed_target_peer_info: &HashMap<DepPath, CollapsedTargetPeerInfo>,
    target: &DepPath,
    parent_peer_names: &HashSet<String>,
) -> bool {
    let Some(info) = collapsed_target_peer_info.get(target) else {
        return true;
    };
    info.peer_names.iter().all(|name| {
        parent_peer_names.contains(name)
            || info.transitive_peer_dependencies.contains(name)
            || info.peer_child_is_compatible_with_parent(name, parent_peer_names)
    })
}

fn collapsed_target_peer_info<'a>(
    graph: &DependenciesGraph,
    targets: impl Iterator<Item = &'a DepPath>,
) -> HashMap<DepPath, CollapsedTargetPeerInfo> {
    targets
        .filter_map(|target| {
            let node = graph.get(target)?;
            Some((
                target.clone(),
                CollapsedTargetPeerInfo {
                    package_name: package_name_from_pkg_id(&node.resolved_package_id),
                    peer_names: dep_path_peer_names(target),
                    transitive_peer_dependencies: node.transitive_peer_dependencies.clone(),
                    child_peer_names: child_peer_names(node),
                },
            ))
        })
        .collect()
}

struct CollapsedTargetPeerInfo {
    package_name: String,
    peer_names: HashSet<String>,
    transitive_peer_dependencies: HashSet<String>,
    child_peer_names: HashMap<String, HashSet<String>>,
}

impl CollapsedTargetPeerInfo {
    fn peer_child_is_compatible_with_parent(
        &self,
        name: &str,
        parent_peer_names: &HashSet<String>,
    ) -> bool {
        self.child_peer_names.get(name).is_some_and(|peer_names| {
            peer_names.is_empty()
                || (peer_names.contains(&self.package_name)
                    && peer_names.iter().all(|peer_name| {
                        peer_name == &self.package_name
                            || parent_peer_names.contains(peer_name)
                            || self.transitive_peer_dependencies.contains(peer_name)
                    }))
        })
    }
}

fn child_peer_names(node: &DependenciesGraphNode) -> HashMap<String, HashSet<String>> {
    node.children.iter().map(|(alias, child)| (alias.clone(), dep_path_peer_names(child))).collect()
}

fn compatible_peer_names_for_parent(node: &DependenciesGraphNode) -> HashSet<String> {
    let mut names = dep_path_peer_names(&node.dep_path);
    names.insert(package_name_from_pkg_id(&node.resolved_package_id));
    names
}

fn package_name_from_pkg_id(pkg_id: &str) -> String {
    pkg_id.rfind('@').map_or_else(|| pkg_id.to_string(), |index| pkg_id[..index].to_string())
}

fn dep_path_peer_names(dep_path: &DepPath) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_peer_names(dep_path.as_str(), &mut names);
    names
}

fn collect_peer_names(raw: &str, names: &mut HashSet<String>) {
    let suffix = index_of_dep_path_suffix(raw);
    let Some(peers_index) = suffix.peers_index else { return };
    let peers_end = suffix.patch_hash_index.unwrap_or(raw.len());
    let Some(segments) = split_peer_suffix_segments(&raw[peers_index..peers_end]) else {
        return;
    };
    for segment in segments {
        if let Some(name) = peer_segment_name(&segment) {
            names.insert(name.to_string());
        }
        collect_peer_names(&segment, names);
    }
}

fn split_peer_suffix_segments(suffix: &str) -> Option<Vec<String>> {
    let bytes = suffix.as_bytes();
    let mut segments = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (idx, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' => {
                if depth == 0 {
                    start = Some(idx + 1);
                }
                depth += 1;
            }
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                if depth == 0 {
                    let start = start.take()?;
                    segments.push(suffix[start..idx].to_string());
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(segments)
}

fn peer_segment_name(segment: &str) -> Option<&str> {
    let head_end = segment.find('(').unwrap_or(segment.len());
    let head = &segment[..head_end];
    let version_at = head.rfind('@')?;
    (version_at > 0).then_some(&head[..version_at])
}

#[cfg(test)]
mod tests;
