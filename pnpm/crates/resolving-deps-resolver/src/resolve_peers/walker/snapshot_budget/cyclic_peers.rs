use super::{Arc, HashMap, HashSet, NodeId, PeerWalkAbort, Walker};
use crate::strongly_connected_components::SccPass;
use std::{cell::Cell, collections::VecDeque};

impl Walker<'_> {
    pub(super) fn update_cyclic_peer_sccs(
        &mut self,
        dirty: &HashSet<NodeId>,
    ) -> Option<HashSet<NodeId>> {
        let mut changed = HashSet::default();
        let Some(cleared_group_members) = self.prune_peerless_cyclic_members(dirty, &mut changed)
        else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return None;
        };
        let Some(participants) = self.collect_peer_scc_participants(dirty, &cleared_group_members)
        else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return None;
        };
        if participants.is_empty() {
            return Some(changed);
        }
        let Some(sccs) = self.compute_participant_sccs(&participants) else {
            self.traversal.snapshot_budget.abort = Some(PeerWalkAbort::Cancelled);
            return None;
        };
        for scc in sccs {
            if self.traversal.snapshot_budget.poll_cancelled_amortized() {
                return None;
            }
            self.apply_peer_scc(scc, &mut changed);
        }
        Some(changed)
    }

    fn prune_peerless_cyclic_members(
        &mut self,
        dirty: &HashSet<NodeId>,
        changed: &mut HashSet<NodeId>,
    ) -> Option<Vec<NodeId>> {
        let mut group_members = Vec::new();
        for id in dirty {
            if self.traversal.snapshot_budget.cancel.poll_probe_amortized() {
                return None;
            }
            if self.has_external_peers(id) {
                continue;
            }
            if let Some(previous_group) = self.traversal.snapshot_budget
                .cyclic_peers_of
                .remove(id)
            {
                changed.insert(id.clone());
                self.caches.peers_cache.evict_referencing(id);
                group_members.extend(previous_group.iter().cloned());
            }
        }
        Some(group_members)
    }

    fn collect_peer_scc_participants(
        &self,
        dirty: &HashSet<NodeId>,
        extra_roots: &[NodeId],
    ) -> Option<Vec<NodeId>> {
        let mut visited: HashSet<NodeId> = HashSet::default();
        let mut queue: VecDeque<NodeId> = VecDeque::new();
        for seed in self.seed_peer_scc_roots(dirty, extra_roots)? {
            if visited.insert(seed.clone()) {
                queue.push_back(seed);
            }
        }
        while let Some(node_id) = queue.pop_front() {
            if self.traversal.snapshot_budget.cancel.poll_probe_amortized() {
                return None;
            }
            self.enqueue_non_empty_peer_targets(&node_id, &mut visited, &mut queue);
            self.enqueue_existing_cyclic_group(&node_id, &mut visited, &mut queue);
        }
        let mut participants: Vec<NodeId> = visited.into_iter().collect();
        participants.sort();
        Some(participants)
    }

    fn seed_peer_scc_roots(
        &self,
        dirty: &HashSet<NodeId>,
        extra_roots: &[NodeId],
    ) -> Option<Vec<NodeId>> {
        let mut seeds: Vec<NodeId> = dirty
            .iter()
            .chain(extra_roots.iter())
            .filter(|id| self.has_external_peers(id))
            .cloned()
            .collect();
        if let Some(scoped) = self.traversal.snapshot_budget.scope_paths.get(&self.opts.project_dir)
        {
            let reachable = super::refresh::collect_reachable_dependents(
                scoped,
                dirty,
                &self.traversal.snapshot_budget.cancel,
            )?;
            seeds.extend(
                reachable
                    .into_iter()
                    .filter(|id| self.has_external_peers(id)),
            );
        }
        Some(seeds)
    }

    fn enqueue_non_empty_peer_targets(
        &self,
        node_id: &NodeId,
        visited: &mut HashSet<NodeId>,
        queue: &mut VecDeque<NodeId>,
    ) {
        let Some(peers) = self.nodes.external_peers.get(node_id) else {
            return;
        };
        for target_id in peers.values() {
            if self.has_external_peers(target_id) && visited.insert(target_id.clone()) {
                queue.push_back(target_id.clone());
            }
        }
    }

    fn enqueue_existing_cyclic_group(
        &self,
        node_id: &NodeId,
        visited: &mut HashSet<NodeId>,
        queue: &mut VecDeque<NodeId>,
    ) {
        let Some(group) = self.traversal.snapshot_budget.cyclic_peers_of.get(node_id) else {
            return;
        };
        for member in group.iter() {
            if self.has_external_peers(member) && visited.insert(member.clone()) {
                queue.push_back(member.clone());
            }
        }
    }

    fn has_external_peers(&self, node_id: &NodeId) -> bool {
        self.nodes.external_peers
            .get(node_id)
            .is_some_and(|peers| !peers.is_empty())
    }

    fn compute_participant_sccs(&self, participants: &[NodeId]) -> Option<Vec<Vec<NodeId>>> {
        let participant_set: HashSet<NodeId> = participants.iter().cloned().collect();
        let cancelled = Cell::new(false);
        let neighbors = |node_id: &NodeId| -> Vec<NodeId> {
            if cancelled.get() || self.traversal.snapshot_budget.cancel.poll_probe_amortized() {
                cancelled.set(true);
                return Vec::new();
            }
            let mut out: Vec<NodeId> = self.nodes.external_peers
                .get(node_id)
                .into_iter()
                .flat_map(|peers| peers.values())
                .filter(|peer| participant_set.contains(*peer))
                .cloned()
                .collect();
            out.sort();
            out.dedup();
            out
        };
        let mut pass = SccPass::default();
        for root in participants {
            if cancelled.get() || self.traversal.snapshot_budget.cancel.poll_probe_amortized() {
                return None;
            }
            if !pass.index_of.contains_key(root) {
                pass.visit_root(root, &neighbors);
            }
        }
        (!cancelled.get()).then_some(pass.sccs)
    }

    fn apply_peer_scc(&mut self, scc: Vec<NodeId>, changed: &mut HashSet<NodeId>) {
        let is_cyclic = scc.len() > 1 || (scc.len() == 1 && self.has_self_peer_edge(&scc[0]));
        if !is_cyclic {
            let member = &scc[0];
            if self.traversal.snapshot_budget.cyclic_peers_of.remove(member).is_some() {
                changed.insert(member.clone());
                self.caches.peers_cache.evict_referencing(member);
            }
            return;
        }
        let group: Arc<HashSet<NodeId>> = Arc::new(scc.into_iter().collect());
        let mut same_as_group: HashMap<usize, bool> = HashMap::default();
        let updated_members: Vec<NodeId> = group
            .iter()
            .filter(|member| {
                self.traversal.snapshot_budget.cyclic_peers_of
                    .get(*member)
                    .is_none_or(|prev| {
                        let ptr = Arc::as_ptr(prev) as usize;
                        !*same_as_group
                            .entry(ptr)
                            .or_insert_with(|| prev.as_ref() == group.as_ref())
                    })
            })
            .cloned()
            .collect();
        for member in updated_members {
            self.traversal.snapshot_budget.cyclic_peers_of.insert(
                member.clone(),
                Arc::clone(&group),
            );
            changed.insert(member.clone());
            self.caches.peers_cache.evict_referencing(&member);
        }
    }

    fn has_self_peer_edge(&self, node_id: &NodeId) -> bool {
        self.nodes.external_peers
            .get(node_id)
            .is_some_and(|peers| {
                peers
                    .values()
                    .any(|target| target == node_id)
            })
    }
}
