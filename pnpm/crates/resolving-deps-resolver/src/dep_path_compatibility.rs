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
    if larger == smaller || !visited.insert((larger, smaller)) {
        return true;
    }

    let (Some(larger_node), Some(smaller_node)) = (graph.get(larger), graph.get(smaller)) else {
        return false;
    };

    if node_deps_count(larger_node) < node_deps_count(smaller_node) {
        return false;
    }

    if !has_all_resolved_peers(larger_node, smaller_node) {
        return false;
    }

    child_deps_are_compatible(graph, larger_node, smaller_node, visited)
}

fn has_all_resolved_peers(
    larger_node: &DependenciesGraphNode,
    smaller_node: &DependenciesGraphNode,
) -> bool {
    smaller_node
        .resolved_peer_names
        .iter()
        .all(|peer| larger_node.resolved_peer_names.contains(peer))
}

fn child_deps_are_compatible<'a>(
    graph: &'a DependenciesGraph,
    larger_node: &'a DependenciesGraphNode,
    smaller_node: &'a DependenciesGraphNode,
    visited: &mut HashSet<(&'a DepPath, &'a DepPath)>,
) -> bool {
    smaller_node.children.iter().all(|(alias, smaller_child)| {
        are_child_deps_compatible(graph, larger_node, alias, smaller_child, visited)
    })
}

fn are_child_deps_compatible<'a>(
    graph: &'a DependenciesGraph,
    larger_node: &'a DependenciesGraphNode,
    alias: &str,
    smaller_child: &'a DepPath,
    visited: &mut HashSet<(&'a DepPath, &'a DepPath)>,
) -> bool {
    let Some(larger_child) = larger_node.children.get(alias) else {
        return false;
    };
    if larger_child == smaller_child {
        return true;
    }
    let (Some(larger_child_node), Some(smaller_child_node)) =
        (graph.get(larger_child), graph.get(smaller_child))
    else {
        return false;
    };
    if larger_child_node.resolved_package_id != smaller_child_node.resolved_package_id {
        return false;
    }
    is_compatible_and_has_more_deps_helper(graph, larger_child, smaller_child, visited)
}
