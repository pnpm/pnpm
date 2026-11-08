use super::{
    Arc, BTreeMap, BTreeSet, FinalPeerContext, HashMap, HashSet, NodeId, PeerNameTarjan, Walker,
};

type PeerSccPass = crate::strongly_connected_components::SccPass<crate::NodeId>;

impl Walker<'_> {
    pub(super) fn is_cyclic_peer(
        node_id: &NodeId,
        peer_alias: &str,
        peer_node_id: &NodeId,
        context: FinalPeerContext<'_>,
    ) -> bool {
        context.scc_of
            .get(node_id)
            .is_some_and(|node_scc| context.scc_of.get(peer_node_id) == Some(node_scc))
            || context.cyclic_peer_names.contains(peer_alias)
    }

    /// When a cyclic peer slot is collapsed to `name@version`, the peer's own
    /// external peers are stripped from that slot. Propagate them onto the
    /// dependent node so two occurrences whose cyclic peers bound different
    /// transitive peers keep distinct final depPaths.
    pub(super) fn propagate_cyclic_external_peers(&mut self, context: FinalPeerContext<'_>) {
        if context.scc_of.is_empty() && context.cyclic_peer_names.is_empty() {
            return;
        }
        let occurrences_by_pkg = self.walked_occurrences_by_pkg();
        let mut candidate_ids: Vec<NodeId> = self.nodes.external_peers
            .iter()
            .filter(|(node_id, peers)| {
                !self.nodes.cache_owners.contains_key(*node_id)
                    && peers
                        .iter()
                        .any(|(alias, peer)| {
                            Self::is_cyclic_peer(
                                node_id,
                                alias,
                                self.cache_owner_node_id(peer),
                                context,
                            )
                        })
            })
            .map(|(node_id, _)| node_id.clone())
            .collect();
        candidate_ids.sort();
        loop {
            let mut changed = false;
            for node_id in &candidate_ids {
                changed |= self.propagate_node_cyclic_peers(node_id, context, &occurrences_by_pkg);
            }
            if !changed {
                break;
            }
        }
    }

    fn walked_occurrences_by_pkg(&self) -> HashMap<Arc<str>, Vec<NodeId>> {
        let mut by_pkg: HashMap<Arc<str>, Vec<NodeId>> = HashMap::default();
        for node_id in self.caches.node_dep_paths.keys() {
            let owner = self.cache_owner_node_id(node_id);
            let Some(tree_node) = self.tree.dependencies_tree.get(owner) else {
                continue;
            };
            by_pkg
                .entry(Arc::clone(&tree_node.resolved_package_id))
                .or_default()
                .push(owner.clone());
        }
        for occurrences in by_pkg.values_mut() {
            occurrences.sort();
            occurrences.dedup();
        }
        by_pkg
    }

    fn propagate_node_cyclic_peers(
        &mut self,
        node_id: &NodeId,
        context: FinalPeerContext<'_>,
        occurrences_by_pkg: &HashMap<Arc<str>, Vec<NodeId>>,
    ) -> bool {
        let Some(peers) = self.nodes.external_peers.get(node_id) else {
            return false;
        };
        let mut cyclic_targets: Vec<(&str, NodeId)> = peers
            .iter()
            .filter_map(|(alias, peer)| {
                let owner = self.cache_owner_node_id(peer);
                Self::is_cyclic_peer(node_id, alias, owner, context)
                    .then(|| (alias.as_str(), owner.clone()))
            })
            .collect();
        cyclic_targets.sort_by(|left, right| left.0.cmp(right.0));
        let mut additions: Vec<(String, NodeId)> = Vec::new();
        for (_, target_id) in cyclic_targets {
            self.collect_cyclic_target_peers(
                node_id,
                &target_id,
                context,
                occurrences_by_pkg,
                &mut additions,
            );
        }
        if additions.is_empty() {
            return false;
        }
        self.apply_propagated_peers(node_id, additions);
        true
    }

    fn collect_cyclic_target_peers(
        &self,
        node_id: &NodeId,
        target_id: &NodeId,
        context: FinalPeerContext<'_>,
        occurrences_by_pkg: &HashMap<Arc<str>, Vec<NodeId>>,
        additions: &mut Vec<(String, NodeId)>,
    ) {
        let (Some(peers), Some(target_peers), Some(target_tree_node)) = (
            self.nodes.external_peers.get(node_id),
            self.nodes.external_peers.get(target_id),
            self.tree.dependencies_tree.get(target_id),
        ) else {
            return;
        };
        let Some(occurrences) = occurrences_by_pkg.get(&target_tree_node.resolved_package_id)
        else {
            return;
        };
        for (alias, peer_node_id) in target_peers.iter() {
            let peer_owner = self.cache_owner_node_id(peer_node_id);
            if !peers.contains_key(alias)
                && !Self::is_cyclic_peer(target_id, alias, peer_owner, context)
                && !additions
                    .iter()
                    .any(|(added, _)| added == alias)
                && self.can_propagate_external_peer(node_id, alias, peer_owner)
                && self.peer_varies_across_occurrences(occurrences, alias, peer_owner)
            {
                additions.push((alias.clone(), peer_node_id.clone()));
            }
        }
    }

    fn peer_varies_across_occurrences(
        &self,
        occurrences: &[NodeId],
        peer_alias: &str,
        peer_owner: &NodeId,
    ) -> bool {
        occurrences
            .iter()
            .any(|other_id| {
                let Some(other_peer) = self.nodes.external_peers
                    .get(other_id)
                    .and_then(|peers| peers.get(peer_alias))
                else {
                    return true;
                };
                let mut visited = HashSet::default();
                !self.same_peer_occurrence(peer_owner, other_peer, &mut visited)
            })
    }

    fn same_peer_occurrence(
        &self,
        left_id: &NodeId,
        right_id: &NodeId,
        visited: &mut HashSet<(NodeId, NodeId)>,
    ) -> bool {
        let left = self.cache_owner_node_id(left_id);
        let right = self.cache_owner_node_id(right_id);
        if left == right || !visited.insert((left.clone(), right.clone())) {
            return true;
        }
        let (Some(left_node), Some(right_node)) =
            (self.tree.dependencies_tree.get(left), self.tree.dependencies_tree.get(right))
        else {
            return false;
        };
        if left_node.resolved_package_id != right_node.resolved_package_id {
            return false;
        }
        let left_peers = self.nodes.external_peers.get(left);
        let right_peers = self.nodes.external_peers.get(right);
        let left_len = left_peers.map_or(0, |peers| peers.len());
        let right_len = right_peers.map_or(0, |peers| peers.len());
        if left_len != right_len {
            return false;
        }
        let Some(left_peers) = left_peers else {
            return true;
        };
        left_peers
            .iter()
            .all(|(alias, left_target)| {
                right_peers
                    .and_then(|peers| peers.get(alias))
                    .is_some_and(|right_target| {
                        self.same_peer_occurrence(left_target, right_target, visited)
                    })
            })
    }

    fn can_propagate_external_peer(
        &self,
        node_id: &NodeId,
        peer_alias: &str,
        peer_owner: &NodeId,
    ) -> bool {
        if node_id == peer_owner {
            return false;
        }
        let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else {
            return false;
        };
        let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id) else {
            return false;
        };
        pkg.name().as_ref() != peer_alias && !pkg.peer_set.taken().contains_key(peer_alias)
    }

    fn apply_propagated_peers(&mut self, node_id: &NodeId, additions: Vec<(String, NodeId)>) {
        let Some(peers) = self.nodes.external_peers.get_mut(node_id) else {
            return;
        };
        let peers_mut = Arc::make_mut(peers);
        let mut record = self.output.node_records.get_mut(node_id);
        for (alias, peer_node_id) in additions {
            if let Some(record) = record.as_deref_mut() {
                record.transitive_peer_dependencies.insert(alias.clone());
            }
            peers_mut.insert(alias, peer_node_id);
        }
    }

    pub(super) fn cyclic_peer_names(&self) -> HashSet<String> {
        let graph = self.peer_name_graph();

        let mut tarjan = PeerNameTarjan {
            graph: &graph,
            index_of: HashMap::default(),
            low_of: HashMap::default(),
            on_stack: HashSet::default(),
            tarjan_stack: Vec::new(),
            cyclic: HashSet::default(),
            next_index: 0,
        };
        for name in graph.keys() {
            if !tarjan.index_of.contains_key(name.as_str()) {
                tarjan.strongconnect(name);
            }
        }
        tarjan.cyclic
    }

    /// The `package name → resolved peer names` graph, with every peer name
    /// present as a vertex of its own.
    ///
    /// Collected by package id, which every occurrence already carries, and
    /// rendered to names only when folding the result: the graph is over
    /// package *names*, of which a workspace has far fewer than the
    /// occurrences contributing to them, and rendering one costs an
    /// allocation (`PkgName` holds scope and bare name separately). Two
    /// package ids can share a name — different versions of one package — so
    /// the fold unions their edges.
    pub(super) fn peer_name_graph(&self) -> BTreeMap<String, BTreeSet<&str>> {
        let mut edges_of_pkg: HashMap<&str, BTreeSet<&str>> = HashMap::default();
        for (node_id, peers) in &self.nodes.external_peers {
            if peers.is_empty() {
                continue;
            }
            let pkg_id = &*self.tree.dependencies_tree[node_id].resolved_package_id;
            let edges = edges_of_pkg.entry(pkg_id).or_default();
            for peer_alias in peers.keys() {
                edges.insert(peer_alias.as_str());
            }
        }

        let mut graph: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
        for (pkg_id, edges) in edges_of_pkg {
            graph
                .entry(self.tree.packages[pkg_id].name().to_string())
                .or_default()
                .extend(edges);
        }
        let peer_names: Vec<&str> = graph
            .values()
            .flat_map(|edges| edges.iter().copied())
            .collect();
        for peer_name in peer_names {
            if !graph.contains_key(peer_name) {
                graph.insert(peer_name.to_string(), BTreeSet::default());
            }
        }
        graph
    }

    /// Strongly-connected components of the peer graph (node → resolved
    /// peers, restricted to peers that themselves carry peers — peerless
    /// peers can't close a cycle). Iterative Tarjan, returning the SCCs
    /// in reverse-topological order plus a `NodeId → SCC index` map.
    ///
    /// Vertices and edge targets are canonicalized through
    /// [`Self::cache_owner_node_id`] to match the owner-keyed lookups
    /// in [`Self::final_peer_id`]: a cycle through a cache-hit
    /// occurrence is a cycle through its owner.
    pub(super) fn peer_sccs(&self) -> (Vec<Vec<NodeId>>, HashMap<NodeId, usize>) {
        let mut participants: Vec<NodeId> = self.nodes.external_peers
            .iter()
            .filter(|(_, peers)| !peers.is_empty())
            .map(|(node_id, _)| self.cache_owner_node_id(node_id).clone())
            .collect();
        participants.sort();
        participants.dedup();
        let participant_set: HashSet<NodeId> = participants.iter().cloned().collect();
        let neighbors = |node_id: &NodeId| -> Vec<NodeId> {
            let mut out: Vec<NodeId> = self.nodes.external_peers
                .get(node_id)
                .into_iter()
                .flat_map(|peers| peers.values())
                .map(|peer| self.cache_owner_node_id(peer))
                .filter(|peer| participant_set.contains(*peer))
                .cloned()
                .collect();
            out.sort();
            out.dedup();
            out
        };

        let mut pass = PeerSccPass::default();
        for root in &participants {
            if pass.index_of.contains_key(root) {
                continue;
            }
            pass.visit_root(root, &neighbors);
        }
        (pass.sccs, pass.scc_of)
    }
}
