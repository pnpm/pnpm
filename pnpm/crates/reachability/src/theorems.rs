//! Properties stated for every graph, each as a verified caller. Creusot
//! proving one is a test passing on all inputs at once.

use crate::{
    reaching,
    spec::{
        edge,
        well_formed,
    },
};
use creusot_std::prelude::{
    Int,
    ensures,
    proof_assert,
    requires,
};

/// A flagged node reaches a flagged node: itself.
#[requires(well_formed(adjacency@, targets@))]
#[requires(node@ < targets@.len() && targets@[node@])]
#[ensures(result)]
pub fn a_target_reaches_itself(adjacency: &[Vec<usize>], targets: &[bool], node: usize) -> bool {
    proof_assert! { crate::spec::target_reaches(adjacency@, targets@, node@); true };
    reaching(adjacency, targets)[node]
}

/// With nothing flagged, no node reaches a flagged node.
#[requires(well_formed(adjacency@, targets@))]
#[requires(forall<node: Int> 0 <= node && node < targets@.len() ==> !targets@[node])]
#[ensures(forall<node: Int> 0 <= node && node < result@.len() ==> !result@[node])]
pub fn nothing_flagged_reaches_nothing(adjacency: &[Vec<usize>], targets: &[bool]) -> Vec<bool> {
    reaching(adjacency, targets)
}

/// A node with an edge to a node that reaches a flagged node reaches it
/// too, so the answer a node gets bounds the answers of its predecessors.
#[requires(well_formed(adjacency@, targets@))]
#[requires(edge(adjacency@, from@, to@))]
#[ensures(result.1 ==> result.0)]
pub fn an_edge_carries_the_answer_back(
    adjacency: &[Vec<usize>],
    targets: &[bool],
    from: usize,
    to: usize,
) -> (bool, bool) {
    let reaches = reaching(adjacency, targets);
    proof_assert! {
        crate::spec::has_element(adjacency@[from@]@, 0);
        to@ < adjacency@.len() && from@ < adjacency@.len()
    };
    proof_assert! {
        reaches@[to@] ==> { crate::spec::extend_back(adjacency@, targets@, from@, to@); reaches@[from@] }
    };
    (reaches[from], reaches[to])
}

/// Two nodes with edges to each other share one answer, and so do all the
/// members of a cycle.
#[requires(well_formed(adjacency@, targets@))]
#[requires(edge(adjacency@, left@, right@) && edge(adjacency@, right@, left@))]
#[ensures(result.0 == result.1)]
pub fn a_cycle_shares_one_answer(
    adjacency: &[Vec<usize>],
    targets: &[bool],
    left: usize,
    right: usize,
) -> (bool, bool) {
    let reaches = reaching(adjacency, targets);
    proof_assert! {
        (reaches@[right@] ==> { crate::spec::extend_back(adjacency@, targets@, left@, right@); reaches@[left@] })
            && (reaches@[left@] ==> { crate::spec::extend_back(adjacency@, targets@, right@, left@); reaches@[right@] })
    };
    (reaches[left], reaches[right])
}

/// A node with no edges reaches a flagged node exactly when it is flagged.
#[requires(well_formed(adjacency@, targets@))]
#[requires(node@ < adjacency@.len() && adjacency@[node@]@.len() == 0)]
#[ensures(result == targets@[node@])]
pub fn a_node_with_no_edges_reaches_only_itself(
    adjacency: &[Vec<usize>],
    targets: &[bool],
    node: usize,
) -> bool {
    proof_assert! { crate::spec::no_edges(adjacency@, targets@, node@); true };
    reaching(adjacency, targets)[node]
}

/// Flagging more nodes never takes an answer away.
#[requires(well_formed(adjacency@, targets@) && well_formed(adjacency@, more@))]
#[requires(forall<node: Int> 0 <= node && node < targets@.len() && targets@[node] ==> more@[node])]
#[ensures(forall<node: Int> 0 <= node && node < result.0@.len() && result.0@[node] ==> result.1@[node])]
pub fn more_flags_never_take_an_answer_away(
    adjacency: &[Vec<usize>],
    targets: &[bool],
    more: &[bool],
) -> (Vec<bool>, Vec<bool>) {
    proof_assert! { crate::spec::more_targets(adjacency@, targets@, more@); true };
    (reaching(adjacency, targets), reaching(adjacency, more))
}
