use super::{
    Arc, DepPath, DirectDep, HashMap, HashSet, NodeId, NodeOutput, ParentRef, ParentRefs,
    PeerWalkMode, RootWalk, Walker,
};
use std::path::PathBuf;

#[derive(Debug)]
pub(in super::super) struct SubtreePaths {
    pub(super) own: DepPath,
    pub(super) children: Vec<Arc<SubtreePaths>>,
}

#[derive(Debug)]
pub(in super::super) struct DirectDepBudget {
    pub(super) subtree: Arc<SubtreePaths>,
    pub(super) importer_refs: Vec<(String, Option<ParentRef>)>,
    pub(super) provider_dep_paths: Vec<(NodeId, Option<DepPath>)>,
}

#[derive(Debug, Default)]
pub(in super::super) struct ScopedDiscoveryPaths {
    pub(super) by_direct_dep: HashMap<NodeId, DirectDepBudget>,
    pub(super) canonical_subtrees: Vec<Arc<SubtreePaths>>,
}

#[derive(Debug, Default)]
pub(in super::super) struct SnapshotBudget {
    pub(in super::super) max_snapshots: Option<usize>,
    pub(in super::super) exceeded: bool,
    pub(in super::super) discovery_dep_paths: HashSet<DepPath>,
    pub(super) cached_subtrees: HashMap<DepPath, Arc<SubtreePaths>>,
    pub(super) visited_subtrees: HashMap<usize, Arc<SubtreePaths>>,
    pub(super) scope_paths: HashMap<Option<PathBuf>, ScopedDiscoveryPaths>,
}

impl SnapshotBudget {
    pub(in super::super) fn new(max_snapshots: Option<usize>) -> Self {
        Self { max_snapshots, ..Self::default() }
    }

    pub(in super::super) fn reset_for_view(&mut self) {
        self.discovery_dep_paths.clear();
        self.cached_subtrees.clear();
        self.visited_subtrees.clear();
        self.scope_paths.clear();
    }

    pub(in super::super) fn record_discovery_dep_path(&mut self, dep_path: &DepPath) -> bool {
        let Some(max) = self.max_snapshots else {
            return true;
        };
        if !self.discovery_dep_paths.contains(dep_path)
            && self.discovery_dep_paths.len() >= max
        {
            self.exceeded = true;
            return false;
        }
        self.discovery_dep_paths.insert(dep_path.clone());
        true
    }

    pub(super) fn settle_discovery_subtree(
        &mut self,
        dep_path: &DepPath,
        children: Vec<Arc<SubtreePaths>>,
    ) -> Option<Arc<SubtreePaths>> {
        self.max_snapshots?;
        if !self.record_discovery_dep_path(dep_path) {
            return None;
        }
        let subtree = Arc::new(SubtreePaths { own: dep_path.clone(), children });
        self.visited_subtrees.insert(Arc::as_ptr(&subtree) as usize, Arc::clone(&subtree));
        self.cached_subtrees.insert(dep_path.clone(), Arc::clone(&subtree));
        Some(subtree)
    }

    pub(in super::super) fn replay_cached_subtree_for_mode(
        &mut self,
        mode: PeerWalkMode,
        dep_path: &DepPath,
    ) -> Option<Arc<SubtreePaths>> {
        match mode {
            PeerWalkMode::Discovery => self.replay_cached_subtree(dep_path),
            PeerWalkMode::Final => None,
        }
    }

    pub(in super::super) fn replay_cached_subtree(
        &mut self,
        dep_path: &DepPath,
    ) -> Option<Arc<SubtreePaths>> {
        self.max_snapshots?;
        let subtree = self.cached_subtrees
            .get(dep_path)
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(SubtreePaths { own: dep_path.clone(), children: Vec::new() })
            });
        self.record_subtree(&subtree);
        (!self.exceeded).then_some(subtree)
    }

    fn record_subtree(&mut self, root: &Arc<SubtreePaths>) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let ptr = Arc::as_ptr(node) as usize;
            if self.visited_subtrees.contains_key(&ptr) {
                continue;
            }
            if !self.record_discovery_dep_path(&node.own) {
                return;
            }
            self.visited_subtrees.insert(ptr, Arc::clone(node));
            stack.extend(node.children.iter());
        }
    }

    pub(super) fn rebuild_live_dep_paths(&mut self) {
        self.discovery_dep_paths.clear();
        self.visited_subtrees.clear();
        let mut stack: Vec<&Arc<SubtreePaths>> = Vec::new();
        for scope in self.scope_paths.values() {
            stack.extend(scope.by_direct_dep.values().map(|entry| &entry.subtree));
            stack.extend(scope.canonical_subtrees.iter());
        }
        while let Some(node) = stack.pop() {
            let ptr = Arc::as_ptr(node) as usize;
            if self.visited_subtrees
                .insert(ptr, Arc::clone(node))
                .is_some()
            {
                continue;
            }
            self.discovery_dep_paths.insert(node.own.clone());
            stack.extend(node.children.iter());
        }
    }
}

