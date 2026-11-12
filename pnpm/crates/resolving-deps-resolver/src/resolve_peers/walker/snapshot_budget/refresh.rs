use super::{
    CancelState, DirectDep, DirectDepBudget, HashMap, HashSet, NodeId, PeerWalkAbort,
    PendingBudgetDep, RootWalk, ScopedDiscoveryPaths, Walker,
};
use std::collections::VecDeque;

const MAX_DIRECT_DEP_REFRESHES: u8 = 4;

struct SubgraphTopology {
    nodes: Vec<NodeId>,
    indegrees: HashMap<NodeId, usize>,
    edges: HashMap<NodeId, Vec<NodeId>>,
}

impl Walker<'_> {
    pub(in super::super::super) fn refresh_unwalked_scope_budget(
        &mut self,
        initial_pending: Vec<PendingBudgetDep>,
        root: &RootWalk,
    ) {
        if self.traversal.snapshot_budget.max_snapshots.is_none()
            || self.traversal.snapshot_budget.poll_cancelled()
            || self.traversal.snapshot_budget.is_stopped()
        {
            return;
        }
        if !initial_pending.is_empty() {
            self.walk_budget_only_direct_deps(initial_pending, root);
        }
        let mut refresh_counts: HashMap<NodeId, u8> = HashMap::default();
        loop {
            if self.traversal.snapshot_budget.poll_cancelled()
                || self.traversal.snapshot_budget.is_stopped()
            {
                break;
            }
            let dirty = std::mem::take(&mut self.traversal.snapshot_budget.dirty_providers);
            if dirty.is_empty() {
                break;
            }
            self.refresh_dirty_subgraph(&dirty, root, &mut refresh_counts);
        }
    }

    fn refresh_dirty_subgraph(
        &mut self,
        dirty: &HashSet<NodeId>,
        root: &RootWalk,
        refresh_counts: &mut HashMap<NodeId, u8>,
    ) {
        let Some(changed_cyclic) = self.update_cyclic_peer_sccs(dirty) else {
            return;
        };
        let Some(mut topology) = self.build_dirty_subgraph(dirty, &changed_cyclic) else {
            return;
        };
        let mut evicted = self.evict_subgraph_entries(&topology.nodes);
        let mut ready = initial_ready_nodes(&topology);
        let mut visited: HashSet<NodeId> = HashSet::default();
        let mut cursor = 0usize;
        while visited.len() < topology.nodes.len() {
            if self.traversal.snapshot_budget.poll_cancelled_amortized()
                || self.traversal.snapshot_budget.is_stopped()
            {
                return;
            }
            let next_id = next_subgraph_node(&mut ready, &topology.nodes, &visited, &mut cursor);
            if !visited.insert(next_id.clone()) {
                continue;
            }
            if let Some(entry) = evicted.remove(&next_id) {
                self.refresh_or_restore_entry(entry, &changed_cyclic, root, refresh_counts);
            }
            release_subgraph_successors(
                &next_id,
                &topology.edges,
                &visited,
                &mut topology.indegrees,
                &mut ready,
            );
        }
    }

    fn evict_subgraph_entries(&mut self, nodes: &[NodeId]) -> HashMap<NodeId, DirectDepBudget> {
        let mut evicted = HashMap::default();
        for node_id in nodes {
            if let Some(removed) = self.evict_direct_dep(node_id) {
                evicted.insert(node_id.clone(), removed);
            }
        }
        evicted
    }

    fn build_dirty_subgraph(
        &mut self,
        dirty: &HashSet<NodeId>,
        changed_cyclic: &HashSet<NodeId>,
    ) -> Option<SubgraphTopology> {
        let scoped = self.traversal.snapshot_budget.scope_paths.get(&self.opts.project_dir)?;
        let cancel = &self.traversal.snapshot_budget.cancel;
        let Some(mut nodes) = collect_reachable_dependents(scoped, dirty, cancel) else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return None;
        };
        for id in changed_cyclic {
            if scoped.by_direct_dep.contains_key(id) {
                nodes.push(id.clone());
            }
        }
        nodes.sort();
        nodes.dedup();
        if nodes.is_empty() {
            return None;
        }
        let Some(topology) = build_subgraph_topology(scoped, nodes, cancel) else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return None;
        };
        Some(topology)
    }

    fn refresh_or_restore_entry(
        &mut self,
        entry: DirectDepBudget,
        changed_cyclic: &HashSet<NodeId>,
        root: &RootWalk,
        refresh_counts: &mut HashMap<NodeId, u8>,
    ) {
        let node_id = &entry.direct_dep.node_id;
        let is_stale = changed_cyclic.contains(node_id)
            || !self.direct_dep_budget_matches(&entry, &root.importer_parents);
        if !is_stale {
            self.restore_direct_dep_budget(entry);
            return;
        }
        let count = refresh_counts.entry(node_id.clone()).or_default();
        if *count >= MAX_DIRECT_DEP_REFRESHES {
            self.traversal.snapshot_budget.mark_exceeded();
            return;
        }
        *count += 1;
        if !self.tree.dependencies_tree.contains_key(node_id) {
            return;
        }
        self.walk_single_budget_direct_dep(&entry.direct_dep, root);
    }

    fn walk_single_budget_direct_dep(&mut self, dep: &DirectDep, root: &RootWalk) {
        self.remember_parent_context_if_peer_provider(
            &dep.alias,
            &dep.node_id,
            &root.parent_dep_paths,
        );
        self.resolve_and_record_direct_budget(dep, root);
        self.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
    }

    fn walk_budget_only_direct_deps(&mut self, pending: Vec<PendingBudgetDep>, root: &RootWalk) {
        self.prime_pending_budget_contexts(&pending, root);
        for item in pending {
            if self.traversal.snapshot_budget.poll_cancelled_amortized()
                || self.traversal.snapshot_budget.is_stopped()
            {
                return;
            }
            if self.opts.scope.hoisted_peer_provider_node_ids.contains(&item.dep.node_id)
                && self.has_recorded_direct_budget(&item.dep.node_id)
            {
                continue;
            }
            self.resolve_or_restore_pending_dep(item, root);
        }
        self.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
    }

    fn prime_pending_budget_contexts(&mut self, pending: &[PendingBudgetDep], root: &RootWalk) {
        for item in pending {
            if !self.opts.scope.hoisted_peer_provider_node_ids.contains(&item.dep.node_id) {
                self.remember_parent_context_if_peer_provider(
                    &item.dep.alias,
                    &item.dep.node_id,
                    &root.parent_dep_paths,
                );
            }
        }
        for item in pending {
            if self.opts.scope.hoisted_peer_provider_node_ids.contains(&item.dep.node_id)
                && !self.has_recorded_direct_budget(&item.dep.node_id)
            {
                self.remember_parent_context_if_peer_provider(
                    &item.dep.alias,
                    &item.dep.node_id,
                    &root.parent_dep_paths,
                );
            }
        }
    }

    fn resolve_or_restore_pending_dep(&mut self, item: PendingBudgetDep, root: &RootWalk) {
        if let Some(entry) = item.saved {
            let can_restore =
                !self.traversal.snapshot_budget.dirty_providers.contains(&item.dep.node_id)
                    && self.direct_dep_budget_matches(&entry, &root.importer_parents);
            if can_restore {
                self.restore_direct_dep_budget(entry);
                return;
            }
        }
        self.resolve_and_record_direct_budget(&item.dep, root);
    }

    fn has_recorded_direct_budget(&self, node_id: &NodeId) -> bool {
        self.traversal.snapshot_budget.scope_paths
            .get(&self.opts.project_dir)
            .is_some_and(|scoped| scoped.by_direct_dep.contains_key(node_id))
    }

    fn resolve_and_record_direct_budget(&mut self, dep: &DirectDep, root: &RootWalk) {
        let output = self.resolve_node(&dep.node_id, &root.context());
        if self.traversal.snapshot_budget.is_stopped() {
            self.traversal.snapshot_budget.clear_in_flight();
        } else {
            self.record_direct_dep_budget(dep, &output, &root.importer_parents);
        }
    }
}

