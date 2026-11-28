//! Paths through a graph given as adjacency lists, the terms the search's
//! contract is stated in.

use creusot_std::prelude::{
    Int,
    Seq,
    logic,
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
