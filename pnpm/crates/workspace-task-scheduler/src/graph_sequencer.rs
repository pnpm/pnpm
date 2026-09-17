pub use strongly_connected_components::StronglyConnectedComponents;

mod strongly_connected_components;

use rustc_hash::FxHashMap;
use std::{
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    path::Path,
};

/// A path node for [`graph_sequencer`], compared and hashed by the
/// underlying `OsStr`'s bytes. `Path`'s own `Hash` and `Eq` walk the
/// path component by component, and on a workspace-scale graph the
/// interner's hashing dominated the sort. Byte equality is stricter
/// than component equality, which for node identity can only split a
/// node the caller spelled two ways — the callers here key their
/// graphs by one canonical `PathBuf` per project.
#[derive(Debug, Clone, Copy)]
pub struct PathNode<'graph>(pub &'graph Path);

impl PartialEq for PathNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_os_str() == other.0.as_os_str()
    }
}

impl Eq for PathNode<'_> {}

impl Hash for PathNode<'_> {
    fn hash<State: Hasher>(&self, state: &mut State) {
        self.0.as_os_str().hash(state);
    }
}

/// Output of [`graph_sequencer`].
#[derive(Debug)]
pub struct GraphSequencerResult<Node> {
    /// One topological order, dependencies before dependents.
    pub order: Vec<Node>,
    /// Cycles encountered while sorting. Each cycle is a list of nodes.
    pub cycles: Vec<Vec<Node>>,
}

/// Return one deterministic topological order and any cycles in a graph.
///
/// `graph` is a node → outgoing-edges map. `included` selects the subset of
/// nodes to be sorted. Edges to nodes outside the included set are ignored.
///
/// Iteration order follows `included`, so the output is deterministic for a
/// given input order.
///
/// The nodes are interned to indices up front and each ready set is gathered
/// from nodes whose degree a removal drops to zero, so a workspace-scale graph
/// sorts in `O(V log V + E)` instead of repeatedly scanning and hashing every
/// node. Cycle discovery is confined to each strongly connected component:
/// nodes that merely lead into a cycle cost nothing extra, and only
/// enumerating the cycles *inside* one component pays that component's size
/// per reported cycle (the price of the established cycle-reporting
/// semantics).
pub fn graph_sequencer<Node>(
    graph: &HashMap<Node, Vec<Node>>,
    included: &[Node],
) -> GraphSequencerResult<Node>
where
    Node: Eq + Hash + Clone,
{
    let indexed = index_graph(graph, included);
    let included_count = indexed.included_count;

    let mut sweep = Sweep {
        reverse_graph: &indexed.reverse_graph,
        // A non-included node is born removed: the order never contains it
        // and the cycle search does not walk through it.
        removed: (0..indexed.adjacency.len())
            .map(|id| id >= included_count)
            .collect(),
        out_degree: indexed.out_degree,
        next: Vec::new(),
    };

    let mut order: Vec<usize> = Vec::with_capacity(included_count);
    let mut cycles: Vec<Vec<Node>> = Vec::new();

    let mut remaining = included_count;
    // The ids whose degree is zero, i.e. the next ready set. Kept sorted in
    // `included` order.
    let mut current: Vec<usize> = (0..included_count)
        .filter(|&id| sweep.out_degree[id] == 0)
        .collect();
    while remaining > 0 {
        if current.is_empty() {
            for cycle in sweep.break_cycles(&indexed.adjacency, included_count) {
                remaining -= cycle.len();
                order.extend(&cycle);
                cycles.push(indexed.interner.to_nodes(&cycle));
            }
        } else {
            for &id in &current {
                sweep.remove(id);
            }
            remaining -= current.len();
            order.extend(&current);
        }
        // Breaking a cycle removes its members one by one, so an earlier
        // member's removal can drop a later member to degree zero right
        // before that member is removed too — filter those out of the
        // zero-degree set instead of adding them to the order twice.
        let mut next = std::mem::take(&mut sweep.next);
        next.retain(|&id| !sweep.removed[id]);
        next.sort_unstable();
        current = next;
    }

    GraphSequencerResult { order: indexed.interner.to_nodes(&order), cycles }
}

/// The interned graph the sort runs on. Ids below `included_count` are the
/// included nodes, in `included` order.
struct Indexed<'graph, Node> {
    interner: Interner<'graph, Node>,
    included_count: usize,
    adjacency: Vec<Vec<usize>>,
    reverse_graph: Vec<Vec<usize>>,
    out_degree: Vec<usize>,
}

fn index_graph<'graph, Node>(
    graph: &'graph HashMap<Node, Vec<Node>>,
    included: &'graph [Node],
) -> Indexed<'graph, Node>
where
    Node: Eq + Hash + Clone,
{
    let mut interner = Interner::with_capacity(included.len() + graph.len());
    // Included nodes are interned first, so an id below `included_count` is
    // an included node and id order follows `included`.
    for node in included {
        interner.intern(node);
    }
    let included_count = interner.nodes.len();
    intern_edge_nodes(&mut interner, graph);

    let node_count = interner.nodes.len();
    let mut indexed = Indexed {
        interner,
        included_count,
        adjacency: vec![Vec::new(); node_count],
        reverse_graph: vec![Vec::new(); node_count],
        out_degree: vec![0; node_count],
    };
    for (from, edges) in graph {
        indexed.add_edges(from, edges);
    }
    indexed
}

