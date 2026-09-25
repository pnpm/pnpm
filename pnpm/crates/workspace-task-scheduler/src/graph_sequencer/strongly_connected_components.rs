/// The strongly connected components of the not-yet-removed subgraph,
/// computed with an iterative Tarjan walk (recursion would overflow on a
/// workspace-deep chain). Removed nodes belong to no component.
pub struct StronglyConnectedComponents {
    component_of: Vec<usize>,
    component_size: Vec<usize>,
}

impl StronglyConnectedComponents {
    const NONE: usize = usize::MAX;

    /// Partitions the graph, excluding nodes marked in `removed`.
    ///
    /// Edges must contain valid node indices and `removed` must have one entry per node.
    #[must_use]
    pub fn compute(adjacency: &[Vec<usize>], removed: &[bool]) -> Self {
        let mut walk = TarjanWalk::new(adjacency.len());
        for root in 0..adjacency.len() {
            if removed[root] || walk.discovery[root] != Self::NONE {
                continue;
            }
            walk.run(root, adjacency, removed);
        }
        StronglyConnectedComponents {
            component_of: walk.component_of,
            component_size: walk.component_size,
        }
    }

    /// Component indices for each node. Removed nodes use [`usize::MAX`].
    #[must_use]
    pub fn component_ids(&self) -> &[usize] {
        &self.component_of
    }

    /// The number of components in the included subgraph.
    #[must_use]
    pub fn component_count(&self) -> usize {
        self.component_size.len()
    }

    /// Whether a cycle through `node` can exist: it shares a non-trivial
    /// component with another node, or loops onto itself. Removals since
    /// [`Self::compute`] can make this a false positive — the search then
    /// comes back empty, exactly as it would have without the filter —
    /// but never a false negative, because removals only take cycles away.
    pub(super) fn may_lie_on_cycle(&self, node: usize, adjacency: &[Vec<usize>]) -> bool {
        self.component_size[self.component_of[node]] >= 2 || adjacency[node].contains(&node)
    }

    pub(super) fn shares_component(&self, left: usize, right: usize) -> bool {
        self.component_of[left] == self.component_of[right]
    }
}

/// One iterative Tarjan pass. Recursion would overflow on a workspace-deep
/// chain, so the DFS keeps its own frames.
struct TarjanWalk {
    discovery: Vec<usize>,
    low_link: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    component_of: Vec<usize>,
    component_size: Vec<usize>,
    next_discovery: usize,
    /// Explicit DFS frames of (node, next edge position).
    frames: Vec<(usize, usize)>,
}

impl TarjanWalk {
    fn new(node_count: usize) -> Self {
        TarjanWalk {
            discovery: vec![StronglyConnectedComponents::NONE; node_count],
            low_link: vec![0; node_count],
            on_stack: vec![false; node_count],
            stack: Vec::new(),
            component_of: vec![StronglyConnectedComponents::NONE; node_count],
            component_size: Vec::new(),
            next_discovery: 0,
            frames: Vec::new(),
        }
    }

    /// Walk everything reachable from `root` that is neither removed nor
    /// already discovered, assigning a component to each node it closes.
    fn run(&mut self, root: usize, adjacency: &[Vec<usize>], removed: &[bool]) {
        self.discover(root);
        while !self.frames.is_empty() {
            let (node, edge_index) = {
                let frame = self.frames.last_mut().expect("the loop guard holds a frame");
                let step = (frame.0, frame.1);
                frame.1 += 1;
                step
            };
            match adjacency[node].get(edge_index) {
                Some(&to) => self.follow_edge(node, to, removed),
                None => self.finish_node(node),
            }
        }
    }

    /// Stamp `node` with the next discovery index and open a frame for it.
    fn discover(&mut self, node: usize) {
        self.discovery[node] = self.next_discovery;
        self.low_link[node] = self.next_discovery;
        self.next_discovery += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
        self.frames.push((node, 0));
    }

    fn follow_edge(&mut self, node: usize, to: usize, removed: &[bool]) {
        if removed[to] {
            return;
        }
        if self.discovery[to] == StronglyConnectedComponents::NONE {
            self.discover(to);
        } else if self.on_stack[to] {
            self.low_link[node] = self.low_link[node].min(self.discovery[to]);
        }
    }

    /// Close `node`'s frame, carry its low link up to its parent, and pop the
    /// component off the stack when `node` is a component root.
    fn finish_node(&mut self, node: usize) {
        self.frames.pop();
        if let Some(&(parent, _)) = self.frames.last() {
            self.low_link[parent] = self.low_link[parent].min(self.low_link[node]);
        }
        if self.low_link[node] == self.discovery[node] {
            self.pop_component(node);
        }
    }

    fn pop_component(&mut self, root: usize) {
        let component = self.component_size.len();
        let mut size = 0;
        loop {
            let member = self.stack.pop().expect("Tarjan stack holds the component");
            self.on_stack[member] = false;
            self.component_of[member] = component;
            size += 1;
            if member == root {
                break;
            }
        }
        self.component_size.push(size);
    }
}
