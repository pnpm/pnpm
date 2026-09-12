use super::{HashMap, HashSet, NodeId};

/// Iterative Tarjan over the peer graph. The DFS stack is explicit so deep
/// peer graphs don't overflow the call stack.
#[derive(Default)]
pub(super) struct PeerSccPass {
    pub(super) index_of: HashMap<NodeId, u32>,
    pub(super) low_of: HashMap<NodeId, u32>,
    pub(super) on_stack: HashSet<NodeId>,
    pub(super) tarjan_stack: Vec<NodeId>,
    /// Reverse-topological order, as Tarjan closes them.
    pub(super) sccs: Vec<Vec<NodeId>>,
    pub(super) scc_of: HashMap<NodeId, usize>,
    pub(super) next_index: u32,
}

impl PeerSccPass {
    pub(super) fn visit_root<Neighbors>(&mut self, root: &NodeId, neighbors: &Neighbors)
    where
        Neighbors: Fn(&NodeId) -> Vec<NodeId>,
    {
        let mut work: Vec<(NodeId, Vec<NodeId>, usize)> = vec![(root.clone(), neighbors(root), 0)];
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

    pub(super) fn open(&mut self, node_id: &NodeId) {
        self.index_of.insert(node_id.clone(), self.next_index);
        self.low_of.insert(node_id.clone(), self.next_index);
        self.next_index += 1;
        self.on_stack.insert(node_id.clone());
        self.tarjan_stack.push(node_id.clone());
    }

    /// Advances `node_id`'s successor cursor to its next unvisited peer,
    /// folding every successor already on the stack into its lowlink on the
    /// way.
    pub(super) fn next_unvisited(
        &mut self,
        node_id: &NodeId,
        successors: &[NodeId],
        cursor: &mut usize,
    ) -> Option<NodeId> {
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

    pub(super) fn close_scc(&mut self, root: &NodeId) {
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
