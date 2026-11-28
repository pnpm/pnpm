//! Which nodes of a graph reach a flagged node.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that [`reaching`] never
//! panics on a graph whose edges name its nodes, and that it answers
//! exactly: a node is marked if and only if a path from it, possibly of no
//! edges, ends at a flagged node. `theorems.rs` states properties that
//! follow for every graph.
//!
//! A caller deciding whether every node a package reaches has some property
//! flags the nodes that lack it, and reads the answer negated. Members of a
//! cycle reach each other, so they share one answer.

#[cfg(creusot)]
pub mod lemmas;
#[cfg(creusot)]
pub mod logic;

#[cfg(creusot)]
use creusot_std::prelude::{
    Snapshot,
    ensures,
    invariant,
    proof_assert,
    requires,
    snapshot,
};
#[cfg(creusot)]
use logic::{
    closed_except,
    edge,
    has,
    pending_marked,
    reaches_target,
    reverses,
    sound,
    well_formed,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;

/// Whether each node of the graph `adjacency` draws reaches a node
/// `targets` flags, itself included. `adjacency[node]` lists the nodes
/// `node` has an edge to.
#[cfg_attr(creusot, requires(well_formed(adjacency@, targets@)))]
#[cfg_attr(creusot, ensures(result@.len() == adjacency@.len()))]
#[cfg_attr(creusot, ensures(forall<node: Int> 0 <= node && node < adjacency@.len()
    ==> result@[node] == reaches_target(adjacency@, targets@, node)))]
#[must_use]
pub fn reaching(adjacency: &[Vec<usize>], targets: &[bool]) -> Vec<bool> {
    let predecessors = predecessors(adjacency);
    let (mut marked, mut pending) = mark_targets(adjacency, targets);
    #[cfg_attr(creusot, invariant(marked@.len() == targets@.len()))]
    #[cfg_attr(creusot, invariant(sound(adjacency@, targets@, marked@)))]
    #[cfg_attr(creusot, invariant(pending_marked(marked@, pending@)))]
    #[cfg_attr(creusot, invariant(forall<other: Int> 0 <= other && other < targets@.len()
        && targets@[other] ==> marked@[other]))]
    #[cfg_attr(creusot, invariant(closed_except(adjacency@, marked@, pending@, -1)))]
    while let Some(node) = pending.pop() {
        mark_predecessors(adjacency, targets, &predecessors, &mut marked, &mut pending, node);
    }
    #[cfg(creusot)]
    proof_assert! {
        forall<other: Int> 0 <= other && other < targets@.len() && reaches_target(adjacency@, targets@, other)
            ==> { lemmas::marked_if_reaching(adjacency@, targets@, marked@, other); marked@[other] }
    };
    marked
}

/// The targets marked, and queued so that their predecessors are marked
/// next. Every marked node is queued, so no edge into one is unmarked yet.
#[cfg_attr(creusot, requires(well_formed(adjacency@, targets@)))]
#[cfg_attr(creusot, ensures(result.0@.len() == targets@.len()))]
#[cfg_attr(creusot, ensures(sound(adjacency@, targets@, result.0@)))]
#[cfg_attr(creusot, ensures(pending_marked(result.0@, result.1@)))]
#[cfg_attr(creusot, ensures(forall<other: Int> 0 <= other && other < targets@.len()
    && targets@[other] ==> result.0@[other]))]
