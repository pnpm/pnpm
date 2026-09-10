use super::{
    AncestorIds, Arc, BTreeMap, ChildEdge, HashMap, HashSet, MissingSummary, NodeId, ResolvedTree,
};

/// The resolved peers an ancestor still has to satisfy: the ones this node
/// did not itself provide a child for.
pub(super) fn external_peers_to_report(
    all_resolved_peers: &HashMap<String, NodeId>,
    children_map: &BTreeMap<String, NodeId>,
    discovery_children: Option<&(Arc<Vec<ChildEdge>>, AncestorIds)>,
) -> HashMap<String, NodeId> {
    all_resolved_peers
        .iter()
        .filter(|(peer_alias, _)| {
            !children_map.contains_key(peer_alias.as_str())
                && discovery_children.is_none_or(|(children, _)| {
                    !children.iter().any(|edge| edge.alias == **peer_alias)
                })
        })
        .map(|(peer_alias, peer_node_id)| (peer_alias.clone(), peer_node_id.clone()))
        .collect()
}

/// The missing-peer names reported for one package by a walk. A
/// package's occurrences can appear in several subtree summaries, whose
/// reports are read as their union.
pub(crate) enum MissingNames<'a> {
    One(&'a HashSet<String>),
    Union(Vec<&'a HashSet<String>>),
}

impl<'a> MissingNames<'a> {
    pub(super) fn add(&mut self, names: &'a HashSet<String>) {
        match self {
            MissingNames::One(first) => *self = MissingNames::Union(vec![first, names]),
            MissingNames::Union(all) => all.push(names),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        let (one, union) = match self {
            MissingNames::One(names) => (Some(*names), None),
            MissingNames::Union(all) => (None, Some(all)),
        };
        one.into_iter()
            .chain(union.into_iter().flatten().copied())
            .flat_map(|names| names.iter().map(String::as_str))
    }
}

/// Strongly-connected-component ids over the recorded children graph.
/// Iterative Tarjan, mirroring the peer-graph variant in the finalize
/// pass, so deep graphs cannot overflow the call stack.
pub(super) fn children_scc_ids(tree: &ResolvedTree) -> HashMap<Arc<str>, usize> {
    let (node_ids, adjacency) = children_graph_adjacency(tree);
    let mut pass = SccPass::new(adjacency);
    pass.run();
    node_ids.into_iter().zip(pass.scc_of_index).filter(|(_, scc)| *scc != usize::MAX).collect()
}

/// The children graph as a dense adjacency list, alongside the package ids
/// the indices stand for.
pub(super) fn children_graph_adjacency(tree: &ResolvedTree) -> (Vec<Arc<str>>, Vec<Vec<usize>>) {
    let mut node_ids: Vec<Arc<str>> = Vec::new();
    let mut index_by_id: HashMap<Arc<str>, usize> = HashMap::default();
    let mut intern = |id: &Arc<str>, node_ids: &mut Vec<Arc<str>>| -> usize {
        if let Some(index) = index_by_id.get(id) {
            return *index;
        }
        let index = node_ids.len();
        node_ids.push(Arc::clone(id));
        index_by_id.insert(Arc::clone(id), index);
        index
    };
    let mut adjacency: Vec<Vec<usize>> = Vec::new();
    for (pkg_id, edges) in &tree.children_by_id {
        let node = intern(pkg_id, &mut node_ids);
        if adjacency.len() <= node {
            adjacency.resize_with(node + 1, Vec::new);
        }
        let targets: Vec<usize> =
            edges.iter().map(|edge| intern(&edge.pkg_id, &mut node_ids)).collect();
        adjacency[node] = targets;
    }
    adjacency.resize_with(node_ids.len(), Vec::new);
    (node_ids, adjacency)
}

/// One iterative Tarjan pass. The DFS stack is explicit so deep dependency
/// graphs don't overflow the call stack.
pub(super) struct SccPass {
    pub(super) adjacency: Vec<Vec<usize>>,
    pub(super) discovery: Vec<u32>,
    pub(super) lowlink: Vec<u32>,
    pub(super) on_stack: Vec<bool>,
    pub(super) tarjan_stack: Vec<usize>,
    pub(super) next_index: u32,
    pub(super) scc_of_index: Vec<usize>,
    pub(super) next_scc: usize,
}

impl SccPass {
    pub(super) fn new(adjacency: Vec<Vec<usize>>) -> Self {
        let node_count = adjacency.len();
        SccPass {
            adjacency,
            discovery: vec![u32::MAX; node_count],
            lowlink: vec![0; node_count],
            on_stack: vec![false; node_count],
            tarjan_stack: Vec::new(),
            next_index: 0,
            scc_of_index: vec![usize::MAX; node_count],
            next_scc: 0,
        }
    }

    pub(super) fn run(&mut self) {
        for root in 0..self.adjacency.len() {
            if self.discovery[root] != u32::MAX {
                continue;
            }
            self.visit_root(root);
        }
    }

    pub(super) fn visit_root(&mut self, root: usize) {
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (node, ref mut cursor)) = work.last_mut() {
            if *cursor == 0 {
                self.open(node);
            }
            if let Some(child) = self.next_unvisited_child(node, cursor) {
                work.push((child, 0));
                continue;
            }
            if self.lowlink[node] == self.discovery[node] {
                self.close_scc(node);
            }
            work.pop();
            if let Some(&mut (parent, _)) = work.last_mut() {
                self.lowlink[parent] = self.lowlink[parent].min(self.lowlink[node]);
            }
        }
    }

    pub(super) fn open(&mut self, node: usize) {
        self.discovery[node] = self.next_index;
        self.lowlink[node] = self.next_index;
        self.next_index += 1;
        self.on_stack[node] = true;
        self.tarjan_stack.push(node);
    }

    /// Advances `node`'s edge cursor to its next unvisited child, folding
    /// every child already on the Tarjan stack into `node`'s lowlink on the
    /// way.
    pub(super) fn next_unvisited_child(
        &mut self,
        node: usize,
        cursor: &mut usize,
    ) -> Option<usize> {
        while *cursor < self.adjacency[node].len() {
            let child = self.adjacency[node][*cursor];
            *cursor += 1;
            if self.discovery[child] == u32::MAX {
                return Some(child);
            }
            if self.on_stack[child] {
                self.lowlink[node] = self.lowlink[node].min(self.discovery[child]);
            }
        }
        None
    }

    pub(super) fn close_scc(&mut self, root: usize) {
        loop {
            let member = self.tarjan_stack.pop().expect("Tarjan stack holds the open SCC");
            self.on_stack[member] = false;
            self.scc_of_index[member] = self.next_scc;
            if member == root {
                break;
            }
        }
        self.next_scc += 1;
    }
}

/// Index the per-package missing-peer names `roots` reported, borrowing
/// from the summaries rather than copying every descendant's names into
/// an owned map: one of these is built per importer per hoist round, so
/// a copy would make each round cost the whole workspace.
pub(crate) fn index_missing_names(
    roots: &[Arc<MissingSummary>],
) -> HashMap<&str, MissingNames<'_>> {
    let mut index: HashMap<&str, MissingNames<'_>> = HashMap::default();
    let mut seen: HashSet<usize> = HashSet::default();
    let mut pending: Vec<&Arc<MissingSummary>> = roots.iter().collect();
    while let Some(summary) = pending.pop() {
        if !seen.insert(Arc::as_ptr(summary) as usize) {
            continue;
        }
        if let Some((pkg_id, names)) = &summary.own {
            index
                .entry(pkg_id.as_str())
                .and_modify(|entry| entry.add(names))
                .or_insert(MissingNames::One(names));
        }
        pending.extend(summary.children.iter());
    }
    index
}
