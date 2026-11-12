mod cyclic_peers;
mod live_paths;
mod refresh;

use super::{
    Arc, DepPath, DirectDep, HashMap, HashSet, NodeId, NodeOutput, ParentRef, ParentRefs,
    PeerWalkMode, RootWalk, Walker,
};
use crate::{PeerWalkAbort, ResolutionCancel};
use live_paths::LivePaths;
use refresh::{candidate_provider_ids, order_evicted_provider_first};
use std::path::PathBuf;

const CANCEL_POLL_MASK: u8 = 63;

#[derive(Debug)]
pub(in super::super) struct SubtreePaths {
    pub(super) own: DepPath,
    pub(super) children: Vec<Arc<SubtreePaths>>,
}

#[derive(Debug)]
pub(in super::super) struct DirectDepBudget {
    pub(super) direct_dep: DirectDep,
    pub(super) subtree: Arc<SubtreePaths>,
    pub(super) importer_refs: Vec<(String, Option<ParentRef>)>,
    pub(super) provider_dep_paths: Vec<(NodeId, Option<DepPath>)>,
}

#[derive(Debug, Default)]
pub(in super::super) struct ScopedDiscoveryPaths {
    pub(super) by_direct_dep: HashMap<NodeId, DirectDepBudget>,
    pub(super) dependents_by_provider: HashMap<NodeId, HashSet<NodeId>>,
    pub(super) canonical_subtrees: Vec<Arc<SubtreePaths>>,
}

impl ScopedDiscoveryPaths {
    fn insert_direct_dep(&mut self, entry: DirectDepBudget) -> Option<DirectDepBudget> {
        let node_id = entry.direct_dep.node_id.clone();
        let previous = self.remove_direct_dep(&node_id);
        for (provider_id, _) in &entry.provider_dep_paths {
            self.dependents_by_provider
                .entry(provider_id.clone())
                .or_default()
                .insert(node_id.clone());
        }
        self.by_direct_dep.insert(node_id, entry);
        previous
    }

    fn remove_direct_dep(&mut self, node_id: &NodeId) -> Option<DirectDepBudget> {
        let removed = self.by_direct_dep.remove(node_id)?;
        for (provider_id, _) in &removed.provider_dep_paths {
            if let Some(dependents) = self.dependents_by_provider.get_mut(provider_id) {
                dependents.remove(node_id);
                if dependents.is_empty() {
                    self.dependents_by_provider.remove(provider_id);
                }
            }
        }
        Some(removed)
    }
}

#[derive(Debug, Default)]
pub(in super::super) struct CancelState {
    probe: ResolutionCancel,
    poll_tick: u8,
}

impl CancelState {
    pub(in super::super) fn new(probe: ResolutionCancel) -> Self {
        Self { probe, poll_tick: 0 }
    }

    pub(in super::super) fn set_probe(&mut self, probe: ResolutionCancel) {
        self.probe = probe;
    }

    fn poll_probe(&self) -> bool {
        self.probe.is_cancelled()
    }

    fn poll_probe_amortized(&mut self) -> bool {
        if !self.probe.is_configured() {
            return false;
        }
        let should_poll = (self.poll_tick & CANCEL_POLL_MASK) == 0;
        self.poll_tick = self.poll_tick.wrapping_add(1);
        should_poll && self.probe.is_cancelled()
    }
}

#[derive(Debug, Default)]
pub(in super::super) struct SnapshotBudget {
    pub(in super::super) max_snapshots: Option<usize>,
    pub(in super::super) abort: Option<PeerWalkAbort>,
    pub(in super::super) cancel: CancelState,
    pub(in super::super) live_paths: LivePaths,
    pub(super) cached_subtrees: HashMap<DepPath, Arc<SubtreePaths>>,
    pub(super) scope_paths: HashMap<Option<PathBuf>, ScopedDiscoveryPaths>,
    pub(super) dirty_providers: HashSet<NodeId>,
    pub(super) cyclic_peers_of: HashMap<NodeId, Arc<HashSet<NodeId>>>,
}