#[cfg_attr(creusot, ensures(closed_except(adjacency@, result.0@, result.1@, -1)))]
#[cfg_attr(
    not(creusot),
    expect(unused_variables, reason = "the graph is read by the contract only")
)]
fn mark_targets(adjacency: &[Vec<usize>], targets: &[bool]) -> (Vec<bool>, Vec<usize>) {
    let mut marked: Vec<bool> = vec![false; targets.len()];
    let mut pending: Vec<usize> = Vec::new();
    let mut node: usize = 0;
    #[cfg_attr(creusot, invariant(node@ <= targets@.len() && marked@.len() == targets@.len()))]
    #[cfg_attr(creusot, invariant(sound(adjacency@, targets@, marked@)))]
    #[cfg_attr(creusot, invariant(pending_marked(marked@, pending@)))]
    #[cfg_attr(creusot, invariant(forall<other: Int> 0 <= other && other < targets@.len()
        ==> marked@[other] == (other < node@ && targets@[other])))]
    #[cfg_attr(creusot, invariant(forall<other: Int> 0 <= other && other < node@ && targets@[other]
        ==> has(pending@, other)))]
    while node < targets.len() {
        if targets[node] {
            #[cfg(creusot)]
            proof_assert! { lemmas::target_reaches(adjacency@, targets@, node@); true };
            marked[node] = true;
            #[cfg(creusot)]
            let before = snapshot! { pending };
            pending.push(node);
            #[cfg(creusot)]
            proof_assert! { lemmas::has_snoc(before@, node); true };
        }
        node += 1;
    }
    (marked, pending)
}

/// Mark every unmarked predecessor of `node` and queue it.
#[cfg_attr(creusot, requires(well_formed(adjacency@, targets@)))]
#[cfg_attr(creusot, requires(reverses(adjacency@, predecessors@)))]
#[cfg_attr(creusot, requires(marked@.len() == targets@.len()))]
#[cfg_attr(creusot, requires(node@ < marked@.len() && marked@[node@]))]
#[cfg_attr(creusot, requires(sound(adjacency@, targets@, marked@)))]
#[cfg_attr(creusot, requires(pending_marked(marked@, pending@)))]
#[cfg_attr(creusot, requires(closed_except(adjacency@, marked@, pending@, node@)))]
#[cfg_attr(creusot, ensures((^marked)@.len() == targets@.len()))]
#[cfg_attr(creusot, ensures(sound(adjacency@, targets@, (^marked)@)))]
#[cfg_attr(creusot, ensures(pending_marked((^marked)@, (^pending)@)))]
#[cfg_attr(creusot, ensures(closed_except(adjacency@, (^marked)@, (^pending)@, -1)))]
#[cfg_attr(creusot, ensures(forall<other: Int> 0 <= other && other < marked@.len()
    && marked@[other] ==> (^marked)@[other]))]
#[cfg_attr(
    not(creusot),
    expect(unused_variables, reason = "the graph and the targets are read by the contract only")
)]
fn mark_predecessors(
    adjacency: &[Vec<usize>],
    targets: &[bool],
    predecessors: &[Vec<usize>],
    marked: &mut [bool],
    pending: &mut Vec<usize>,
    node: usize,
) {
    let mut index: usize = 0;
    #[cfg(creusot)]
    let start: Snapshot<Seq<bool>> = snapshot! { marked@ };
    #[cfg_attr(creusot, invariant(index@ <= predecessors@[node@]@.len()))]
    #[cfg_attr(creusot, invariant(marked@.len() == targets@.len() && marked@[node@]))]
    #[cfg_attr(creusot, invariant(sound(adjacency@, targets@, marked@)))]
    #[cfg_attr(creusot, invariant(pending_marked(marked@, pending@)))]
    #[cfg_attr(creusot, invariant(closed_except(adjacency@, marked@, pending@, node@)))]
    #[cfg_attr(creusot, invariant(forall<earlier: Int> 0 <= earlier && earlier < index@
        ==> marked@[predecessors@[node@]@[earlier]@]))]
    #[cfg_attr(creusot, invariant(forall<other: Int> 0 <= other && other < (*start).len()
        && (*start)[other] ==> marked@[other]))]
    while index < predecessors[node].len() {
        let predecessor = predecessors[node][index];
        #[cfg(creusot)]
        proof_assert! {
            lemmas::has_element(predecessors@[node@]@, index@);
            edge(adjacency@, predecessor@, node@) && predecessor@ < marked@.len()
        };
        if !marked[predecessor] {
            #[cfg(creusot)]
            proof_assert! {
                lemmas::extend_back(adjacency@, targets@, predecessor@, node@);
                reaches_target(adjacency@, targets@, predecessor@)
            };
            marked[predecessor] = true;
            #[cfg(creusot)]
            let before = snapshot! { *pending };
            pending.push(predecessor);
            #[cfg(creusot)]
            proof_assert! { lemmas::has_snoc(before@, predecessor); true };
        }
        index += 1;
    }
}