fn intern_edge_nodes<'graph, Node: Eq + Hash + Clone>(
    interner: &mut Interner<'graph, Node>,
    graph: &'graph HashMap<Node, Vec<Node>>,
) {
    for (from, edges) in graph {
        interner.intern(from);
        for to in edges {
            interner.intern(to);
        }
    }
}

impl<Node: Eq + Hash + Clone> Indexed<'_, Node> {
    /// Only an edge between two included nodes counts toward the degree
    /// sweep; the rest exist for the cycle search alone.
    fn add_edges(&mut self, from: &Node, edges: &[Node]) {
        let from = self.interner.index_of[from];
        for to in edges {
            let to = self.interner.index_of[to];
            self.adjacency[from].push(to);
            if from < self.included_count && to < self.included_count {
                self.out_degree[from] += 1;
                self.reverse_graph[to].push(from);
            }
        }
    }
}

/// The mutable half of the sort: which nodes are gone, what each remaining
/// node still waits on, and the ready set the current round uncovered.
struct Sweep<'a> {
    reverse_graph: &'a [Vec<usize>],
    out_degree: Vec<usize>,
    removed: Vec<bool>,
    next: Vec<usize>,
}

impl Sweep<'_> {
    /// Every remaining node keeps a dependency alive: cycles. Break them the
    /// way the scan finds them, in `included` order, and return them.
    ///
    /// A cycle through a node lies entirely inside the node's strongly
    /// connected component, so only members of a non-trivial component (or
    /// self-loops) are searched, and each search stays inside its component.
    /// Without the filter, every node that merely leads *into* a cycle pays a
    /// full reachability walk that finds nothing.
    fn break_cycles(&mut self, adjacency: &[Vec<usize>], included_count: usize) -> Vec<Vec<usize>> {
        let components = StronglyConnectedComponents::compute(adjacency, &self.removed);
        let mut broken: Vec<Vec<usize>> = Vec::new();
        for id in 0..included_count {
            if self.removed[id] || !components.may_lie_on_cycle(id, adjacency) {
                continue;
            }
            let cycle = find_cycle(id, adjacency, &self.removed, &components);
            if cycle.is_empty() {
                continue;
            }
            for &node in &cycle {
                self.remove(node);
            }
            broken.push(cycle);
        }
        broken
    }
    /// Remove `id`, collecting into [`Self::next`] the parents its removal
    /// drops to degree zero.
    fn remove(&mut self, id: usize) {
        self.removed[id] = true;
        for &parent in &self.reverse_graph[id] {
            if self.out_degree[parent] > 0 {
                self.out_degree[parent] -= 1;
                if self.out_degree[parent] == 0 && !self.removed[parent] {
                    self.next.push(parent);
                }
            }
        }
    }
}

/// Node ↔ index mapping: every hash lookup the sort needs happens once
/// here, and the algorithm itself runs on plain indices.
struct Interner<'graph, Node> {
    index_of: FxHashMap<&'graph Node, usize>,
    nodes: Vec<&'graph Node>,
}

impl<'graph, Node: Eq + Hash + Clone> Interner<'graph, Node> {
    fn with_capacity(capacity: usize) -> Self {
        Interner {
            index_of: FxHashMap::with_capacity_and_hasher(capacity, rustc_hash::FxBuildHasher),
            nodes: Vec::with_capacity(capacity),
        }
    }

    fn intern(&mut self, node: &'graph Node) -> usize {
        *self.index_of
            .entry(node)
            .or_insert_with(|| {
                self.nodes.push(node);
                self.nodes.len() - 1
            })
    }

    fn to_nodes(&self, ids: &[usize]) -> Vec<Node> {
        ids.iter()
            .map(|&id| self.nodes[id].clone())
            .collect()
    }
}

/// The longest of the shortest cycles running from `start` back to itself
/// through nodes not yet removed, or empty when there is none. The walk
/// stays inside `start`'s strongly connected component — no cycle through
/// `start` can leave it.
fn find_cycle(
    start: usize,
    adjacency: &[Vec<usize>],
    removed: &[bool],
    components: &StronglyConnectedComponents,
) -> Vec<usize> {
    let mut queue: VecDeque<(usize, Vec<usize>)> = VecDeque::new();
    queue.push_back((start, vec![start]));
    let mut cycle_visited = vec![false; adjacency.len()];
    let mut found_cycles: Vec<Vec<usize>> = Vec::new();

    while let Some((id, cycle)) = queue.pop_front() {
        for &to in &adjacency[id] {
            if to == start {
                cycle_visited[to] = true;
                found_cycles.push(cycle.clone());
                continue;
            }
            if removed[to] || cycle_visited[to] || !components.shares_component(start, to) {
                continue;
            }
            cycle_visited[to] = true;
            let mut new_cycle = cycle.clone();
            new_cycle.push(to);
            queue.push_back((to, new_cycle));
        }
    }

    found_cycles.sort_by_key(|cycle| std::cmp::Reverse(cycle.len()));
    found_cycles
        .into_iter()
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