pub(super) fn collect_reachable_dependents(
    scoped: &ScopedDiscoveryPaths,
    dirty: &HashSet<NodeId>,
    cancel: &CancelState,
) -> Option<Vec<NodeId>> {
    let mut reachable: HashSet<NodeId> = HashSet::default();
    let mut stack: Vec<NodeId> = dirty.iter().cloned().collect();
    while let Some(provider_id) = stack.pop() {
        if cancel.poll_probe_amortized() {
            return None;
        }
        let Some(dependents) = scoped.dependents_by_provider.get(&provider_id) else {
            continue;
        };
        for dep_id in dependents {
            if scoped.by_direct_dep.contains_key(dep_id) && reachable.insert(dep_id.clone()) {
                stack.push(dep_id.clone());
            }
        }
    }
    let mut nodes: Vec<NodeId> = reachable.into_iter().collect();
    nodes.sort();
    Some(nodes)
}

fn build_subgraph_topology(
    scoped: &ScopedDiscoveryPaths,
    nodes: Vec<NodeId>,
    cancel: &CancelState,
) -> Option<SubgraphTopology> {
    let in_subgraph: HashSet<&NodeId> = nodes.iter().collect();
    let mut indegrees: HashMap<NodeId, usize> = HashMap::default();
    let mut edges: HashMap<NodeId, Vec<NodeId>> = HashMap::default();
    for node_id in &nodes {
        if cancel.poll_probe_amortized() {
            return None;
        }
        let preds = scoped.by_direct_dep
            .get(node_id)
            .map(|entry| collect_node_predecessors(entry, node_id, &in_subgraph))
            .unwrap_or_default();
        indegrees.insert(node_id.clone(), preds.len());
        for prov_id in preds {
            edges
                .entry(prov_id.clone())
                .or_default()
                .push(node_id.clone());
        }
    }
    Some(SubgraphTopology { nodes, indegrees, edges })
}

