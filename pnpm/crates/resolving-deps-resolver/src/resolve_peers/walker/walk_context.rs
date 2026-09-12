use super::{
    AncestorIds, Arc, BTreeMap, ChildEdge, DepPath, DirectDep, HashMap, HashSet, NodeId,
    ParentPkgInfo, ParentRefs, PeerProviderChildren, ResolvedPackage, ResolvedTree, SharedChain,
    UndoRealize, Walker, pkg_name_version,
};

/// Output of [`Walker::resolve_node`] — the per-node result the parent
/// folds into its own state.
pub(in super::super) struct NodeOutput {
    pub(in super::super) dep_path: DepPath,
    /// Peers that this node + its subtree resolved against ancestors.
    /// Excludes peers resolved against this node's own children (those
    /// are absorbed into the children's depPaths).
    pub(in super::super) external_resolved_peers: Arc<HashMap<String, NodeId>>,
    /// Real `peerDependencies` resolved anywhere in this node's
    /// subtree. This feeds the auto-install-peers loop.
    pub(in super::super) auto_install_resolved_peers: HashMap<String, NodeId>,
    pub(in super::super) missing_peers: Arc<HashMap<String, MissingPeerInfo>>,
    /// [`ResolvePeersResult::missing_names_by_pkg`](crate::ResolvePeersResult::missing_names_by_pkg)'s per-subtree
    /// slice, propagated bottom-up so discovery can aggregate it from
    /// the importer's direct deps alone.
    pub(in super::super) subtree_missing_by_pkg: SubtreeMissingByPkg,
}

/// Sentinel for "this node's subtree is still missing peer `X`". The
/// `range` + `optional` payload is recorded for a future `peersCache`
/// lookup, but the issue-collection path uses
/// [`PeerDependencyIssues::missing`](crate::PeerDependencyIssues::missing) directly, so neither field is read
/// after construction yet.
#[derive(Debug, Clone)]
pub(in super::super) struct MissingPeerInfo {
    #[allow(dead_code, reason = "future peersCache validation")]
    pub(in super::super) range: String,
    #[allow(dead_code, reason = "future peersCache validation")]
    pub(in super::super) optional: bool,
}

/// Persistent summary of missing peers in a subtree. Child summaries
/// are shared with their parents instead of repeatedly copying every
/// descendant's package map into each ancestor and cache entry.
#[derive(Debug)]
pub(crate) struct MissingSummary {
    pub(super) own: Option<(String, HashSet<String>)>,
    pub(super) children: Vec<Arc<MissingSummary>>,
}

pub(in super::super) type SubtreeMissingByPkg = Option<Arc<MissingSummary>>;

pub(super) enum ChildAliases<'a> {
    Realized(&'a BTreeMap<String, NodeId>),
    Deferred(&'a [ChildEdge]),
}

impl ChildAliases<'_> {
    pub(super) fn contains(&self, alias: &str) -> bool {
        match self {
            ChildAliases::Realized(children) => children.contains_key(alias),
            ChildAliases::Deferred(children) => children.iter().any(|edge| edge.alias == alias),
        }
    }
}

#[derive(Default)]
pub(super) struct ChildOutputs {
    pub(super) external_peers: HashMap<String, NodeId>,
    pub(super) auto_install_resolved_peers: HashMap<String, NodeId>,
    pub(super) missing_peers: HashMap<String, MissingPeerInfo>,
    pub(super) dep_paths: BTreeMap<String, DepPath>,
    pub(super) missing_summaries: Vec<Arc<MissingSummary>>,
}

impl ChildOutputs {
    pub(super) fn push(
        &mut self,
        alias: &str,
        output: NodeOutput,
        child_aliases: &ChildAliases<'_>,
        collect_dep_paths: bool,
    ) {
        let NodeOutput {
            dep_path,
            external_resolved_peers,
            auto_install_resolved_peers,
            missing_peers,
            subtree_missing_by_pkg,
        } = output;
        if let Some(summary) = subtree_missing_by_pkg
            && !self.missing_summaries.iter().any(|existing| Arc::ptr_eq(existing, &summary))
        {
            self.missing_summaries.push(summary);
        }
        if collect_dep_paths {
            self.dep_paths.insert(alias.to_string(), dep_path);
        }
        self.auto_install_resolved_peers.extend(auto_install_resolved_peers);
        for (peer_alias, peer_node_id) in external_resolved_peers.iter() {
            if !child_aliases.contains(peer_alias) {
                self.external_peers.insert(peer_alias.clone(), peer_node_id.clone());
            }
        }
        self.missing_peers
            .extend(missing_peers.iter().map(|(name, info)| (name.clone(), info.clone())));
    }
}

