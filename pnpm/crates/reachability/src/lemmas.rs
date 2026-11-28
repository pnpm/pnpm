//! The lemmas that show the search marks exactly the nodes with a path to a
//! target.

use super::logic::{
    closed_except,
    edge,
    ends_at_target,
    has,
    is_path,
    reaches_target,
    well_formed,
};
use creusot_std::{
    logic::such_that,
    prelude::{
        Int,
        Seq,
        ensures,
        logic,
        requires,
        variant,
    },
};

/// `list` with `last` appended holds what `list` held, and `last`.
#[logic]
#[ensures(forall<value: Int> has(list.snoc(last), value)
    == (has(list, value) || last@ == value))]
pub fn has_snoc(list: Seq<usize>, last: usize) {}

/// `list` without its last element holds what `list` held but the last,
/// unless an earlier element is the last too.
#[logic]
#[requires(list.len() > 0)]
#[ensures(forall<value: Int> has(list.subsequence(0, list.len() - 1), value) ==> has(list, value))]
#[ensures(forall<value: Int> has(list, value) && value != list[list.len() - 1]@
    ==> has(list.subsequence(0, list.len() - 1), value))]
pub fn has_init(list: Seq<usize>) {}

/// A path that starts with an edge into another path is a path.
#[logic]
#[requires(is_path(adjacency, path) && edge(adjacency, from, path[0]))]
#[ensures(is_path(adjacency, Seq::singleton(from).concat(path)))]
#[ensures(Seq::singleton(from).concat(path)[0] == from)]
#[ensures(ends_at_target(targets, path) ==> ends_at_target(targets, Seq::singleton(from).concat(path)))]
pub fn prepend(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, from: Int, path: Seq<Int>) {}

/// A node with an edge to a node that reaches a target reaches it too.
#[logic]
#[requires(edge(adjacency, from, to) && reaches_target(adjacency, targets, to))]
#[ensures(reaches_target(adjacency, targets, from))]
pub fn extend_back(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, from: Int, to: Int) {
    pearlite! {
        prepend(adjacency, targets, from, such_that(|path: Seq<Int>| is_path(adjacency, path)
            && path[0] == to && ends_at_target(targets, path)))
    }
}

/// The tail of a path of two or more nodes is a path to the same end.
#[logic]
#[requires(is_path(adjacency, path) && path.len() > 1)]
#[ensures(is_path(adjacency, path.tail()))]
#[ensures(path.tail()[0] == path[1])]
#[ensures(ends_at_target(targets, path) ==> ends_at_target(targets, path.tail()))]
pub fn path_tail(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, path: Seq<Int>) {}

/// When the marked nodes hold every target and every predecessor of a
/// marked node, a path to a target starts at a marked node.
#[logic]
#[variant(path.len())]
#[requires(closed_except(adjacency, marked, Seq::empty(), -1))]
#[requires(forall<node: Int> 0 <= node && node < targets.len() && targets[node] ==> marked[node])]
#[requires(targets.len() == marked.len() && well_formed(adjacency, targets))]
#[requires(is_path(adjacency, path) && ends_at_target(targets, path))]
#[ensures(0 <= path[0] && path[0] < marked.len() && marked[path[0]])]
pub fn marked_start(
    adjacency: Seq<Vec<usize>>,
    targets: Seq<bool>,
    marked: Seq<bool>,
    path: Seq<Int>,
) {
    pearlite! {
        if path.len() > 1 {
            path_tail(adjacency, targets, path);
            marked_start(adjacency, targets, marked, path.tail())
        }
    }
}

/// [`marked_start`] for a node: every node that reaches a target is marked.
#[logic]
#[requires(closed_except(adjacency, marked, Seq::empty(), -1))]
#[requires(forall<node: Int> 0 <= node && node < targets.len() && targets[node] ==> marked[node])]
#[requires(targets.len() == marked.len() && well_formed(adjacency, targets))]
#[requires(reaches_target(adjacency, targets, node))]
#[ensures(0 <= node && node < marked.len() && marked[node])]
pub fn marked_if_reaching(
    adjacency: Seq<Vec<usize>>,
    targets: Seq<bool>,
    marked: Seq<bool>,
    node: Int,
) {
    pearlite! {
        marked_start(adjacency, targets, marked, such_that(|path: Seq<Int>| is_path(adjacency, path)
            && path[0] == node && ends_at_target(targets, path)))
    }
}

/// A target reaches itself.
#[logic]
#[requires(0 <= node && node < targets.len() && targets[node])]
#[ensures(reaches_target(adjacency, targets, node))]
pub fn target_reaches(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, node: Int) {
    pearlite! { singleton_path(adjacency, node) }
}

/// A single node is a path.
#[logic]
#[ensures(is_path(adjacency, Seq::singleton(node)))]
#[ensures(Seq::singleton(node)[0] == node && Seq::singleton(node).len() == 1)]
pub fn singleton_path(adjacency: Seq<Vec<usize>>, node: Int) {}

/// A list holds each of its elements.
#[logic]
#[requires(0 <= index && index < list.len())]
#[ensures(has(list, list[index]@))]
pub fn has_element(list: Seq<usize>, index: Int) {}

/// Flagging more nodes keeps every node that reached a target reaching one.
#[logic]
#[requires(targets.len() == more.len())]
#[requires(forall<node: Int> 0 <= node && node < targets.len() && targets[node] ==> more[node])]
#[ensures(forall<node: Int> reaches_target(adjacency, targets, node)
    ==> reaches_target(adjacency, more, node))]
pub fn more_targets(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, more: Seq<bool>) {}

/// From a node with no edges, every path is that node alone.
#[logic]
#[requires(0 <= node && node < adjacency.len() && adjacency[node]@.len() == 0)]
#[ensures(reaches_target(adjacency, targets, node)
    == (0 <= node && node < targets.len() && targets[node]))]
pub fn no_edges(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, node: Int) {
    pearlite! { target_reaches_if(adjacency, targets, node) }
}

/// [`target_reaches`] where `node` may not be a target.
#[logic]
#[ensures(0 <= node && node < targets.len() && targets[node]
    ==> reaches_target(adjacency, targets, node))]
pub fn target_reaches_if(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, node: Int) {
    pearlite! { singleton_path(adjacency, node) }
}