fn collect_node_predecessors<'entry>(
    entry: &'entry DirectDepBudget,
    node_id: &NodeId,
    in_subgraph: &HashSet<&NodeId>,
) -> HashSet<&'entry NodeId> {
    entry.provider_dep_paths
        .iter()
        .map(|(prov_id, _)| prov_id)
        .filter(|prov_id| *prov_id != node_id && in_subgraph.contains(prov_id))
        .collect()
}

fn initial_ready_nodes(topology: &SubgraphTopology) -> VecDeque<NodeId> {
    topology.nodes
        .iter()
        .filter(|id| {
            topology.indegrees
                .get(*id)
                .copied()
                .unwrap_or(0)
                == 0
        })
        .cloned()
        .collect()
}

fn next_subgraph_node(
    ready: &mut VecDeque<NodeId>,
    nodes: &[NodeId],
    visited: &HashSet<NodeId>,
    cursor: &mut usize,
) -> NodeId {
    if let Some(id) = ready.pop_front() {
        return id;
    }
    while *cursor < nodes.len() && visited.contains(&nodes[*cursor]) {
        *cursor += 1;
    }
    nodes[*cursor].clone()
}

fn release_subgraph_successors(
    node_id: &NodeId,
    edges: &HashMap<NodeId, Vec<NodeId>>,
    visited: &HashSet<NodeId>,
    indegrees: &mut HashMap<NodeId, usize>,
    ready: &mut VecDeque<NodeId>,
) {
    let Some(successors) = edges.get(node_id) else {
        return;
    };
    for succ in successors {
        if visited.contains(succ) {
            continue;
        }
        if let Some(degree) = indegrees.get_mut(succ) {
            *degree = degree.saturating_sub(1);
            if *degree == 0 {
                ready.push_back(succ.clone());
            }
        }
    }
}
