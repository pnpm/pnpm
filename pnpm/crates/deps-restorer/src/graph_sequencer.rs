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
/// chunk. Cycle discovery is confined to each strongly connected component:
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
            //
            // A cycle through a node lies entirely inside the node's
            // strongly connected component, so only members of a
            // non-trivial component (or self-loops) are searched, and each
            // search stays inside its component. Without the filter, every
            // node that merely leads *into* a cycle pays a full
            // reachability walk that finds nothing.
            let components = StronglyConnectedComponents::compute(&adjacency, &removed);
            let mut cycle_ids: Vec<usize> = Vec::new();
            for id in 0..included_count {
                if removed[id] || !components.may_lie_on_cycle(id, &adjacency) {
                    continue;
                }
                let cycle = find_cycle(id, &adjacency, &removed, &components);
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

/// The strongly connected components of the not-yet-removed subgraph,
/// computed with an iterative Tarjan walk (recursion would overflow on a
/// workspace-deep chain). Removed nodes belong to no component.
struct StronglyConnectedComponents {
    component_of: Vec<usize>,
    component_size: Vec<usize>,
}

impl StronglyConnectedComponents {
    const NONE: usize = usize::MAX;

    fn compute(adjacency: &[Vec<usize>], removed: &[bool]) -> Self {
        let node_count = adjacency.len();
        let mut discovery = vec![Self::NONE; node_count];
        let mut low_link = vec![0; node_count];
        let mut on_stack = vec![false; node_count];
        let mut stack: Vec<usize> = Vec::new();
        let mut component_of = vec![Self::NONE; node_count];
        let mut component_size: Vec<usize> = Vec::new();
        let mut next_discovery = 0;
        // Explicit DFS frames of (node, next edge position).
        let mut frames: Vec<(usize, usize)> = Vec::new();

        for root in 0..node_count {
            if removed[root] || discovery[root] != Self::NONE {
                continue;
            }
            discovery[root] = next_discovery;
            low_link[root] = next_discovery;
            next_discovery += 1;
            stack.push(root);
            on_stack[root] = true;
            frames.push((root, 0));
            while let Some(frame) = frames.last_mut() {
                let node = frame.0;
                let edge_index = frame.1;
                frame.1 += 1;
                if let Some(&to) = adjacency[node].get(edge_index) {
                    if removed[to] {
                        continue;
                    }
                    if discovery[to] == Self::NONE {
                        discovery[to] = next_discovery;
                        low_link[to] = next_discovery;
                        next_discovery += 1;
                        stack.push(to);
                        on_stack[to] = true;
                        frames.push((to, 0));
                    } else if on_stack[to] {
                        low_link[node] = low_link[node].min(discovery[to]);
                    }
                } else {
                    frames.pop();
                    if let Some(&(parent, _)) = frames.last() {
                        low_link[parent] = low_link[parent].min(low_link[node]);
                    }
                    if low_link[node] == discovery[node] {
                        let component = component_size.len();
                        let mut size = 0;
                        loop {
                            let member = stack.pop().expect("Tarjan stack holds the component");
                            on_stack[member] = false;
                            component_of[member] = component;
                            size += 1;
                            if member == node {
                                break;
                            }
                        }
                        component_size.push(size);
                    }
                }
            }
        }

        StronglyConnectedComponents { component_of, component_size }
    }

    /// Whether a cycle through `node` can exist: it shares a non-trivial
    /// component with another node, or loops onto itself. Removals since
    /// [`Self::compute`] can make this a false positive — the search then
    /// comes back empty, exactly as it would have without the filter —
    /// but never a false negative, because removals only take cycles away.
    fn may_lie_on_cycle(&self, node: usize, adjacency: &[Vec<usize>]) -> bool {
        self.component_size[self.component_of[node]] >= 2 || adjacency[node].contains(&node)
    }

    fn shares_component(&self, left: usize, right: usize) -> bool {
        self.component_of[left] == self.component_of[right]
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
    found_cycles.into_iter().next().unwrap_or_default()
}

#[cfg(test)]
mod tests;
