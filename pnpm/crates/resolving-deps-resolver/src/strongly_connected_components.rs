use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::hash::Hash;

/// Iterative Tarjan over a directed graph. The DFS stack is explicit so deep
/// peer graphs don't overflow the call stack.
pub(crate) struct SccPass<Node> {
    pub(crate) index_of: HashMap<Node, u32>,
    pub(crate) low_of: HashMap<Node, u32>,
    pub(crate) on_stack: HashSet<Node>,
    pub(crate) tarjan_stack: Vec<Node>,
    /// Reverse-topological order, as Tarjan closes them.
    pub(crate) sccs: Vec<Vec<Node>>,
    pub(crate) scc_of: HashMap<Node, usize>,
    pub(crate) next_index: u32,
}

impl<Node: Clone + Eq + Hash> SccPass<Node> {
    pub(crate) fn visit_root<Neighbors>(&mut self, root: &Node, neighbors: &Neighbors)
    where
        Neighbors: Fn(&Node) -> Vec<Node>,
    {
        let mut work: Vec<(Node, Vec<Node>, usize)> = vec![(root.clone(), neighbors(root), 0)];
        while let Some((node_id, successors, cursor)) = work.last_mut() {
            if *cursor == 0 {
                self.open(node_id);
            }
            if let Some(child) = self.next_unvisited(node_id, successors, cursor) {
                let child_successors = neighbors(&child);
                work.push((child, child_successors, 0));
                continue;
            }
            let node_id = node_id.clone();
            self.close_scc(&node_id);
            work.pop();
            if let Some((parent, _, _)) = work.last() {
                let parent_low = self.low_of[parent];
                let node_low = self.low_of[&node_id];
                self.low_of.insert(parent.clone(), parent_low.min(node_low));
            }
        }
    }

    pub(crate) fn open(&mut self, node_id: &Node) {
        self.index_of.insert(node_id.clone(), self.next_index);
        self.low_of.insert(node_id.clone(), self.next_index);
        self.next_index += 1;
        self.on_stack.insert(node_id.clone());
        self.tarjan_stack.push(node_id.clone());
    }

    /// Advances `node_id`'s successor cursor to its next unvisited peer,
    /// folding every successor already on the stack into its lowlink on the
    /// way.
    pub(crate) fn next_unvisited(
        &mut self,
        node_id: &Node,
        successors: &[Node],
        cursor: &mut usize,
    ) -> Option<Node> {
        while *cursor < successors.len() {
            let child = successors[*cursor].clone();
            *cursor += 1;
            if !self.index_of.contains_key(&child) {
                return Some(child);
            }
            if self.on_stack.contains(&child) {
                let node_low = self.low_of[node_id];
                let child_index = self.index_of[&child];
                self.low_of.insert(node_id.clone(), node_low.min(child_index));
            }
        }
        None
    }

    pub(crate) fn close_scc(&mut self, root: &Node) {
        if self.low_of[root] != self.index_of[root] {
            return;
        }
        let scc_index = self.sccs.len();
        let mut component = Vec::new();
        while let Some(member) = self.tarjan_stack.pop() {
            self.on_stack.remove(&member);
            self.scc_of.insert(member.clone(), scc_index);
            let is_root = member == *root;
            component.push(member);
            if is_root {
                break;
            }
        }
        self.sccs.push(component);
    }
}

impl<Node> Default for SccPass<Node> {
    fn default() -> Self {
        Self {
            index_of: HashMap::default(),
            low_of: HashMap::default(),
            on_stack: HashSet::default(),
            tarjan_stack: Vec::new(),
            sccs: Vec::new(),
            scc_of: HashMap::default(),
            next_index: 0,
        }
    }
}