impl Walker<'_> {
    pub(in super::super) fn evict_stale_scope_budget(
        &mut self,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> Vec<DirectDep> {
        if self.traversal.snapshot_budget.max_snapshots.is_none() {
            return Vec::new();
        }
        let scope = self.opts.project_dir.clone();
        let (stale_ids, evicted_unwalked) =
            self.find_stale_scope_ids(parents_direct, walk_direct, importer_parents);
        let cleared_canonical = parents_direct.is_empty()
            && self.traversal.snapshot_budget.scope_paths
                .get(&scope)
                .is_some_and(|scoped| !scoped.canonical_subtrees.is_empty());
        if stale_ids.is_empty() && !cleared_canonical {
            return Vec::new();
        }
        let scoped = self.traversal.snapshot_budget.scope_paths.entry(scope).or_default();
        for node_id in stale_ids {
            scoped.by_direct_dep.remove(&node_id);
        }
        if cleared_canonical {
            scoped.canonical_subtrees.clear();
        }
        self.traversal.snapshot_budget.rebuild_live_dep_paths();
        evicted_unwalked
    }

    fn find_stale_scope_ids(
        &self,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> (Vec<NodeId>, Vec<DirectDep>) {
        let Some(scoped) = self.traversal.snapshot_budget.scope_paths.get(&self.opts.project_dir)
        else {
            return (Vec::new(), Vec::new());
        };
        let parent_ids: HashSet<NodeId> = parents_direct
            .iter()
            .map(|dep| dep.node_id.clone())
            .collect();
        let walk_ids: HashSet<NodeId> = walk_direct
            .iter()
            .map(|dep| dep.node_id.clone())
            .collect();
        let stale_ids: Vec<NodeId> = scoped.by_direct_dep
            .iter()
            .filter(|(node_id, entry)| {
                !parent_ids.contains(*node_id)
                    || walk_ids.contains(*node_id)
                    || !self.direct_dep_budget_matches(entry, importer_parents)
            })
            .map(|(node_id, _)| node_id.clone())
            .collect();
        let stale_set: HashSet<&NodeId> = stale_ids.iter().collect();
        let evicted_unwalked = parents_direct
            .iter()
            .filter(|dep| {
                stale_set.contains(&dep.node_id)
                    && !walk_ids.contains(&dep.node_id)
                    && self.tree.dependencies_tree.contains_key(&dep.node_id)
            })
            .cloned()
            .collect();
        (stale_ids, evicted_unwalked)
    }

    pub(in super::super) fn record_direct_dep_budget(
        &mut self,
        node_id: &NodeId,
        output: &NodeOutput,
        importer_parents: &ParentRefs,
    ) {
        let Some(subtree) = output.subtree_dep_paths.as_ref() else {
            return;
        };
        let importer_refs = output.external_resolved_peers
            .keys()
            .chain(output.missing_peers.keys())
            .map(|name| (name.clone(), importer_parents.get(name).cloned()))
            .collect();
        let provider_dep_paths = output.external_resolved_peers
            .values()
            .map(|peer_node_id| {
                (peer_node_id.clone(), self.caches.node_dep_paths.get(peer_node_id).cloned())
            })
            .collect();
        let scope = self.opts.project_dir.clone();
        self.traversal.snapshot_budget.scope_paths
            .entry(scope)
            .or_default()
            .by_direct_dep
            .insert(
                node_id.clone(),
                DirectDepBudget { subtree: Arc::clone(subtree), importer_refs, provider_dep_paths },
            );
    }

    pub(super) fn record_canonical_budget(&mut self, output: &NodeOutput) {
        let Some(subtree) = output.subtree_dep_paths.as_ref() else {
            return;
        };
        let scope = self.opts.project_dir.clone();
        let canonical = &mut self.traversal.snapshot_budget.scope_paths
            .entry(scope)
            .or_default()
            .canonical_subtrees;
        if !canonical.iter().any(|seen| Arc::ptr_eq(seen, subtree)) {
            canonical.push(Arc::clone(subtree));
        }
    }

    pub(in super::super) fn refresh_unwalked_scope_budget(
        &mut self,
        parents_direct: &[DirectDep],
        mut pending: Vec<DirectDep>,
        root: &RootWalk,
    ) {
        if self.traversal.snapshot_budget.max_snapshots.is_none()
            || self.traversal.snapshot_budget.exceeded
        {
            return;
        }
        let max_refreshes = parents_direct.len().saturating_add(1);
        let mut refresh_counts: HashMap<NodeId, usize> = HashMap::default();
        loop {
            pending.extend(self.evict_newly_stale_recorded_deps(
                parents_direct,
                root,
                &refresh_counts,
                max_refreshes,
            ));
            if pending.is_empty() || self.traversal.snapshot_budget.exceeded {
                break;
            }
            let batch = std::mem::take(&mut pending);
            for dep in &batch {
                *refresh_counts.entry(dep.node_id.clone()).or_default() += 1;
            }
            self.walk_budget_only_direct_deps(&batch, root);
        }
    }

    fn evict_newly_stale_recorded_deps(
        &mut self,
        parents_direct: &[DirectDep],
        root: &RootWalk,
        refresh_counts: &HashMap<NodeId, usize>,
        max_refreshes: usize,
    ) -> Vec<DirectDep> {
        let scope = self.opts.project_dir.clone();
        let Some(scoped) = self.traversal.snapshot_budget.scope_paths.get(&scope) else {
            return Vec::new();
        };
        let stale_ids: Vec<NodeId> = scoped.by_direct_dep
            .iter()
            .filter(|(_, entry)| !self.direct_dep_budget_matches(entry, &root.importer_parents))
            .map(|(node_id, _)| node_id.clone())
            .collect();
        if stale_ids.is_empty() {
            return Vec::new();
        }
        if stale_ids
            .iter()
            .any(|node_id| {
                refresh_counts
                    .get(node_id)
                    .copied()
                    .unwrap_or(0)
                    >= max_refreshes
            })
        {
            self.traversal.snapshot_budget.exceeded = true;
            return Vec::new();
        }
        let stale_set: HashSet<NodeId> = stale_ids.into_iter().collect();
        let scoped = self.traversal.snapshot_budget.scope_paths.entry(scope).or_default();
        for node_id in &stale_set {
            scoped.by_direct_dep.remove(node_id);
        }
        self.traversal.snapshot_budget.rebuild_live_dep_paths();
        parents_direct
            .iter()
            .filter(|dep| {
                stale_set.contains(&dep.node_id)
                    && self.tree.dependencies_tree.contains_key(&dep.node_id)
            })
            .cloned()
            .collect()
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
            if self.traversal.snapshot_budget.exceeded {
                return;
            }
            self.resolve_and_record_direct_budget(dep, root);
        }
        for dep in &provider_direct {
            if self.traversal.snapshot_budget.exceeded {
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
        self.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
    }

    fn has_recorded_direct_budget(&self, node_id: &NodeId) -> bool {
        self.traversal.snapshot_budget.scope_paths
            .get(&self.opts.project_dir)
            .is_some_and(|scoped| scoped.by_direct_dep.contains_key(node_id))
    }

    fn resolve_and_record_direct_budget(&mut self, dep: &DirectDep, root: &RootWalk) {
        let output = self.resolve_node(&dep.node_id, &root.context());
        if !self.traversal.snapshot_budget.exceeded {
            self.record_direct_dep_budget(&dep.node_id, &output, &root.importer_parents);
        }
    }

    fn direct_dep_budget_matches(
        &self,
        entry: &DirectDepBudget,
        importer_parents: &ParentRefs,
    ) -> bool {
        entry.importer_refs
            .iter()
            .all(|(name, recorded)| importer_parents.get(name) == recorded.as_ref())
            && entry.provider_dep_paths
                .iter()
                .all(|(node_id, dep_path)| {
                    self.caches.node_dep_paths.get(node_id) == dep_path.as_ref()
                })
    }
}
