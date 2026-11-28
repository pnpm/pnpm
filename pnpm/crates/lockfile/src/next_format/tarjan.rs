//! Strongly connected components, for the peer cycles the resolver collapses.

use pnpm_strongly_connected_components::StronglyConnectedComponents;
use std::collections::HashMap;

/// Every node in a component, ordered so that a component comes after every
/// component it has an edge to. Nodes that don't participate come first, each
/// on its own, and have no entry in `component_of`.
pub(super) struct Components {
    pub components: Vec<Vec<usize>>,
    pub component_of: HashMap<usize, usize>,
}

/// The components of the participating nodes of `0..count`. Edges to
/// non-participating nodes are ignored.
pub(super) fn components(
    count: usize,
    neighbors: impl Fn(usize) -> Vec<usize>,
    participates: impl Fn(usize) -> bool,
) -> Components {
    let removed: Vec<bool> = (0..count)
        .map(|node| !participates(node))
        .collect();
    let adjacency: Vec<Vec<usize>> = (0..count)
        .map(|node| if removed[node] { Vec::new() } else { neighbors(node) })
        .collect();
    let found = StronglyConnectedComponents::compute(&adjacency, &removed);
    let mut components: Vec<Vec<usize>> = (0..count)
        .filter(|&node| removed[node])
        .map(|node| vec![node])
        .collect();
    let offset = components.len();
    components.extend_from_slice(found.components());
    let component_of = (0..count)
        .filter(|&node| !removed[node])
        .map(|node| (node, found.component_ids()[node] + offset))
        .collect();
    Components { components, component_of }
}
