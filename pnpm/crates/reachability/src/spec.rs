//! Paths through a graph given as adjacency lists, and the lemmas that show
//! the search marks exactly the nodes with a path to a target.

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

/// Whether `list` holds `value`.
#[logic(open)]
pub fn has(list: Seq<usize>, value: Int) -> bool {
    pearlite! { exists<index: Int> 0 <= index && index < list.len() && list[index]@ == value }
}

/// Whether `adjacency` has an edge from `from` to `to`.
#[logic(open)]
pub fn edge(adjacency: Seq<Vec<usize>>, from: Int, to: Int) -> bool {
    pearlite! { 0 <= from && from < adjacency.len() && has(adjacency[from]@, to) }
}

/// Whether every edge of `adjacency` names a node, and `targets` has one
/// flag per node.
#[logic(open)]
pub fn well_formed(adjacency: Seq<Vec<usize>>, targets: Seq<bool>) -> bool {
    pearlite! {
        targets.len() == adjacency.len()
            && forall<from: Int, index: Int> 0 <= from && from < adjacency.len()
                && 0 <= index && index < adjacency[from]@.len()
                ==> adjacency[from]@[index]@ < adjacency.len()
    }
}

/// Whether `path` is non-empty and each step of it is an edge.
#[logic(open)]
pub fn is_path(adjacency: Seq<Vec<usize>>, path: Seq<Int>) -> bool {
    pearlite! {
        path.len() > 0
            && forall<step: Int> 0 <= step && step + 1 < path.len()
                ==> edge(adjacency, path[step], path[step + 1])
    }
}

/// Whether `path` ends at a node `targets` flags.
#[logic(open)]
pub fn ends_at_target(targets: Seq<bool>, path: Seq<Int>) -> bool {
    pearlite! {
        path.len() > 0 && 0 <= path[path.len() - 1] && path[path.len() - 1] < targets.len()
            && targets[path[path.len() - 1]]
    }
}

/// Whether some path from `node` ends at a node `targets` flags. A target
/// reaches itself.
#[logic(open)]
pub fn reaches_target(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, node: Int) -> bool {
    pearlite! {
        exists<path: Seq<Int>> is_path(adjacency, path) && path[0] == node
            && ends_at_target(targets, path)
    }
}

/// Whether every node `marked` holds has each of its predecessors marked
/// too, leaving out the nodes in `pending` and `skip`.
#[logic(open)]
pub fn closed_except(
    adjacency: Seq<Vec<usize>>,
    marked: Seq<bool>,
    pending: Seq<usize>,
    skip: Int,
) -> bool {
    pearlite! {
        forall<from: Int, to: Int> edge(adjacency, from, to) && 0 <= to && to < marked.len()
            && marked[to] && !has(pending, to) && to != skip
            ==> marked[from]
    }
}

/// Whether every node `marked` holds reaches a target.
#[logic(open)]
pub fn sound(adjacency: Seq<Vec<usize>>, targets: Seq<bool>, marked: Seq<bool>) -> bool {
    pearlite! {
        forall<node: Int> 0 <= node && node < marked.len() && marked[node]
            ==> reaches_target(adjacency, targets, node)
    }
}

/// Whether every node in `pending` is a marked node.
#[logic(open)]
pub fn pending_marked(marked: Seq<bool>, pending: Seq<usize>) -> bool {
    pearlite! {
        forall<index: Int> 0 <= index && index < pending.len()
            ==> pending[index]@ < marked.len() && marked[pending[index]@]
    }
}

/// Whether `predecessors[to]` lists exactly the nodes with an edge to `to`.
#[logic(open)]
pub fn reverses(adjacency: Seq<Vec<usize>>, predecessors: Seq<Vec<usize>>) -> bool {
    pearlite! {
        predecessors.len() == adjacency.len()
            && forall<to: Int, from: Int> 0 <= to && to < predecessors.len()
                ==> has(predecessors[to]@, from) == edge(adjacency, from, to)
    }
}

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