impl SnapshotBudget {
    pub(in super::super) fn with_limits(
        max_snapshots: Option<usize>,
        cancel: ResolutionCancel,
    ) -> Self {
        Self { max_snapshots, cancel: CancelState::new(cancel), ..Self::default() }
    }

    #[must_use]
    pub(in super::super) fn is_stopped(&self) -> bool {
        self.abort.is_some()
    }

    pub(in super::super) fn mark_exceeded(&mut self) {
        self.abort.get_or_insert(PeerWalkAbort::SnapshotLimitExceeded);
    }

    pub(in super::super) fn poll_cancelled(&mut self) -> bool {
        match self.abort {
            Some(PeerWalkAbort::Cancelled) => return true,
            Some(PeerWalkAbort::SnapshotLimitExceeded) | None => {}
        }
        if self.cancel.poll_probe() {
            self.abort = Some(PeerWalkAbort::Cancelled);
            return true;
        }
        false
    }

    pub(in super::super) fn poll_cancelled_amortized(&mut self) -> bool {
        match self.abort {
            Some(PeerWalkAbort::Cancelled) => return true,
            Some(PeerWalkAbort::SnapshotLimitExceeded) | None => {}
        }
        if self.cancel.poll_probe_amortized() {
            self.abort = Some(PeerWalkAbort::Cancelled);
            return true;
        }
        false
    }

    pub(in super::super) fn is_cyclic_peer_pair(
        &self,
        node_id: &NodeId,
        peer_node_id: &NodeId,
    ) -> bool {
        self.cyclic_peers_of
            .get(node_id)
            .is_some_and(|group| group.contains(peer_node_id))
    }

    pub(in super::super) fn mark_dirty_provider(&mut self, node_id: &NodeId) {
        if self.max_snapshots.is_some() {
            self.dirty_providers.insert(node_id.clone());
        }
    }

    pub(in super::super) fn clear_in_flight(&mut self) {
        self.live_paths.clear_in_flight();
    }

    pub(in super::super) fn reset_for_view(&mut self) {
        self.live_paths.clear();
        self.cached_subtrees.clear();
        self.scope_paths.clear();
        self.dirty_providers.clear();
        self.cyclic_peers_of.clear();
    }

    pub(super) fn settle_discovery_subtree(
        &mut self,
        dep_path: &DepPath,
        children: Vec<Arc<SubtreePaths>>,
    ) -> Option<Arc<SubtreePaths>> {
        let max = self.max_snapshots?;
        if self.is_stopped() {
            return None;
        }
        if !self.live_paths.record_in_flight_dep_path(dep_path, max) {
            self.mark_exceeded();
            return None;
        }
        let subtree = Arc::new(SubtreePaths { own: dep_path.clone(), children });
        self.live_paths.mark_in_flight_subtree(&subtree);
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
        let max = self.max_snapshots?;
        if self.is_stopped() {
            return None;
        }
        let subtree = Arc::clone(
            self.cached_subtrees
                .entry(dep_path.clone())
                .or_insert_with(|| {
                    Arc::new(SubtreePaths { own: dep_path.clone(), children: Vec::new() })
                }),
        );
        if !self.live_paths.record_in_flight_subtree(&subtree, max) {
            self.mark_exceeded();
            return None;
        }
        Some(subtree)
    }
}

