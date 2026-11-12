use super::{
    Arc, HashMap, NodeId, NodeWalkContext, ParentPkgInfo, ParentRefs, PeerWalkMode, SharedChain,
    TreeChildren, Walker,
};
use crate::resolved_tree::DependenciesTreeNode;

impl Walker<'_> {
    /// The children-graph SCC table behind the canonical cycle gate; see
    /// [`CanonicalCycleGate::table`](super::super::discovery::CanonicalCycleGate::table).
    pub(in super::super) fn canonical_scc(&self) -> Arc<HashMap<Arc<str>, usize>> {
        self.caches.canonical_cycles.table(self.tree, self.caches.view_generation())
    }

    /// Whether the peer walk drops the `pkg_id → child_pkg_id` edge:
    /// a self-edge, or an intra-SCC edge whose target is not later in
    /// package-id order — the same answer at every occurrence.
    pub(in super::super) fn cuts_cycle_edge(
        scc_of: &HashMap<Arc<str>, usize>,
        pkg_id: &str,
        child_pkg_id: &str,
    ) -> bool {
        pkg_id == child_pkg_id
            || match (scc_of.get(pkg_id), scc_of.get(child_pkg_id)) {
                (Some(pkg_scc), Some(child_scc)) => pkg_scc == child_scc && child_pkg_id <= pkg_id,
                _ => false,
            }
    }

    /// The shared record-only node a canonical back-edge references;
    /// created lazily and queued for the driver's importer-context
    /// walk. See [`CanonicalCycleGate::backedge_nodes`](super::super::discovery::CanonicalCycleGate::backedge_nodes).
    pub(in super::super) fn canonical_backedge_node(
        &mut self,
        pkg_id: &Arc<str>,
        depth: i32,
    ) -> NodeId {
        if self.tree.packages.get(&**pkg_id).is_some_and(|pkg| pkg.is_leaf) {
            let node_id = NodeId::leaf(pkg_id);
            if !self.tree.dependencies_tree.contains_key(&node_id) {
                self.tree.dependencies_tree.insert(
                    node_id.clone(),
                    DependenciesTreeNode::new(Arc::clone(pkg_id), TreeChildren::Lazy, depth),
                );
            }
            return node_id;
        }
        if let Some(node_id) = self.caches.canonical_cycles.backedge_nodes.get(&**pkg_id)
            && self.tree.dependencies_tree.contains_key(node_id)
        {
            return node_id.clone();
        }
        let node_id = NodeId::next();
        self.tree.dependencies_tree.insert(
            node_id.clone(),
            DependenciesTreeNode::new(Arc::clone(pkg_id), TreeChildren::Lazy, depth),
        );
        self.caches.canonical_cycles.backedge_nodes.insert(Arc::clone(pkg_id), node_id.clone());
        self.traversal.pending_canonical_nodes.push(node_id.clone());
        node_id
    }

    /// Walk every queued canonical back-edge target at importer-root
    /// context. Targets realized during these walks queue more, so the
    /// drain loops until quiet.
    pub(in super::super) fn drain_pending_canonical_nodes(
        &mut self,
        importer_parents: &Arc<ParentRefs>,
        importer_parent_dep_paths: &Arc<HashMap<String, ParentPkgInfo>>,
    ) {
        self.traversal.in_canonical_drain = true;
        while let Some(node_id) = self.traversal.pending_canonical_nodes.pop() {
            if self.traversal.snapshot_budget.poll_cancelled()
                || self.traversal.snapshot_budget.is_stopped()
            {
                break;
            }
            if self.traversal.visited_this_call.contains(&node_id) {
                continue;
            }
            let output = self.resolve_node(
                &node_id,
                &NodeWalkContext {
                    parent_refs: importer_parents,
                    parent_dep_paths: importer_parent_dep_paths,
                    chain_names: &SharedChain::default(),
                    parent_node_ids: &SharedChain::default(),
                    parent_pkg_ids: &SharedChain::default(),
                },
            );
            match self.traversal.mode {
                PeerWalkMode::Discovery if !self.traversal.snapshot_budget.is_stopped() => {
                    self.record_canonical_budget(&output);
                }
                PeerWalkMode::Discovery => {
                    self.traversal.snapshot_budget.clear_in_flight();
                }
                PeerWalkMode::Final => {}
            }
        }
        self.traversal.in_canonical_drain = false;
    }
}
