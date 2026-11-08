//! Strongly connected components, for the peer cycles the resolver collapses.

use std::collections::HashMap;

/// Every node in a component, ordered so that a component comes after every
/// component it has an edge to. Nodes that don't participate come first, each
/// on its own, and have no entry in `component_of`.
pub(super) struct Components {
    pub components: Vec<Vec<usize>>,
    pub component_of: HashMap<usize, usize>,
}

struct Frame {
    node: usize,
    edges: Vec<usize>,
    position: usize,
}

struct Tarjan {
    index_of: Vec<Option<usize>>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    frames: Vec<Frame>,
    next_index: usize,
}

impl Tarjan {
    fn enter(&mut self, node: usize, edges: Vec<usize>) {
        self.index_of[node] = Some(self.next_index);
        self.low[node] = self.next_index;
        self.next_index += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
        self.frames.push(Frame { node, edges, position: 0 });
    }
}

/// Iterative Tarjan over the participating nodes of `0..count`. Edges to
/// non-participating nodes are ignored.
pub(super) fn components(
    count: usize,
    neighbors: impl Fn(usize) -> Vec<usize>,
    participates: impl Fn(usize) -> bool,
) -> Components {
    let mut components: Vec<Vec<usize>> = (0..count)
        .filter(|&node| !participates(node))
        .map(|node| vec![node])
        .collect();
    let mut component_of = HashMap::new();
    let edges_of = |node: usize| -> Vec<usize> {
        neighbors(node)
            .into_iter()
            .filter(|&next| participates(next))
            .collect()
    };
    let mut state = Tarjan {
        index_of: vec![None; count],
        low: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        frames: Vec::new(),
        next_index: 0,
    };
    for root in (0..count).filter(|&node| participates(node)) {
        if state.index_of[root].is_some() {
            continue;
        }
        state.enter(root, edges_of(root));
        while let Some(frame) = state.frames.last_mut() {
            let node = frame.node;
            if let Some(&next) = frame.edges.get(frame.position) {
                frame.position += 1;
                match state.index_of[next] {
                    None => state.enter(next, edges_of(next)),
                    Some(next_index) if state.on_stack[next] => {
                        state.low[node] = state.low[node].min(next_index);
                    }
                    Some(_) => {}
                }
                continue;
            }
            state.frames.pop();
            if let Some(parent) = state.frames.last() {
                let parent = parent.node;
                state.low[parent] = state.low[parent].min(state.low[node]);
            }
            if Some(state.low[node]) == state.index_of[node] {
                let mut component = Vec::new();
                while let Some(member) = state.stack.pop() {
                    state.on_stack[member] = false;
                    component_of.insert(member, components.len());
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                components.push(component);
            }
        }
    }
    Components { components, component_of }
}