/// Index, for every package id in the tree, which of its child edges can
/// stand in as a peer-dependency provider. Entries carried over from an
/// earlier walk are left alone.
pub(super) fn index_peer_provider_children(
    tree: &ResolvedTree,
    index: &mut HashMap<String, PeerProviderChildren>,
) {
    // With no peer names in the tree, no edge can index as a
    // provider: every entry stays the empty default.
    let tree_declares_peers = !tree.all_peer_dep_names.is_empty();
    for (pkg_id, children) in &tree.children_by_id {
        if index.contains_key(&**pkg_id) {
            continue;
        }
        let mut providers = PeerProviderChildren::default();
        if tree_declares_peers {
            for (edge_index, edge) in children.iter().enumerate() {
                index_peer_provider_edge(tree, &mut providers, edge_index, edge);
            }
        }
        index.insert(pkg_id.to_string(), providers);
    }
}

/// An edge is indexed under every peer name it can satisfy: its install
/// alias, the package's real name, or both when they differ.
pub(super) fn index_peer_provider_edge(
    tree: &ResolvedTree,
    providers: &mut PeerProviderChildren,
    edge_index: usize,
    edge: &ChildEdge,
) {
    let Some(pkg) = tree.packages.get(&edge.pkg_id) else { return };
    let real_name = pkg_name_version(&pkg.result).0;
    let alias_is_peer = tree.all_peer_dep_names.contains(&edge.alias);
    let real_name_is_peer = tree.all_peer_dep_names.contains(&real_name);
    if !alias_is_peer && !real_name_is_peer {
        return;
    }
    providers.relevant_edge_indices.push(edge_index);
    if alias_is_peer {
        providers.edge_indices_by_name.entry(edge.alias.clone()).or_default().push(edge_index);
    }
    if real_name_is_peer && real_name != edge.alias {
        providers.edge_indices_by_name.entry(real_name).or_default().push(edge_index);
    }
}

/// What one wanted-lockfile peer-context entry is validated against.
pub(super) struct LockedPinContext<'a> {
    pub(super) pkg: &'a ResolvedPackage,
    pub(super) provider_paths: &'a HashMap<NodeId, DepPath>,
    pub(super) parent_refs: &'a ParentRefs,
    pub(super) parent_node_ids: &'a SharedChain<NodeId>,
}

/// The parent context one node is resolved against: [`Walker::resolve_node`]'s
/// tail parameters, grouped.
pub(in super::super) struct NodeWalkContext<'a> {
    pub(in super::super) parent_refs: &'a Arc<ParentRefs>,
    pub(in super::super) parent_dep_paths: &'a Arc<HashMap<String, ParentPkgInfo>>,
    pub(in super::super) chain_names: &'a SharedChain<String>,
    pub(in super::super) parent_node_ids: &'a SharedChain<NodeId>,
    pub(in super::super) parent_pkg_ids: &'a SharedChain<String>,
}

/// The importer-level walk context: the direct deps' parents and the
/// empty ancestor chains.
pub(in super::super) struct RootWalk {
    pub(in super::super) importer_parents: Arc<ParentRefs>,
    pub(in super::super) parent_dep_paths: Arc<HashMap<String, ParentPkgInfo>>,
    pub(super) chain_names: SharedChain<String>,
    pub(super) parent_node_ids: SharedChain<NodeId>,
    pub(super) parent_pkg_ids: SharedChain<String>,
}

impl RootWalk {
    pub(in super::super) fn of(walker: &Walker<'_>, parents_direct: &[DirectDep]) -> Self {
        let importer_parents = Arc::new(walker.build_importer_parents_from(parents_direct));
        let parent_dep_paths = walker.parent_dep_paths_from_refs(&importer_parents);
        Self {
            importer_parents,
            parent_dep_paths,
            chain_names: SharedChain::default(),
            parent_node_ids: SharedChain::default(),
            parent_pkg_ids: SharedChain::default(),
        }
    }

    pub(in super::super) fn context(&self) -> NodeWalkContext<'_> {
        NodeWalkContext {
            parent_refs: &self.importer_parents,
            parent_dep_paths: &self.parent_dep_paths,
            chain_names: &self.chain_names,
            parent_node_ids: &self.parent_node_ids,
            parent_pkg_ids: &self.parent_pkg_ids,
        }
    }
}

/// The occurrence's package and tree-node facts, read once on entry.
pub(super) struct NodeEntry {
    pub(super) pkg: Arc<ResolvedPackage>,
    pub(super) pkg_name: String,
    pub(super) depth: i32,
    pub(super) installable: bool,
    pub(super) provider_children: BTreeMap<String, NodeId>,
    pub(super) preview_undo: Option<UndoRealize>,
}

