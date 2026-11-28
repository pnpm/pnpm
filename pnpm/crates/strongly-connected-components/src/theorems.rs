//! Properties stated for every graph, each as a verified caller. Creusot
//! proving one is a test passing on all inputs at once.

use crate::{
    StronglyConnectedComponents,
    logic::{
        none,
        well_formed,
    },
};
use creusot_std::prelude::{
    ensures,
    requires,
};

/// Every node of a [`StronglyConnectedComponents`] is labelled removed or
/// with a component below the count. The type invariant carries this, so a
/// caller that holds the value needs no precondition to rely on it.
#[requires(node@ < components.ids().len())]
#[ensures(components.ids()[node@]@ == none() || components.ids()[node@]@ < components.count())]
pub fn every_label_is_removed_or_a_component(
    components: &StronglyConnectedComponents,
    node: usize,
) {
}

/// With nothing removed, every node has a component below the count. The
/// audit graph indexes per-component tables by `component_ids` on that
/// promise.
#[requires(well_formed(adjacency@, removed@))]
#[requires(forall<node> 0 <= node && node < removed@.len() ==> !removed@[node])]
#[ensures(forall<node> 0 <= node && node < adjacency@.len() ==>
    result.ids()[node]@ < result.count())]
#[ensures(result.count() <= adjacency@.len())]
pub fn nothing_removed_labels_every_node(
    adjacency: &[Vec<usize>],
    removed: &[bool],
) -> StronglyConnectedComponents {
    StronglyConnectedComponents::compute(adjacency, removed)
}

/// A node left in after more removals can still be asked whether it may
/// lie on a cycle. The task-graph sequencer asks while it removes the
/// members of each cycle it breaks.
#[requires(well_formed(adjacency@, removed_then@))]
#[requires(removed_now@.len() == removed_then@.len())]
#[requires(forall<node> 0 <= node && node < removed_then@.len() && removed_then@[node] ==>
    removed_now@[node])]
#[requires(node@ < adjacency@.len() && !removed_now@[node@])]
pub fn later_removals_keep_the_cycle_filter_callable(
    adjacency: &[Vec<usize>],
    removed_then: &[bool],
    removed_now: &[bool],
    node: usize,
) -> bool {
    StronglyConnectedComponents::compute(adjacency, removed_then).may_lie_on_cycle(node, adjacency)
}

/// A node with an edge to itself may lie on a cycle.
#[requires(well_formed(adjacency@, removed@))]
#[requires(node@ < adjacency@.len() && !removed@[node@])]
#[requires(adjacency@[node@]@.contains(node))]
#[ensures(result)]
pub fn self_loop_may_lie_on_cycle(adjacency: &[Vec<usize>], removed: &[bool], node: usize) -> bool {
    StronglyConnectedComponents::compute(adjacency, removed).may_lie_on_cycle(node, adjacency)
}

/// A removed node shares a component only with other removed nodes.
#[requires(well_formed(adjacency@, removed@))]
#[requires(left@ < adjacency@.len() && right@ < adjacency@.len())]
#[requires(removed@[left@] && !removed@[right@])]
#[ensures(!result)]
pub fn removed_node_shares_nothing_with_an_included_one(
    adjacency: &[Vec<usize>],
    removed: &[bool],
    left: usize,
    right: usize,
) -> bool {
    let components = StronglyConnectedComponents::compute(adjacency, removed);
    creusot_std::prelude::proof_assert!(components.ids()[left@]@ == none());
    components.shares_component(left, right)
}
