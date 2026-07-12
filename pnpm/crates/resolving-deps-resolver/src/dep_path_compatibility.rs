//! Shared depPath-compatibility helpers used by both
//! [`fn@crate::dedupe_peer_dependents::dedupe_peer_dependents`] and
//! [`fn@crate::dedupe_injected_deps::dedupe_injected_deps`]. They live in
//! their own module so the two consumers share one implementation rather
//! than each carrying a copy.

use std::collections::HashSet;

use pacquet_deps_path::DepPath;

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
/// IMPORTANT: this only compares dependency/peer *sets*, not package
/// identity — two different packages (or two versions of the same
/// package) with compatible dependency sets, e.g. leaf nodes with none,
/// would be considered compatible. Callers must therefore only compare
/// depPaths already known to share the same package identity
/// (`pkgIdWithPatchHash`). In `dedupe_peer_dependents` that holds because
/// the candidates are grouped by `pkgIdWithPatchHash`;
/// `dedupe_injected_deps` enforces it explicitly before calling this.
pub(crate) fn is_compatible_and_has_more_deps(
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