impl Walker<'_> {
    pub(super) fn evict_direct_dep(&mut self, node_id: &NodeId) -> Option<DirectDepBudget> {
        let removed = self.traversal.snapshot_budget.scope_paths
            .get_mut(&self.opts.project_dir)?
            .remove_direct_dep(node_id)?;
        self.traversal.snapshot_budget.live_paths.release_subtree(&removed.subtree);
        Some(removed)
    }

    pub(in super::super) fn evict_stale_scope_budget(
        &mut self,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> Vec<DirectDep> {
        self.traversal.snapshot_budget.dirty_providers.clear();
        if self.traversal.snapshot_budget.max_snapshots.is_none() {
            return Vec::new();
        }
        let (stale_ids, evicted_candidates) =
            self.find_stale_scope_ids(parents_direct, walk_direct, importer_parents);
        let cleared_canonical = parents_direct.is_empty()
            && self.traversal.snapshot_budget.scope_paths
                .get(&self.opts.project_dir)
                .is_some_and(|scoped| !scoped.canonical_subtrees.is_empty());
        if stale_ids.is_empty() && !cleared_canonical {
            return Vec::new();
        }
        for node_id in stale_ids {
            self.evict_direct_dep(&node_id);
        }
        if cleared_canonical {
            self.release_canonical_scope_subtrees();
        }
        order_evicted_provider_first(evicted_candidates)
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

    fn find_stale_scope_ids(
        &self,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        importer_parents: &ParentRefs,
    ) -> (Vec<NodeId>, Vec<(DirectDep, Vec<NodeId>)>) {
        let Some(scoped) = self.traversal.snapshot_budget.scope_paths.get(&self.opts.project_dir)
        else {
            return (Vec::new(), Vec::new());
        };
        let parent_by_id: HashMap<&NodeId, &DirectDep> = parents_direct
            .iter()
            .map(|dep| (&dep.node_id, dep))
            .collect();
        let walk_ids: HashSet<&NodeId> = walk_direct
            .iter()
            .map(|dep| &dep.node_id)
            .collect();
        let mut stale_ids = Vec::new();
        let mut evicted_candidates = Vec::new();
        for (node_id, entry) in &scoped.by_direct_dep {
            let parent_dep = parent_by_id.get(node_id).copied();
            let in_walk = walk_ids.contains(node_id);
            if parent_dep.is_some()
                && !in_walk
                && self.direct_dep_budget_matches(entry, importer_parents)
            {
                continue;
            }
            stale_ids.push(node_id.clone());
            if let Some(dep) = parent_dep
                && !in_walk
                && self.tree.dependencies_tree.contains_key(node_id)
            {
                evicted_candidates.push((
                    dep.clone(),
                    candidate_provider_ids(entry, importer_parents),
                ));
            }
        }
        (stale_ids, evicted_candidates)
    }

    pub(in super::super) fn record_direct_dep_budget(
        &mut self,
        dep: &DirectDep,
        output: &NodeOutput,
        importer_parents: &ParentRefs,
    ) {
        let Some(subtree) = output.subtree_dep_paths.as_ref() else {
            self.traversal.snapshot_budget.clear_in_flight();
            return;
        };
        let importer_refs = output.external_resolved_peers
            .keys()
            .chain(output.missing_peers.keys())
            .map(|name| (name.clone(), importer_parents.get(name).cloned()))
            .collect();
        let provider_dep_paths = output.external_resolved_peers
            .iter()
            .map(|(name, peer_node_id)| {
                let scoped_id = importer_parents
                    .get(name)
                    .and_then(|parent| parent.node_id.as_ref())
                    .unwrap_or(peer_node_id);
                (scoped_id.clone(), self.caches.node_dep_paths.get(scoped_id).cloned())
            })
            .collect();
        let entry = DirectDepBudget {
            direct_dep: dep.clone(),
            subtree: Arc::clone(subtree),
            importer_refs,
            provider_dep_paths,
        };
        let budget = &mut self.traversal.snapshot_budget;
        budget.live_paths.retain_subtree(subtree);
        budget.live_paths.clear_in_flight();
        let scope = self.opts.project_dir.clone();
        if let Some(previous) = budget.scope_paths
            .entry(scope)
            .or_default()
            .insert_direct_dep(entry)
        {
            budget.live_paths.release_subtree(&previous.subtree);
        }
    }

    pub(super) fn record_canonical_budget(&mut self, output: &NodeOutput) {
        let Some(subtree) = output.subtree_dep_paths.as_ref() else {
            self.traversal.snapshot_budget.clear_in_flight();
            return;
        };
        let scope = self.opts.project_dir.clone();
        let budget = &mut self.traversal.snapshot_budget;
        let canonical = &mut budget.scope_paths.entry(scope).or_default().canonical_subtrees;
        if !canonical.iter().any(|seen| Arc::ptr_eq(seen, subtree)) {
            canonical.push(Arc::clone(subtree));
            budget.live_paths.retain_subtree(subtree);
        }
        budget.live_paths.clear_in_flight();
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
