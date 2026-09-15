//! Shared depPath-compatibility helpers used by both
//! [`fn@crate::dedupe_peer_dependents::dedupe_peer_dependents`] and
//! [`fn@crate::dedupe_injected_deps::dedupe_injected_deps`].

use std::collections::VecDeque;

use pnpm_deps_path::DepPath;
use rustc_hash::FxHashSet as HashSet;

use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};

/// Number of edges a variant carries: its child dependencies plus the
/// peers it resolved against its ancestors.
pub(crate) fn node_deps_count(node: &DependenciesGraphNode) -> usize {
    node.edges.children.len() + node.edges.resolved_peer_names.len()
}

/// Whether `larger` can absorb `smaller`: it must have at least as many
/// deps, every peer `smaller` resolved must also be resolved by `larger`,
/// and each of `smaller`'s child aliases must resolve either to the same
/// depPath or to a variant of the same package that `larger`'s child can
/// absorb in turn. That last case is what lets a package whose child
/// carries a peer suffix absorb the variant whose child does not.
///
/// Compares dependency/peer *sets* only, not package identity, so callers
/// must pass depPaths already known to share a `pkgIdWithPatchHash` —
/// otherwise two unrelated leaf packages (both with empty sets) would
/// count as compatible.
///
/// A pair reached twice is taken as compatible, which is what terminates
/// dependency cycles.
pub(crate) fn is_compatible_and_has_more_deps<'a>(
    graph: &'a DependenciesGraph,
    larger: &'a DepPath,
    smaller: &'a DepPath,
) -> bool {
    let mut queue = PairQueue::new(larger, smaller);
    while let Some((larger, smaller)) = queue.pop() {
        if !pair_is_compatible(graph, larger, smaller, &mut queue) {
            return false;
        }
    }
    true
}

/// Pairs still to check, breadth-first, so the shallowest incompatible
/// pair settles the whole check. A pair is recorded when it enters, so an
/// edge that reaches one already queued adds nothing.
struct PairQueue<'a> {
    pending: VecDeque<(&'a DepPath, &'a DepPath)>,
    queued: HashSet<(&'a DepPath, &'a DepPath)>,
}

impl<'a> PairQueue<'a> {
    fn new(larger: &'a DepPath, smaller: &'a DepPath) -> Self {
        let mut queue = PairQueue { pending: VecDeque::new(), queued: HashSet::default() };
        queue.push(larger, smaller);
        queue
    }

    fn push(&mut self, larger: &'a DepPath, smaller: &'a DepPath) {
        if larger != smaller && self.queued.insert((larger, smaller)) {
            self.pending.push_back((larger, smaller));
        }
    }

    fn pop(&mut self) -> Option<(&'a DepPath, &'a DepPath)> {
        self.pending.pop_front()
    }
}

/// Checks one pair and queues the child pairs whose depPaths differ.
fn pair_is_compatible<'a>(
    graph: &'a DependenciesGraph,
    larger: &DepPath,
    smaller: &DepPath,
    queue: &mut PairQueue<'a>,
) -> bool {
    let (Some(larger_node), Some(smaller_node)) = (graph.get(larger), graph.get(smaller)) else {
        return false;
    };

    if node_deps_count(larger_node) < node_deps_count(smaller_node) {
        return false;
    }

    if !has_all_resolved_peers(larger_node, smaller_node) {
        return false;
    }

    smaller_node.edges.children
        .iter()
        .all(|(alias, smaller_child)| {
            queue_child_pair(graph, larger_node, alias, smaller_child, queue)
        })
}

fn has_all_resolved_peers(
    larger_node: &DependenciesGraphNode,
    smaller_node: &DependenciesGraphNode,
) -> bool {
    smaller_node.edges.resolved_peer_names
        .iter()
        .all(|peer| larger_node.edges.resolved_peer_names.contains(peer))
}

fn queue_child_pair<'a>(
    graph: &'a DependenciesGraph,
    larger_node: &'a DependenciesGraphNode,
    alias: &str,
    smaller_child: &'a DepPath,
    queue: &mut PairQueue<'a>,
) -> bool {
    let Some(larger_child) = larger_node.edges.children.get(alias) else {
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
    queue.push(larger_child, smaller_child);
    true
}
