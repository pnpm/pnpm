use super::{
    DirectDep, DirectDepBudget, HashMap, HashSet, NodeId, ParentRefs, RootWalk,
    ScopedDiscoveryPaths, Walker,
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
        initial_pending: &[DirectDep],
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
        let changed_cyclic = self.update_cyclic_peer_sccs(dirty);
        let Some(mut topology) = self.build_dirty_subgraph(dirty, &changed_cyclic) else {
            return;
        };
        let mut ready = initial_ready_nodes(&topology);
        let mut visited: HashSet<NodeId> = HashSet::default();
        let mut cursor = 0usize;
        while visited.len() < topology.nodes.len() {
            if self.traversal.snapshot_budget.poll_cancelled()
                || self.traversal.snapshot_budget.is_stopped()
            {
                return;
            }
            let next_id = next_subgraph_node(&mut ready, &topology.nodes, &visited, &mut cursor);
            if !visited.insert(next_id.clone()) {
                continue;
            }
            self.refresh_single_if_stale(&next_id, &changed_cyclic, root, refresh_counts);
            release_subgraph_successors(
                &next_id,
                &topology.edges,
                &visited,
                &mut topology.indegrees,
                &mut ready,
            );
        }
    }

    fn build_dirty_subgraph(
        &self,
        dirty: &HashSet<NodeId>,
        changed_cyclic: &HashSet<NodeId>,
    ) -> Option<SubgraphTopology> {
        let scoped = self.traversal.snapshot_budget.scope_paths.get(&self.opts.project_dir)?;
        let mut nodes = collect_reachable_dependents(scoped, dirty);
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
        let (indegrees, edges) = build_subgraph_edges(scoped, &nodes);
        Some(SubgraphTopology { nodes, indegrees, edges })
    }

    fn refresh_single_if_stale(
        &mut self,
        node_id: &NodeId,
        changed_cyclic: &HashSet<NodeId>,
        root: &RootWalk,
        refresh_counts: &mut HashMap<NodeId, u8>,
    ) {
        let is_stale = changed_cyclic.contains(node_id)
            || self.traversal.snapshot_budget.scope_paths
                .get(&self.opts.project_dir)
                .and_then(|scoped| scoped.by_direct_dep.get(node_id))
                .is_some_and(|entry| {
                    !self.direct_dep_budget_matches(entry, &root.importer_parents)
                });
        if !is_stale {
            return;
        }
        let count = refresh_counts.entry(node_id.clone()).or_default();
        if *count >= MAX_DIRECT_DEP_REFRESHES {
            self.traversal.snapshot_budget.mark_exceeded();
            return;
        }
        *count += 1;
        let Some(removed) = self.evict_direct_dep(node_id) else {
            return;
        };
        if !self.tree.dependencies_tree.contains_key(&removed.direct_dep.node_id) {
            return;
        }
        self.walk_single_budget_direct_dep(&removed.direct_dep, root);
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

    fn walk_budget_only_direct_deps(&mut self, pending: &[DirectDep], root: &RootWalk) {
        let (own_direct, provider_direct): (Vec<&DirectDep>, Vec<&DirectDep>) = pending
            .iter()
            .partition(|dep| {
                !self.opts.scope.hoisted_peer_provider_node_ids.contains(&dep.node_id)
            });
        for dep in &own_direct {
            self.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &root.parent_dep_paths,
            );
        }
        for dep in &own_direct {
            if self.traversal.snapshot_budget.poll_cancelled()
                || self.traversal.snapshot_budget.is_stopped()
            {
                return;
            }
            self.resolve_and_record_direct_budget(dep, root);
        }
        self.walk_provider_budget_deps(&provider_direct, root);
        self.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
    }

    fn walk_provider_budget_deps(&mut self, provider_direct: &[&DirectDep], root: &RootWalk) {
        for dep in provider_direct {
            if self.traversal.snapshot_budget.poll_cancelled()
                || self.traversal.snapshot_budget.is_stopped()
            {
                return;
            }
            if self.has_recorded_direct_budget(&dep.node_id) {
                continue;
            }
            self.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &root.parent_dep_paths,
            );
            self.resolve_and_record_direct_budget(dep, root);
        }
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

pub(super) fn order_evicted_provider_first(
    candidates: Vec<(DirectDep, Vec<NodeId>)>,
) -> Vec<DirectDep> {
    if candidates.len() <= 1 {
        return candidates
            .into_iter()
            .map(|(dep, _)| dep)
            .collect();
    }
    let mut deps_by_id: HashMap<NodeId, DirectDep> = HashMap::default();
    let mut providers_by_id: HashMap<NodeId, Vec<NodeId>> = HashMap::default();
    let mut order_ids = Vec::with_capacity(candidates.len());
    for (dep, providers) in candidates {
        order_ids.push(dep.node_id.clone());
        providers_by_id.insert(dep.node_id.clone(), providers);
        deps_by_id.insert(dep.node_id.clone(), dep);
    }
    let mut state: HashMap<NodeId, u8> = HashMap::default();
    let mut sorted = Vec::with_capacity(order_ids.len());
    for id in &order_ids {
        visit_provider_first(id, &providers_by_id, &mut deps_by_id, &mut state, &mut sorted);
    }
    sorted
}

fn visit_provider_first(
    node_id: &NodeId,
    providers_by_id: &HashMap<NodeId, Vec<NodeId>>,
    deps_by_id: &mut HashMap<NodeId, DirectDep>,
    state: &mut HashMap<NodeId, u8>,
    sorted: &mut Vec<DirectDep>,
) {
    if state.contains_key(node_id) || !deps_by_id.contains_key(node_id) {
        return;
    }
    state.insert(node_id.clone(), 1);
    if let Some(providers) = providers_by_id.get(node_id) {
        for provider_id in providers {
            if provider_id != node_id {
                visit_provider_first(provider_id, providers_by_id, deps_by_id, state, sorted);
            }
        }
    }
    state.insert(node_id.clone(), 2);
    if let Some(dep) = deps_by_id.remove(node_id) {
        sorted.push(dep);
    }
}

pub(super) fn candidate_provider_ids(
    entry: &DirectDepBudget,
    importer_parents: &ParentRefs,
) -> Vec<NodeId> {
    let mut providers: Vec<NodeId> = entry.provider_dep_paths
        .iter()
        .map(|(id, _)| id.clone())
        .chain(
            entry.importer_refs
                .iter()
                .filter_map(|(name, _)| importer_parents.get(name)?.node_id.clone()),
        )
        .collect();
    providers.sort();
    providers.dedup();
    providers
}

pub(super) fn collect_reachable_dependents(
    scoped: &ScopedDiscoveryPaths,
    dirty: &HashSet<NodeId>,
) -> Vec<NodeId> {
    let mut reachable: HashSet<NodeId> = HashSet::default();
    let mut stack: Vec<NodeId> = dirty.iter().cloned().collect();
    while let Some(provider_id) = stack.pop() {
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
    nodes
}

fn build_subgraph_edges(
    scoped: &ScopedDiscoveryPaths,
    nodes: &[NodeId],
) -> (HashMap<NodeId, usize>, HashMap<NodeId, Vec<NodeId>>) {
    let in_subgraph: HashSet<&NodeId> = nodes.iter().collect();
    let mut indegrees: HashMap<NodeId, usize> = HashMap::default();
    let mut edges: HashMap<NodeId, Vec<NodeId>> = HashMap::default();
    for node_id in nodes {
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
    (indegrees, edges)
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
