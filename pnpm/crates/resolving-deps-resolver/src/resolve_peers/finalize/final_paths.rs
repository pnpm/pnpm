use super::{
    BTreeMap, BTreeSet, DepPath, FinalPeerContext, HashMap, HashSet, NodeId, PeerId,
    PeerNameTarjan, PeerSccPass, Walker, create_peer_dep_graph_hash, link_path_to_peer_version,
    peer_id_pair, pkg_name,
};

impl Walker<'_> {
    /// Recompute every node's depPath with its resolved peers' *full*
    /// suffixes. Genuine peer cycles (detected as multi-node peer-graph
    /// SCCs, or self-loops) keep the `name@version` collapse; every other
    /// peer slot carries the peer's own depPath. The cycle detection
    /// runs synchronously over the already-walked graph.
    pub(in super::super) fn build_final_dep_paths(&self) -> HashMap<NodeId, DepPath> {
        let (_, scc_of) = self.peer_sccs();
        let cyclic_peer_names = self.cyclic_peer_names();
        let mut final_dep_paths: HashMap<NodeId, DepPath> = HashMap::default();
        let mut visiting = HashSet::default();
        let mut node_ids: Vec<NodeId> = self.node_external_peers.keys().cloned().collect();
        node_ids.sort();
        for node_id in node_ids {
            self.final_dep_path_for_node(
                &node_id,
                &scc_of,
                &cyclic_peer_names,
                &mut final_dep_paths,
                &mut visiting,
            );
        }
        final_dep_paths
    }

    pub(super) fn final_dep_path_for_node(
        &self,
        node_id: &NodeId,
        scc_of: &HashMap<NodeId, usize>,
        cyclic_peer_names: &HashSet<String>,
        final_dep_paths: &mut HashMap<NodeId, DepPath>,
        visiting: &mut HashSet<NodeId>,
    ) -> DepPath {
        let node_id = self.cache_owner_node_id(node_id);
        if let Some(dep_path) = final_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        let Some(peers) = self.node_external_peers.get(node_id) else {
            return self.provisional_dep_path_of(node_id);
        };
        if peers.is_empty() {
            return self.provisional_dep_path_of(node_id);
        }
        if !visiting.insert(node_id.clone()) {
            return self.provisional_dep_path_of(node_id);
        }
        let peer_ids: Vec<PeerId> = peers
            .iter()
            .map(|(peer_alias, peer_node_id)| {
                self.final_peer_id(
                    node_id,
                    peer_alias,
                    peer_node_id,
                    FinalPeerContext { scc_of, cyclic_peer_names },
                    final_dep_paths,
                    visiting,
                )
            })
            .collect();
        let suffix = create_peer_dep_graph_hash(&peer_ids, self.opts.peers_suffix_max_length);
        let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
        let dep_path = DepPath::from(format!("{}{}", self.tree.packages[pkg_id].id, suffix));
        final_dep_paths.insert(node_id.clone(), dep_path.clone());
        visiting.remove(node_id);
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
    pub(super) fn final_peer_id(
        &self,
        node_id: &NodeId,
        peer_alias: &str,
        peer_node_id: &NodeId,
        context: FinalPeerContext<'_>,
        final_dep_paths: &mut HashMap<NodeId, DepPath>,
        visiting: &mut HashSet<NodeId>,
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
            peer_id_pair(&pkg.result)
        };
        if self.opts.dedupe_peers && self.tree.dependencies_tree.contains_key(peer_node_id) {
            return pair();
        }
        if context
            .scc_of
            .get(node_id)
            .is_some_and(|node_scc| context.scc_of.get(peer_node_id) == Some(node_scc))
        {
            return pair();
        }
        if context.cyclic_peer_names.contains(peer_alias) {
            return pair();
        }
        PeerId::DepPath(self.final_dep_path_for_node(
            peer_node_id,
            context.scc_of,
            context.cyclic_peer_names,
            final_dep_paths,
            visiting,
        ))
    }

    /// The upstream `pathsByNodeId`: every walked node's final
    /// `DepPath`. Empty unless
    /// [`ResolvePeersOptions::collect_paths_by_node_id`](super::super::ResolvePeersOptions::collect_paths_by_node_id)
    /// asked for it.
    pub(in super::super) fn final_paths_by_node_id(
        &self,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> HashMap<NodeId, DepPath> {
        if !self.opts.collect_paths_by_node_id {
            return HashMap::default();
        }
        self.node_dep_paths
            .keys()
            .map(|node_id| (node_id.clone(), self.final_dep_path_of(node_id, final_dep_paths)))
            .collect()
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
        for (node_id, peers) in &self.node_external_peers {
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
            graph.entry(pkg_name(&self.tree.packages[pkg_id].result)).or_default().extend(edges);
        }
        let peer_names: Vec<&str> =
            graph.values().flat_map(|edges| edges.iter().copied()).collect();
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
        let mut participants: Vec<NodeId> = self
            .node_external_peers
            .iter()
            .filter(|(_, peers)| !peers.is_empty())
            .map(|(node_id, _)| self.cache_owner_node_id(node_id).clone())
            .collect();
        participants.sort();
        participants.dedup();
        let participant_set: HashSet<NodeId> = participants.iter().cloned().collect();
        let neighbors = |node_id: &NodeId| -> Vec<NodeId> {
            let mut out: Vec<NodeId> = self
                .node_external_peers
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
