//! Shared depPath-compatibility helpers used by both
//! [`fn@crate::dedupe_peer_dependents::dedupe_peer_dependents`] and
//! [`fn@crate::dedupe_injected_deps::dedupe_injected_deps`].

use pnpm_deps_path::DepPath;
use rustc_hash::FxHashSet as HashSet;

use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};

/// Number of edges a variant carries: its child dependencies plus the
/// peers it resolved against its ancestors.
pub(crate) fn node_deps_count(node: &DependenciesGraphNode) -> usize {
    node.children.len() + node.resolved_peer_names.len()
}

/// Whether `larger` can absorb `smaller`: it must have at least as many
/// deps, every one of `smaller`'s child depPaths must appear among
/// `larger`'s children, and every peer `smaller` resolved must also be
/// resolved by `larger`.
///
/// Compares dependency/peer *sets* only, not package identity, so callers
/// must pass depPaths already known to share a `pkgIdWithPatchHash` —
/// otherwise two unrelated leaf packages (both with empty sets) would
/// count as compatible.
pub(crate) fn is_compatible_and_has_more_deps(
    graph: &DependenciesGraph,
    larger: &DepPath,
    smaller: &DepPath,
) -> bool {
    let mut visited = HashSet::default();
    is_compatible_and_has_more_deps_helper(graph, larger, smaller, &mut visited)
}

fn is_compatible_and_has_more_deps_helper<'a>(
    graph: &'a DependenciesGraph,
    larger: &'a DepPath,
    smaller: &'a DepPath,
    visited: &mut HashSet<(&'a DepPath, &'a DepPath)>,
) -> bool {
    if larger == smaller {
        return true;
    }
    if !visited.insert((larger, smaller)) {
        return true;
    }

    let Some(larger_node) = graph.get(larger) else { return false; };
    let Some(smaller_node) = graph.get(smaller) else { return false; };

    if node_deps_count(larger_node) < node_deps_count(smaller_node) {
        return false;
    }

    if !smaller_node
        .resolved_peer_names
        .iter()
        .all(|peer| larger_node.resolved_peer_names.contains(peer))
    {
        return false;
    }

    for (alias, smaller_child) in &smaller_node.children {
        let Some(larger_child) = larger_node.children.get(alias) else {
            return false;
        };
        if larger_child == smaller_child {
            continue;
        }
        let Some(larger_child_node) = graph.get(larger_child) else {
            return false;
        };
        if let Some(smaller_child_node) = graph.get(smaller_child) {
            if larger_child_node.resolved_package_id != smaller_child_node.resolved_package_id {
                return false;
            }
        } else {
            return false;
        }
        if !is_compatible_and_has_more_deps_helper(graph, larger_child, smaller_child, visited) {
            return false;
        }
    }

    true
}
