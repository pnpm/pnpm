use super::{
    CancelState, DirectDep, DirectDepBudget, HashMap, HashSet, NodeId, ParentRefs, PeerWalkAbort,
    ScopedDiscoveryPaths, Walker, refresh::collect_reachable_dependents,
};

#[derive(Debug)]
pub(in super::super::super) struct PendingBudgetDep {
    pub(super) dep: DirectDep,
    pub(super) saved: Option<DirectDepBudget>,
}

struct EvictionPlan {
    direct_stale: HashSet<NodeId>,
    all_evict: Vec<NodeId>,
}

impl Walker<'_> {
    pub(in super::super::super) fn evict_stale_scope_budget(
        &mut self,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> Vec<PendingBudgetDep> {
        self.traversal.snapshot_budget.dirty_providers.clear();
        if self.traversal.snapshot_budget.max_snapshots.is_none() {
            return Vec::new();
        }
        let Some(plan) = self.plan_scope_eviction(parents_direct, walk_direct, importer_parents)
        else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return Vec::new();
        };
        let cleared_canonical = parents_direct.is_empty() && self.has_canonical_scope_subtrees();
        if plan.all_evict.is_empty() && !cleared_canonical {
            return Vec::new();
        }
        let candidates =
            self.execute_scope_eviction(&plan, parents_direct, walk_direct, importer_parents);
        if cleared_canonical {
            self.release_canonical_scope_subtrees();
        }
        let Some(sorted) =
            order_evicted_provider_first(candidates, &self.traversal.snapshot_budget.cancel)
        else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return Vec::new();
        };
        sorted
    }

    fn has_canonical_scope_subtrees(&self) -> bool {
        self.traversal.snapshot_budget.scope_paths
            .get(&self.opts.project_dir)
            .is_some_and(|scoped| !scoped.canonical_subtrees.is_empty())
    }

    fn plan_scope_eviction(
        &self,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> Option<EvictionPlan> {
        let Some(scoped) = self.traversal.snapshot_budget.scope_paths.get(&self.opts.project_dir)
        else {
            return Some(EvictionPlan { direct_stale: HashSet::default(), all_evict: Vec::new() });
        };
        let direct_stale =
            self.collect_direct_stale_ids(scoped, parents_direct, walk_direct, importer_parents)?;
        let mut seeds = direct_stale.clone();
        seeds.extend(walk_direct.iter().map(|dep| dep.node_id.clone()));
        let dependents =
            collect_reachable_dependents(scoped, &seeds, &self.traversal.snapshot_budget.cancel)?;
        let mut all_evict: Vec<NodeId> = direct_stale
            .iter()
            .cloned()
            .chain(dependents)
            .collect();
        all_evict.sort();
        all_evict.dedup();
        Some(EvictionPlan { direct_stale, all_evict })
    }

    fn collect_direct_stale_ids(
        &self,
        scoped: &ScopedDiscoveryPaths,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> Option<HashSet<NodeId>> {
        let parent_ids: HashSet<&NodeId> = parents_direct
            .iter()
            .map(|dep| &dep.node_id)
            .collect();
        let walk_ids: HashSet<&NodeId> = walk_direct
            .iter()
            .map(|dep| &dep.node_id)
            .collect();
        let mut direct_stale = HashSet::default();
        for (node_id, entry) in &scoped.by_direct_dep {
            if self.traversal.snapshot_budget.cancel.poll_probe_amortized() {
                return None;
            }
            let stays_valid = parent_ids.contains(node_id)
                && !walk_ids.contains(node_id)
                && self.direct_dep_budget_matches(entry, importer_parents);
            if !stays_valid {
                direct_stale.insert(node_id.clone());
            }
        }
        Some(direct_stale)
    }

    fn execute_scope_eviction(
        &mut self,
        plan: &EvictionPlan,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> Vec<(PendingBudgetDep, Vec<NodeId>)> {
        let parent_by_id: HashMap<&NodeId, &DirectDep> = parents_direct
            .iter()
            .map(|dep| (&dep.node_id, dep))
            .collect();
        let walk_ids: HashSet<&NodeId> = walk_direct
            .iter()
            .map(|dep| &dep.node_id)
            .collect();
        let mut candidates = Vec::new();
        for node_id in &plan.all_evict {
            let Some(removed) = self.evict_direct_dep(node_id) else {
                continue;
            };
            if let Some(dep) = parent_by_id.get(node_id).copied()
                && !walk_ids.contains(node_id)
                && self.tree.dependencies_tree.contains_key(node_id)
            {
                let providers = candidate_provider_ids(&removed, importer_parents);
                let saved = (!plan.direct_stale.contains(node_id)).then_some(removed);
                candidates.push((PendingBudgetDep { dep: dep.clone(), saved }, providers));
            }
        }
        candidates
    }

    fn release_canonical_scope_subtrees(&mut self) {
        let Some(scoped) =
            self.traversal.snapshot_budget.scope_paths.get_mut(&self.opts.project_dir)
        else {
            return;
        };
        let subtrees = std::mem::take(&mut scoped.canonical_subtrees);
        for subtree in &subtrees {
            self.traversal.snapshot_budget.live_paths.release_subtree(subtree);
        }
    }
}

fn candidate_provider_ids(entry: &DirectDepBudget, importer_parents: &ParentRefs) -> Vec<NodeId> {
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

fn order_evicted_provider_first(
    candidates: Vec<(PendingBudgetDep, Vec<NodeId>)>,
    cancel: &CancelState,
) -> Option<Vec<PendingBudgetDep>> {
    if candidates.len() <= 1 {
        return Some(
            candidates
                .into_iter()
                .map(|(item, _)| item)
                .collect(),
        );
    }
    let mut deps_by_id: HashMap<NodeId, PendingBudgetDep> = HashMap::default();
    let mut providers_by_id: HashMap<NodeId, Vec<NodeId>> = HashMap::default();
    let mut order_ids = Vec::with_capacity(candidates.len());
    for (item, providers) in candidates {
        let id = item.dep.node_id.clone();
        order_ids.push(id.clone());
        providers_by_id.insert(id.clone(), providers);
        deps_by_id.insert(id, item);
    }
    let mut state: HashMap<NodeId, u8> = HashMap::default();
    let mut sorted = Vec::with_capacity(order_ids.len());
    for root_id in &order_ids {
        if !visit_provider_first_iter(
            root_id,
            &providers_by_id,
            &mut deps_by_id,
            &mut state,
            &mut sorted,
            cancel,
        ) {
            return None;
        }
    }
    Some(sorted)
}

fn visit_provider_first_iter(
    root_id: &NodeId,
    providers_by_id: &HashMap<NodeId, Vec<NodeId>>,
    deps_by_id: &mut HashMap<NodeId, PendingBudgetDep>,
    state: &mut HashMap<NodeId, u8>,
    sorted: &mut Vec<PendingBudgetDep>,
    cancel: &CancelState,
) -> bool {
    if state.contains_key(root_id) || !deps_by_id.contains_key(root_id) {
        return true;
    }
    state.insert(root_id.clone(), 1);
    let mut stack = vec![(root_id.clone(), 0usize)];
    while let Some((node_id, idx)) = stack.last_mut() {
        if cancel.poll_probe_amortized() {
            return false;
        }
        if let Some(next_provider) =
            next_unvisited_provider(node_id, idx, providers_by_id, deps_by_id, state)
        {
            state.insert(next_provider.clone(), 1);
            stack.push((next_provider, 0));
            continue;
        }
        let (finished_id, _) = stack.pop().expect("non-empty provider DFS stack");
        state.insert(finished_id.clone(), 2);
        if let Some(item) = deps_by_id.remove(&finished_id) {
            sorted.push(item);
        }
    }
    true
}

fn next_unvisited_provider(
    node_id: &NodeId,
    idx: &mut usize,
    providers_by_id: &HashMap<NodeId, Vec<NodeId>>,
    deps_by_id: &HashMap<NodeId, PendingBudgetDep>,
    state: &HashMap<NodeId, u8>,
) -> Option<NodeId> {
    let providers = providers_by_id.get(node_id)?;
    while *idx < providers.len() {
        let candidate = &providers[*idx];
        *idx += 1;
        if candidate != node_id
            && !state.contains_key(candidate)
            && deps_by_id.contains_key(candidate)
        {
            return Some(candidate.clone());
        }
    }
    None
}