/// `predecessors[to]` lists the nodes with an edge to `to`.
#[cfg_attr(creusot, requires(forall<from: Int, index: Int> 0 <= from && from < adjacency@.len()
    && 0 <= index && index < adjacency@[from]@.len() ==> adjacency@[from]@[index]@ < adjacency@.len()))]
#[cfg_attr(creusot, ensures(reverses(adjacency@, result@)))]
fn predecessors(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut predecessors: Vec<Vec<usize>> = Vec::new();
    #[cfg_attr(creusot, invariant(predecessors@.len() <= adjacency@.len()))]
    #[cfg_attr(creusot, invariant(forall<to: Int> 0 <= to && to < predecessors@.len()
        ==> predecessors@[to]@.len() == 0))]
    while predecessors.len() < adjacency.len() {
        predecessors.push(Vec::new());
    }
    let mut from: usize = 0;
    #[cfg_attr(creusot, invariant(from@ <= adjacency@.len() && predecessors@.len() == adjacency@.len()))]
    #[cfg_attr(creusot, invariant(forall<to: Int, source: Int> 0 <= to && to < predecessors@.len()
        ==> has(predecessors@[to]@, source) == (source < from@ && edge(adjacency@, source, to))))]
    while from < adjacency.len() {
        add_edges_from(adjacency, &mut predecessors, from);
        from += 1;
    }
    predecessors
}

/// Record `from` as a predecessor of each node it has an edge to.
#[cfg_attr(creusot, requires(from@ < adjacency@.len() && predecessors@.len() == adjacency@.len()))]
#[cfg_attr(creusot, requires(forall<index: Int> 0 <= index && index < adjacency@[from@]@.len()
    ==> adjacency@[from@]@[index]@ < adjacency@.len()))]
#[cfg_attr(creusot, requires(forall<to: Int, source: Int> 0 <= to && to < predecessors@.len()
    ==> has(predecessors@[to]@, source) == (source < from@ && edge(adjacency@, source, to))))]
#[cfg_attr(creusot, ensures((^predecessors)@.len() == adjacency@.len()))]
#[cfg_attr(creusot, ensures(forall<to: Int, source: Int> 0 <= to && to < adjacency@.len()
    ==> has((^predecessors)@[to]@, source) == (source <= from@ && edge(adjacency@, source, to))))]
fn add_edges_from(adjacency: &[Vec<usize>], predecessors: &mut [Vec<usize>], from: usize) {
    let mut index: usize = 0;
    #[cfg_attr(creusot, invariant(index@ <= adjacency@[from@]@.len()))]
    #[cfg_attr(creusot, invariant(predecessors@.len() == adjacency@.len()))]
    #[cfg_attr(creusot, invariant(forall<to: Int, source: Int> 0 <= to && to < predecessors@.len()
        ==> has(predecessors@[to]@, source) == ((source < from@ && edge(adjacency@, source, to))
            || (source == from@ && has(adjacency@[from@]@.subsequence(0, index@), to)))))]
    while index < adjacency[from].len() {
        let to = adjacency[from][index];
        #[cfg(creusot)]
        let before = snapshot! { predecessors@ };
        predecessors[to].push(from);
        #[cfg(creusot)]
        proof_assert! {
            lemmas::has_snoc((*before)[to@]@, from);
            lemmas::has_snoc(adjacency@[from@]@.subsequence(0, index@), to);
            true
        };
        index += 1;
    }
}

#[cfg(creusot)]
use creusot_std::prelude::{
    Int,
    Seq,
};

#[cfg(test)]
mod tests;
#[cfg(creusot)]
#[expect(dead_code, reason = "a theorem is proved, never called")]
mod theorems;
