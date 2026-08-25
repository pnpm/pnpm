use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
};

/// Output of [`graph_sequencer`].
#[derive(Debug)]
pub struct GraphSequencerResult<Node> {
    /// `false` when at least one cycle of length > 1 was found.
    pub safe: bool,
    /// Topologically ordered groups. Every node in chunk `i` has all of its
    /// outgoing edges (within the included subset) pointing into earlier
    /// chunks `0..i`, so chunk `i` may run only after chunks `0..i` finish.
    pub chunks: Vec<Vec<Node>>,
    /// Cycles encountered while sorting. Each cycle is a list of nodes.
    pub cycles: Vec<Vec<Node>>,
}

/// Topologically sort a graph into chunks.
///
/// `graph` is a node → outgoing-edges map. `included` selects the subset of
/// nodes to be sorted. Edges to nodes outside the included set are ignored.
///
/// Iteration order follows `included`, so the output is deterministic for a
/// given input order.
///
/// The nodes are interned to indices up front and each chunk is gathered
/// from the nodes whose degree a removal drops to zero, so a workspace-scale
/// graph (thousands of projects in thousands of chunks) sorts in
/// `O(V log V + E)` instead of scanning — and hashing — every node once per
/// chunk.
pub fn graph_sequencer<Node>(
    graph: &HashMap<Node, Vec<Node>>,
    included: &[Node],
) -> GraphSequencerResult<Node>
where
    Node: Eq + Hash + Clone,
{
    let mut interner = Interner::with_capacity(included.len() + graph.len());
    // Included nodes are interned first, so an id below `included_count` is
    // an included node and ids order chunks the way `included` orders them.
    for node in included {
        interner.intern(node);
    }
    let included_count = interner.nodes.len();
    for (from, edges) in graph {
        interner.intern(from);
        for to in edges {
            interner.intern(to);
        }
    }
    let node_count = interner.nodes.len();

    let is_included = |id: usize| id < included_count;

    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); node_count];
    let mut reverse_graph: Vec<Vec<usize>> = vec![Vec::new(); node_count];
    let mut out_degree: Vec<usize> = vec![0; node_count];
    for (from, edges) in graph {
        let from = interner.index_of[from];
        for to in edges {
            let to = interner.index_of[to];
            adjacency[from].push(to);
            if is_included(from) && is_included(to) {
                out_degree[from] += 1;
                reverse_graph[to].push(from);
            }
        }
    }

    // A non-included node is born removed: chunks never contain it and the
    // cycle search does not walk through it.
    let mut removed: Vec<bool> = (0..node_count).map(|id| !is_included(id)).collect();

    let mut chunks: Vec<Vec<Node>> = Vec::new();
    let mut cycles: Vec<Vec<Node>> = Vec::new();
    let mut safe = true;

    let mut remaining = included_count;
    // The ids whose degree is zero, i.e. the next chunk. Kept sorted so a
    // chunk lists its nodes in `included` order.
    let mut current: Vec<usize> = (0..included_count).filter(|&id| out_degree[id] == 0).collect();
    while remaining > 0 {
        let mut next: Vec<usize> = Vec::new();
        let mut remove_node = |id: usize, removed: &mut [bool], next: &mut Vec<usize>| {
            removed[id] = true;
            for &parent in &reverse_graph[id] {
                if out_degree[parent] > 0 {
                    out_degree[parent] -= 1;
                    if out_degree[parent] == 0 && !removed[parent] {
                        next.push(parent);
                    }
                }
            }
        };

        if current.is_empty() {
            // Every remaining node keeps a dependency alive: cycles. Break
            // them the way the scan finds them, in `included` order.
            let mut cycle_ids: Vec<usize> = Vec::new();
            for id in 0..included_count {
                if removed[id] {
                    continue;
                }
                let cycle = find_cycle(id, &adjacency, &removed);
                if cycle.is_empty() {
                    continue;
                }
                if cycle.len() > 1 {
                    safe = false;
                }
                for &node in &cycle {
                    remove_node(node, &mut removed, &mut next);
                }
                cycle_ids.extend(cycle.iter().copied());
                cycles.push(interner.to_nodes(&cycle));
            }
            remaining -= cycle_ids.len();
            chunks.push(interner.to_nodes(&cycle_ids));
        } else {
            for &id in &current {
                remove_node(id, &mut removed, &mut next);
            }
            remaining -= current.len();
            chunks.push(interner.to_nodes(&current));
        }
        // Breaking a cycle removes its members one by one, so an earlier
        // member's removal can drop a later member to degree zero right
        // before that member is removed too — filter those out of the
        // zero-degree set instead of re-chunking them.
        next.retain(|&id| !removed[id]);
        next.sort_unstable();
        current = next;
    }

    GraphSequencerResult { safe, chunks, cycles }
}

/// Node ↔ index mapping: every hash lookup the sort needs happens once
/// here, and the algorithm itself runs on plain indices.
struct Interner<'graph, Node> {
    index_of: HashMap<&'graph Node, usize>,
    nodes: Vec<&'graph Node>,
}

impl<'graph, Node: Eq + Hash + Clone> Interner<'graph, Node> {
    fn with_capacity(capacity: usize) -> Self {
        Interner { index_of: HashMap::with_capacity(capacity), nodes: Vec::with_capacity(capacity) }
    }

    fn intern(&mut self, node: &'graph Node) -> usize {
        *self.index_of.entry(node).or_insert_with(|| {
            self.nodes.push(node);
            self.nodes.len() - 1
        })
    }

    fn to_nodes(&self, ids: &[usize]) -> Vec<Node> {
        ids.iter().map(|&id| self.nodes[id].clone()).collect()
    }
}

/// The longest of the shortest cycles running from `start` back to itself
/// through nodes not yet removed, or empty when there is none.
fn find_cycle(start: usize, adjacency: &[Vec<usize>], removed: &[bool]) -> Vec<usize> {
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
            if removed[to] || cycle_visited[to] {
                continue;
            }
            cycle_visited[to] = true;
            let mut new_cycle = cycle.clone();
            new_cycle.push(to);
            queue.push_back((to, new_cycle));
        }
    }

    found_cycles.sort_by_key(|cycle| std::cmp::Reverse(cycle.len()));
    found_cycles.into_iter().next().unwrap_or_default()
}

#[cfg(test)]
mod tests;
