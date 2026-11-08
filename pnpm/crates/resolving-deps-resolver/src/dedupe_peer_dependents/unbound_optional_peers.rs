//! The variants an importer's rejected optional peers must stay unbound in.

use super::{DepPath, DependenciesGraph, DirectByImporter, HashSet, UnboundOptionalPeers};

/// Every variant in an importer's closure that leaves unbound one of the
/// optional peers in `rejected_by_importer`, with the names it leaves
/// unbound.
pub(crate) fn collect_unbound_optional_peers<'a>(
    graph: &DependenciesGraph,
    direct_by_importer: &DirectByImporter,
    rejected_by_importer: impl IntoIterator<Item = (&'a str, &'a HashSet<String>)>,
) -> UnboundOptionalPeers {
    let mut unbound = UnboundOptionalPeers::default();
    for (importer_id, rejected) in rejected_by_importer {
        if rejected.is_empty() {
            continue;
        }
        let Some(direct) = direct_by_importer.get(importer_id) else { continue };
        for dep_path in closure(graph, direct.values()) {
            let names = unbound_peer_names(graph, dep_path, rejected);
            if !names.is_empty() {
                unbound
                    .entry(dep_path.clone())
                    .or_default()
                    .extend(names);
            }
        }
    }
    unbound
}

fn closure<'a>(
    graph: &'a DependenciesGraph,
    roots: impl Iterator<Item = &'a DepPath>,
) -> HashSet<&'a DepPath> {
    let mut pending: Vec<&DepPath> = roots.collect();
    let mut visited: HashSet<&DepPath> = HashSet::default();
    while let Some(dep_path) = pending.pop() {
        let Some(node) = graph.get(dep_path) else { continue };
        if visited.insert(dep_path) {
            pending.extend(node.edges.children.values());
        }
    }
    visited
}

fn unbound_peer_names(
    graph: &DependenciesGraph,
    dep_path: &DepPath,
    rejected: &HashSet<String>,
) -> Vec<String> {
    let edges = &graph[dep_path].edges;
    edges.peer_set
        .taken()
        .iter()
        .filter(|(name, peer)| {
            peer.optional
                && rejected.contains(*name)
                && !edges.resolved_peer_names.contains(*name)
        })
        .map(|(name, _)| name.clone())
        .collect()
}
