//! Strongly connected components, for the peer cycles the resolver collapses.

use std::collections::HashMap;

/// Every node in a component, ordered so that a component comes after every
/// component it has an edge to. Nodes that don't participate come first, each
/// on its own, and have no entry in `component_of`.
pub(super) struct Components {
    pub components: Vec<Vec<usize>>,
    pub component_of: HashMap<usize, usize>,
}

/// Iterative Tarjan over the participating nodes of `0..count`. Edges to
/// non-participating nodes are ignored.
pub(super) fn components(
    count: usize,
    neighbors: impl Fn(usize) -> Vec<usize>,
    participates: impl Fn(usize) -> bool,
) -> Components {
    let edges_of = |node: usize| -> Vec<usize> {
        neighbors(node)
            .into_iter()
            .filter(|&next| participates(next))
            .collect()
    };
    let mut tarjan = Tarjan {
        index_of: vec![None; count],
        low: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        frames: Vec::new(),
        next_index: 0,
        found: Components {
            components: (0..count)
                .filter(|&node| !participates(node))
                .map(|node| vec![node])
                .collect(),
            component_of: HashMap::new(),
        },
    };
    for root in (0..count).filter(|&node| participates(node)) {
        if tarjan.index_of[root].is_none() {
            tarjan.visit(root, &edges_of);
        }
    }
    tarjan.found
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
    found: Components,
}

impl Tarjan {
    fn visit(&mut self, root: usize, edges_of: &impl Fn(usize) -> Vec<usize>) {
        self.enter(root, edges_of(root));
        while let Some(frame) = self.frames.last_mut() {
            let Some(&next) = frame.edges.get(frame.position) else {
                self.leave();
                continue;
            };
            frame.position += 1;
            let node = frame.node;
            match self.index_of[next] {
                None => self.enter(next, edges_of(next)),
                Some(next_index) if self.on_stack[next] => {
                    self.low[node] = self.low[node].min(next_index);
                }
                Some(_) => {}
            }
        }
    }

    fn enter(&mut self, node: usize, edges: Vec<usize>) {
        self.index_of[node] = Some(self.next_index);
        self.low[node] = self.next_index;
        self.next_index += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
        self.frames.push(Frame { node, edges, position: 0 });
    }

    /// Finishes the node on top of the frames, emitting its component when it
    /// is the component's root.
    fn leave(&mut self) {
        let Some(Frame { node, .. }) = self.frames.pop() else { return };
        if let Some(parent) = self.frames.last() {
            let parent = parent.node;
            self.low[parent] = self.low[parent].min(self.low[node]);
        }
        if Some(self.low[node]) != self.index_of[node] {
            return;
        }
        let index = self.found.components.len();
        let mut component = Vec::new();
        while let Some(member) = self.stack.pop() {
            self.on_stack[member] = false;
            self.found.component_of.insert(member, index);
            component.push(member);
            if member == node {
                break;
            }
        }
        self.found.components.push(component);
    }
}
