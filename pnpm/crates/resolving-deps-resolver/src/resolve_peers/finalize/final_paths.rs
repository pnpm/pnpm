use super::{
    DepPath, FinalPeerContext, HashMap, HashSet, NodeId, PeerId, Walker, link_path_to_peer_version,
    peer_id_pair,
};

/// The depPaths [`Walker::build_final_dep_paths`] settled on.
pub(crate) struct FinalDepPaths {
    pub(crate) by_node_id: HashMap<NodeId, DepPath>,
    /// The peer ids each peer-suffixed final depPath was built from, so a
    /// pass that later retargets those peers can build the suffix again.
    /// Empty unless [`Walker::build_final_dep_paths`] was asked for them.
    pub(crate) peer_ids: HashMap<DepPath, Vec<PeerId>>,
    recording: PeerIdRecording,
    visiting: HashSet<NodeId>,
}

/// Whether [`Walker::build_final_dep_paths`] keeps the peer ids behind
/// each peer-suffixed depPath, for a pass that retargets those peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerIdRecording {
    Record,
    Skip,
}

impl Walker<'_> {
    /// Recompute every node's depPath with its resolved peers' *full*
    /// suffixes. Genuine peer cycles (detected as multi-node peer-graph
    /// SCCs, or self-loops) keep the `name@version` collapse; every other
    /// peer slot carries the peer's own depPath. The cycle detection
    /// runs synchronously over the already-walked graph.
    pub(in super::super) fn build_final_dep_paths(
        &mut self,
        recording: PeerIdRecording,
    ) -> FinalDepPaths {
        let (_, scc_of) = self.peer_sccs();
        let cyclic_peer_names = self.cyclic_peer_names();
        let context = FinalPeerContext { scc_of: &scc_of, cyclic_peer_names: &cyclic_peer_names };
        if !self.opts.dedupe_peers {
            self.propagate_cyclic_external_peers(context);
        }
        let mut final_dep_paths = FinalDepPaths {
            recording,
            by_node_id: HashMap::default(),
            peer_ids: HashMap::default(),
            visiting: HashSet::default(),
        };
        let mut node_ids: Vec<NodeId> = self.nodes.external_peers
            .keys()
            .cloned()
            .collect();
        node_ids.sort();
        for node_id in node_ids {
            self.final_dep_path_for_node(&node_id, context, &mut final_dep_paths);
        }
        final_dep_paths
    }

    fn final_dep_path_for_node(
        &self,
        node_id: &NodeId,
        context: FinalPeerContext<'_>,
        final_dep_paths: &mut FinalDepPaths,
    ) -> DepPath {
        let node_id = self.cache_owner_node_id(node_id);
        if let Some(dep_path) = final_dep_paths.by_node_id.get(node_id) {
            return dep_path.clone();
        }
        let Some(peers) = self.nodes.external_peers.get(node_id) else {
            return self.provisional_dep_path_of(node_id);
        };
        if peers.is_empty() {
            return self.provisional_dep_path_of(node_id);
        }
        if !final_dep_paths.visiting.insert(node_id.clone()) {
            return self.provisional_dep_path_of(node_id);
        }
        let peer_ids: Vec<PeerId> = peers
            .iter()
            .map(|(peer_alias, peer_node_id)| {
                self.final_peer_id(node_id, peer_alias, peer_node_id, context, final_dep_paths)
            })
            .collect();
        let suffix =
            pnpm_lockfile::PeerBindings::suffix_of(&peer_ids, self.opts.peers_suffix_max_length);
        let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
        let dep_path = DepPath::from(format!("{}{}", self.tree.packages[pkg_id].id, suffix));
        match final_dep_paths.recording {
            PeerIdRecording::Record => {
                final_dep_paths.peer_ids.insert(dep_path.clone(), peer_ids);
            }
            PeerIdRecording::Skip => {}
        }
        final_dep_paths.by_node_id.insert(node_id.clone(), dep_path.clone());
        final_dep_paths.visiting.remove(node_id);
        dep_path
    }

    /// Resolve `node_id` to the depPath the rebuilt graph should key /
    /// reference it by. Prefers the corrected `final_dep_paths` entry
    /// and falls back to the provisional value for peerless nodes.
    pub(in super::super) fn final_dep_path_of(
        &self,
        node_id: &NodeId,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> DepPath {
        let node_id = self.cache_owner_node_id(node_id);
        if let Some(dep_path) = final_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        self.provisional_dep_path_of(node_id)
    }

    /// One resolved-peer slot of `node_id`'s suffix, computed against the
    /// already-finalized depPaths. Like [`Self::build_peer_id`] but
    /// substitutes the provisional cycle fallback with a strongly-
    /// connected-component test: a peer is collapsed to `name@version`
    /// only when it shares a peer-graph SCC with `node_id` (a genuine
    /// cycle). Non-cyclic peers carry their full depPath.
    fn final_peer_id(
        &self,
        node_id: &NodeId,
        peer_alias: &str,
        peer_node_id: &NodeId,
        context: FinalPeerContext<'_>,
        final_dep_paths: &mut FinalDepPaths,
    ) -> PeerId {
        let peer_node_id = self.cache_owner_node_id(peer_node_id);
        if let NodeId::Leaf(id) = peer_node_id
            && let Some(rel) = id.strip_prefix("link:")
        {
            return PeerId::Pair {
                name: peer_alias.to_string(),
                version: link_path_to_peer_version(rel),
            };
        }
        let pair = || {
            let tree_node = &self.tree.dependencies_tree[peer_node_id];
            let pkg = &self.tree.packages[&tree_node.resolved_package_id];
            peer_id_pair(pkg.result())
        };
        if self.opts.dedupe_peers && self.tree.dependencies_tree.contains_key(peer_node_id) {
            return pair();
        }
        if Self::is_cyclic_peer(node_id, peer_alias, peer_node_id, context) {
            return pair();
        }
        PeerId::DepPath(self.final_dep_path_for_node(peer_node_id, context, final_dep_paths))
    }

    /// The upstream `pathsByNodeId`: every walked node's final
    /// `DepPath`. Empty unless
    /// [`crate::PeerResolutionScope::collect_paths_by_node_id`](crate::PeerResolutionScope::collect_paths_by_node_id)
    /// asked for it.
    pub(in super::super) fn final_paths_by_node_id(
        &self,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> HashMap<NodeId, DepPath> {
        if !self.opts.scope.collect_paths_by_node_id {
            return HashMap::default();
        }
        self.caches.node_dep_paths
            .keys()
            .map(|node_id| (node_id.clone(), self.final_dep_path_of(node_id, final_dep_paths)))
            .collect()
    }
}