/// The ancestor chains the node's children walk under.
pub(super) struct ChildChains {
    pub(super) names: SharedChain<String>,
    pub(super) node_ids: SharedChain<NodeId>,
    pub(super) pkg_ids: SharedChain<String>,
}

impl ChildChains {
    pub(super) fn context<'c>(
        &'c self,
        parent_refs: &'c Arc<ParentRefs>,
        parent_dep_paths: &'c Arc<HashMap<String, ParentPkgInfo>>,
    ) -> NodeWalkContext<'c> {
        NodeWalkContext {
            parent_refs,
            parent_dep_paths,
            chain_names: &self.names,
            parent_node_ids: &self.node_ids,
            parent_pkg_ids: &self.pkg_ids,
        }
    }
}

/// What walking a node's children produced.
pub(super) struct ChildrenWalk {
    pub(super) outputs: ChildOutputs,
    pub(super) children_map: Arc<BTreeMap<String, NodeId>>,
    pub(super) discovery_children: Option<(Arc<Vec<ChildEdge>>, AncestorIds)>,
    pub(super) realize_undo: Option<UndoRealize>,
    pub(super) chains: ChildChains,
}

/// The node's peer verdict, shared with its caches and records.
pub(super) struct SettledPeers {
    pub(super) dep_path: DepPath,
    pub(super) own_resolved: HashMap<String, NodeId>,
    pub(super) all_resolved: Arc<HashMap<String, NodeId>>,
    pub(super) all_missing: Arc<HashMap<String, MissingPeerInfo>>,
    pub(super) missing_from_children: Arc<HashMap<String, MissingPeerInfo>>,
    pub(super) subtree_missing_by_pkg: SubtreeMissingByPkg,
    pub(super) is_pure: bool,
}

/// The still-lazy children a discovery walk descends into.
#[derive(Clone, Copy)]
pub(super) struct DeferredChildren<'a> {
    pub(super) pkg_id: &'a Arc<str>,
    pub(super) children: &'a [ChildEdge],
    pub(super) parent_ids: &'a AncestorIds,
    pub(super) provider_children: &'a BTreeMap<String, NodeId>,
    /// The parent's own depth; the children sit one below it.
    pub(super) depth: i32,
}

/// What one walked node contributes to the per-`pkgIdWithPatchHash` caches.
pub(super) struct WalkResult<'a> {
    pub(super) dep_path: &'a DepPath,
    pub(super) all_resolved_peers: &'a Arc<HashMap<String, NodeId>>,
    pub(super) all_missing_peers: &'a Arc<HashMap<String, MissingPeerInfo>>,
    pub(super) missing_peers_of_children: &'a Arc<HashMap<String, MissingPeerInfo>>,
    pub(super) subtree_missing_by_pkg: &'a Option<Arc<MissingSummary>>,
    pub(super) is_pure: bool,
}

/// The [`ParentRefs`] view a node hands down to its descendants,
/// as [`Walker::build_child_parent_refs`] computes it.
pub(super) struct ChildParentRefs {
    pub(super) refs: Arc<ParentRefs>,
    /// Only what this node itself contributed: its peer-relevant
    /// children.
    pub(super) own: ParentRefs,
    /// Whether `refs` says anything the caller's map didn't. `false`
    /// lets the caller pass its own parent-context snapshot down
    /// instead of rebuilding an identical one.
    pub(super) changed: bool,
}

/// The peers of one node, as [`Walker::resolve_node_peers`] resolves
/// them, and the depPath the combined set renders to.
pub(super) struct NodePeers {
    /// The node's own `peerDependencies`, resolved.
    pub(super) own_resolved: HashMap<String, NodeId>,
    /// The above folded with what the subtree resolved against
    /// ancestors, minus the node's own name — the set `dep_path`'s
    /// suffix renders.
    pub(super) all_resolved: HashMap<String, NodeId>,
    pub(super) all_missing: HashMap<String, MissingPeerInfo>,
    pub(super) dep_path: DepPath,
}

pub(super) struct NodePeersContext<'a> {
    pub(super) pkg: &'a ResolvedPackage,
    pub(super) pkg_name: &'a str,
    /// The augmented refs visible at this node, including its own
    /// peer-relevant children.
    pub(super) parent_refs: &'a ParentRefs,
    pub(super) chain_names: &'a SharedChain<String>,
    pub(super) ancestor_pkg_ids: &'a SharedChain<String>,
    /// Taken by value: it is the base the combined resolved-peer map is
    /// built on, so folding into it costs no extra map.
    pub(super) external_from_children: HashMap<String, NodeId>,
    pub(super) missing_from_children: &'a HashMap<String, MissingPeerInfo>,
}
