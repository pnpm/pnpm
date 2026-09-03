//! The peer-resolution walk itself: [`Walker`], its state, the
//! per-importer entry [`Walker::walk`], the recursive
//! [`Walker::resolve_node`], and the peer matching each visited node
//! performs against its parent context.

use crate::{
    dependencies_graph::{
        DependenciesGraph, MissingPeer, ParentChain, PeerDependencyIssue, PeerDependencyIssues,
    },
    node_id::NodeId,
    resolve_peers::{
        ResolvePeersOptions, ResolvePeersResult,
        cache::{
            CacheHitContext, DeferredChildContext, PeerProviderChildren, PeersCacheItem,
            merge_realize_undo,
        },
        context::{
            ComparablePeerRange, CurrentProviderSource, ParentPkgInfo, ParentRef, ParentRefs,
            SharedChain, importer_relative_link_dep_path, insert_parent_ref,
            link_node_id_as_dep_path, peer_id_pair, pkg_name_version, remap_link_node_id,
            satisfies_with_prereleases,
        },
        discovery::PeerDiscoveryCaches,
        finalize::{NodeRecord, PendingPeerEdge, WalkedNode},
    },
    resolved_tree::{
        AncestorIds, ChildEdge, DirectDep, PeerDep, ResolvedPackage, ResolvedTree, TreeChildren,
    },
};
use pnpm_deps_path::{
    DepPath, PeerId, create_peer_dep_graph_hash, index_of_dep_path_suffix,
    link_path_to_peer_version,
};
use pnpm_resolving_resolver_base::get_peer_version_range;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct Walker<'tree> {
    pub(super) tree: &'tree mut ResolvedTree,
    pub(super) opts: ResolvePeersOptions,
    pub(super) graph: DependenciesGraph,
    pub(super) issues: PeerDependencyIssues,
    pub(super) missing_ancestor_pkg_ids: HashMap<String, Vec<SharedChain<String>>>,
    /// `NodeId → DepPath` once a node has been walked. Lets repeated
    /// visits (an importer-direct dep that's also reached transitively)
    /// reuse the already-computed depPath.
    pub(super) node_dep_paths: HashMap<NodeId, DepPath>,
    /// Peers each node and its subtree resolved against ancestors —
    /// the "unknown resolved peers" propagated up so a parent can fold
    /// its descendants' peer dependencies into its own peer suffix.
    /// Indexed by `NodeId`; value's keys are peer aliases.
    pub(super) node_external_peers: HashMap<NodeId, Arc<HashMap<String, NodeId>>>,
    /// Cache-hit occurrence → fully walked occurrence that produced the
    /// reused peer-resolution verdict.
    pub(super) cache_owner_by_node_id: HashMap<NodeId, NodeId>,
    /// Peers each node and its subtree declared but couldn't find.
    /// Indexed by `NodeId`; value's keys are peer aliases.
    pub(super) node_missing_peers: HashMap<NodeId, Arc<HashMap<String, MissingPeerInfo>>>,
    /// Peers each node's children declared but couldn't find.
    /// Indexed by `NodeId`; value's keys are peer aliases.
    pub(super) node_missing_peers_of_children:
        HashMap<NodeId, Arc<HashMap<String, MissingPeerInfo>>>,
    /// Resolver-stage real peer providers seen while walking the tree.
    /// This intentionally excludes `peerDependenciesMeta`-only entries:
    /// the auto-install pass reads real `peerDependencies` entries only.
    resolved_peer_providers_by_alias: BTreeMap<String, NodeId>,
    /// Stack of nodes currently being walked. Re-entry on a node here
    /// is a cycle — the recursion bottoms out with a `name@version`
    /// peer-id and the original visit drives the actual graph insert.
    pub(super) in_progress: HashSet<NodeId>,
    /// Graph edges whose target `NodeId` had no `DepPath` yet at the
    /// time we built the parent's `graph_children` map — typically
    /// because the target is a later sibling direct dep that the walker
    /// hasn't reached yet. `walk()` drains this list once every direct
    /// dep is walked and patches the recorded entries with the now-known
    /// `DepPath`. Without this post-pass the install layer
    /// would walk the parent's `children` map and find no symlink edge
    /// for the child, leaving the package without it in its slot.
    pub(super) pending_peer_edges: Vec<PendingPeerEdge>,
    /// Membership guard for [`Walker::pending_peer_edges`], keeping the
    /// buffer free of exact duplicates. Cleared whenever the buffer drains.
    pub(super) pending_peer_edge_keys: HashSet<(DepPath, String, NodeId)>,
    /// Set of `pkgIdWithPatchHash` values whose full subtree resolved
    /// with zero external peers and zero missing peers. A revisit of
    /// any such package whose own `peerDependencies` is empty
    /// short-circuits with `depPath = pkgIdWithPatchHash` — no
    /// recursion, no peersCache lookup.
    /// Populated bottom-up: a node is added when its local `is_pure`
    /// flag is true after its own walk completes.
    pub(super) pure_pkgs: HashMap<String, DepPath>,
    /// Per-`pkgIdWithPatchHash` cached results from earlier walks of
    /// non-pure subtrees. Each cache item records the `depPath`, the
    /// external `(peer_name → NodeId)` map, and the `(peer_name →
    /// info)` missing set produced by one specific parent peer
    /// context. [`Walker::find_hit`] iterates the bucket and accepts
    /// the first item whose cached context is compatible with the
    /// current call's `child_parent_refs`.
    ///
    /// [`Walker::find_hit`] calls [`Walker::parent_packages_match`] for
    /// the deep check, which compares against the parent context
    /// snapshots held in [`Walker::parent_pkgs_of_node`].
    pub(super) peers_cache: HashMap<String, Vec<PeersCacheItem>>,
    /// Per-`NodeId` snapshot of the parent peer context (peer-relevant
    /// names → [`ParentPkgInfo`]) recorded at the moment the walker
    /// first descended into that node. Backs
    /// [`Walker::parent_packages_match`]: a [`PeersCacheItem`] is a
    /// cache hit only when each of its resolved-peer `NodeId`s has an
    /// entry here whose recorded parent context still matches the
    /// current walk's `parent_refs` (or, for `purePkgs` peers, the
    /// presence-and-pkg-id match short-circuit).
    pub(super) parent_pkgs_of_node: HashMap<NodeId, Arc<HashMap<String, ParentPkgInfo>>>,
    pub(super) retained_peer_node_ids: HashSet<NodeId>,
    /// Per-`NodeId` snapshot captured at graph-insert time, consumed by
    /// the post-walk [`Walker::build_final_dep_paths`] /
    /// [`Walker::build_final_graph`] pass. See [`NodeRecord`].
    pub(super) node_records: HashMap<NodeId, NodeRecord>,
    pub(super) next_record_order: u64,
    /// Reverse index over the tree nodes' `previous_dep_path`, built
    /// only when [`ResolvePeersOptions::resolved_peer_provider_paths`]
    /// is set. The upstream `nodeIdsByPreviousDepPath`.
    node_ids_by_previous_dep_path: HashMap<DepPath, NodeId>,
    /// Importers whose direct dependencies count as "current" peer
    /// providers for the must-win guard. Swapped per importer by the
    /// workspace entry point.
    pub(super) current_provider_sources: Vec<CurrentProviderSource>,
    /// `true` for a peer-hoist discovery pass: the walk records no
    /// graph entries, node records, or pending edges, and the caller
    /// runs none of the final depPath/graph passes. Everything that
    /// decides *what* resolves or goes missing is unchanged.
    pub(super) discovery: bool,
    /// Nodes this call resolved (any return path except the cycle
    /// re-entry). Distinguishes them from nodes only known through the
    /// persistent [`PeerDiscoveryCaches`], so the pruned-provider
    /// fallback keeps its per-call meaning.
    pub(super) visited_this_call: HashSet<NodeId>,
    packages_by_id: HashMap<String, Arc<ResolvedPackage>>,
    pub(super) peer_provider_children_by_pkg_id: HashMap<String, PeerProviderChildren>,
    peer_provider_index_peer_names: HashSet<String>,
    /// Children-graph SCC ids behind the canonical cycle gate: every
    /// intra-SCC edge whose target is not canonically later
    /// (package-id order) is cut, the same cut at every occurrence, so
    /// realized subtrees are entry-independent and no walk path can
    /// revisit a package. Built lazily once per walker; the tree's
    /// children are frozen for the walker's lifetime.
    children_sccs: std::cell::OnceCell<Arc<HashMap<Arc<str>, usize>>>,
    pub(super) empty_resolved_peers: Arc<HashMap<String, NodeId>>,
    pub(super) empty_missing_peers: Arc<HashMap<String, MissingPeerInfo>>,
    /// The shared record-only occurrence per canonical back-edge
    /// target: every back-edge to a package references this one node,
    /// walked once at importer-root context via
    /// [`Self::pending_canonical_nodes`].
    canonical_backedge_nodes: HashMap<Arc<str>, NodeId>,
    /// Canonical back-edge targets realized but not yet walked; the
    /// walk drivers drain this after their direct-dep loops.
    pub(super) pending_canonical_nodes: Vec<NodeId>,
    /// Set while [`Self::drain_pending_canonical_nodes`] walks a shared
    /// back-edge target at importer context: those walks must not emit
    /// missing-peer issues — every real position reports its own state,
    /// and an importer-context miss there would demand an auto-install
    /// the positions do not need.
    in_canonical_drain: bool,
    /// Raw `peerDependencies` range → its comparable form. Peer-heavy
    /// workspaces declare the same few ranges across many nodes, so the
    /// walk parses each distinct one once. Scoped to the walk: the
    /// mapping is a pure function of the raw range, and nothing outside
    /// it needs the entries.
    comparable_peer_ranges: HashMap<String, Arc<ComparablePeerRange>>,
}

impl<'tree> Walker<'tree> {
    pub(super) fn new(
        tree: &'tree mut ResolvedTree,
        opts: ResolvePeersOptions,
        node_ids_by_previous_dep_path: HashMap<DepPath, NodeId>,
        current_provider_sources: Vec<CurrentProviderSource>,
        caches: PeerDiscoveryCaches,
        discovery: bool,
    ) -> Self {
        let PeerDiscoveryCaches {
            node_dep_paths,
            pure_pkgs,
            peers_cache,
            parent_pkgs_of_node,
            retained_peer_node_ids,
            mut peer_provider_children_by_pkg_id,
            mut peer_provider_index_peer_names,
            canonical_backedge_nodes,
        } = caches;
        if peer_provider_index_peer_names != tree.all_peer_dep_names {
            peer_provider_children_by_pkg_id.clear();
            peer_provider_index_peer_names.clone_from(&tree.all_peer_dep_names);
        }
        // With no peer names in the tree, no edge can index as a
        // provider: every entry stays the empty default.
        let tree_declares_peers = !tree.all_peer_dep_names.is_empty();
        for (pkg_id, children) in &tree.children_by_id {
            if peer_provider_children_by_pkg_id.contains_key(&**pkg_id) {
                continue;
            }
            let mut providers = PeerProviderChildren::default();
            if tree_declares_peers {
                for (edge_index, edge) in children.iter().enumerate() {
                    let Some(pkg) = tree.packages.get(&edge.pkg_id) else { continue };
                    let real_name = pkg_name_version(&pkg.result).0;
                    let alias_is_peer = tree.all_peer_dep_names.contains(&edge.alias);
                    let real_name_is_peer = tree.all_peer_dep_names.contains(&real_name);
                    if !alias_is_peer && !real_name_is_peer {
                        continue;
                    }
                    providers.relevant_edge_indices.push(edge_index);
                    if alias_is_peer {
                        providers
                            .edge_indices_by_name
                            .entry(edge.alias.clone())
                            .or_default()
                            .push(edge_index);
                    }
                    if real_name_is_peer && real_name != edge.alias {
                        providers
                            .edge_indices_by_name
                            .entry(real_name)
                            .or_default()
                            .push(edge_index);
                    }
                }
            }
            peer_provider_children_by_pkg_id
                .insert(std::sync::Arc::<str>::clone(pkg_id).to_string(), providers);
        }
        Walker {
            tree,
            opts,
            graph: DependenciesGraph::default(),
            issues: PeerDependencyIssues::default(),
            missing_ancestor_pkg_ids: HashMap::default(),
            node_dep_paths,
            node_external_peers: HashMap::default(),
            cache_owner_by_node_id: HashMap::default(),
            node_missing_peers: HashMap::default(),
            node_missing_peers_of_children: HashMap::default(),
            resolved_peer_providers_by_alias: BTreeMap::new(),
            in_progress: HashSet::default(),
            pending_peer_edges: Vec::new(),
            pending_peer_edge_keys: HashSet::default(),
            pure_pkgs,
            peers_cache,
            parent_pkgs_of_node,
            retained_peer_node_ids,
            node_records: HashMap::default(),
            next_record_order: 0,
            node_ids_by_previous_dep_path,
            current_provider_sources,
            discovery,
            visited_this_call: HashSet::default(),
            packages_by_id: HashMap::default(),
            peer_provider_children_by_pkg_id,
            peer_provider_index_peer_names,
            children_sccs: std::cell::OnceCell::new(),
            empty_resolved_peers: Arc::new(HashMap::default()),
            empty_missing_peers: Arc::new(HashMap::default()),
            canonical_backedge_nodes,
            pending_canonical_nodes: Vec::new(),
            in_canonical_drain: false,
            comparable_peer_ranges: HashMap::default(),
        }
    }

    /// The cached [`ComparablePeerRange`] for `raw_range`, building it
    /// on the first request. Shared out behind an [`Arc`] so the caller
    /// can keep it while taking `&mut self` again.
    fn comparable_peer_range(&mut self, raw_range: &str) -> Arc<ComparablePeerRange> {
        if let Some(range) = self.comparable_peer_ranges.get(raw_range) {
            return Arc::clone(range);
        }
        let range = Arc::new(ComparablePeerRange::new(raw_range));
        self.comparable_peer_ranges.insert(raw_range.to_string(), Arc::clone(&range));
        range
    }

    pub(super) fn into_caches(self) -> PeerDiscoveryCaches {
        PeerDiscoveryCaches {
            node_dep_paths: self.node_dep_paths,
            pure_pkgs: self.pure_pkgs,
            peers_cache: self.peers_cache,
            parent_pkgs_of_node: self.parent_pkgs_of_node,
            retained_peer_node_ids: self.retained_peer_node_ids,
            peer_provider_children_by_pkg_id: self.peer_provider_children_by_pkg_id,
            peer_provider_index_peer_names: self.peer_provider_index_peer_names,
            canonical_backedge_nodes: self.canonical_backedge_nodes,
        }
    }

    /// The children-graph SCC table behind the canonical cycle gate;
    /// see [`Walker::children_sccs`].
    pub(super) fn canonical_scc(&self) -> Arc<HashMap<Arc<str>, usize>> {
        Arc::clone(self.children_sccs.get_or_init(|| Arc::new(children_scc_ids(self.tree))))
    }

    /// Whether the peer walk drops the `pkg_id → child_pkg_id` edge:
    /// a self-edge, or an intra-SCC edge whose target is not later in
    /// package-id order — the same answer at every occurrence.
    pub(super) fn cuts_cycle_edge(
        scc_of: &HashMap<Arc<str>, usize>,
        pkg_id: &str,
        child_pkg_id: &str,
    ) -> bool {
        pkg_id == child_pkg_id
            || match (scc_of.get(pkg_id), scc_of.get(child_pkg_id)) {
                (Some(pkg_scc), Some(child_scc)) => pkg_scc == child_scc && child_pkg_id <= pkg_id,
                _ => false,
            }
    }

    /// The shared record-only node a canonical back-edge references;
    /// created lazily and queued for the driver's importer-context
    /// walk. See [`Self::canonical_backedge_nodes`].
    pub(super) fn canonical_backedge_node(&mut self, pkg_id: &Arc<str>, depth: i32) -> NodeId {
        if self.tree.packages.get(&**pkg_id).is_some_and(|pkg| pkg.is_leaf) {
            let node_id = NodeId::leaf(pkg_id);
            if !self.tree.dependencies_tree.contains_key(&node_id) {
                self.tree.dependencies_tree.insert(
                    node_id.clone(),
                    crate::resolved_tree::DependenciesTreeNode::new(
                        Arc::clone(pkg_id),
                        TreeChildren::Lazy { parent_ids: AncestorIds::default() },
                        depth,
                        true,
                    ),
                );
            }
            return node_id;
        }
        if let Some(node_id) = self.canonical_backedge_nodes.get(&**pkg_id)
            && self.tree.dependencies_tree.contains_key(node_id)
        {
            return node_id.clone();
        }
        let node_id = NodeId::next();
        self.tree.dependencies_tree.insert(
            node_id.clone(),
            crate::resolved_tree::DependenciesTreeNode::new(
                Arc::clone(pkg_id),
                TreeChildren::Lazy { parent_ids: AncestorIds::default() },
                depth,
                true,
            ),
        );
        self.canonical_backedge_nodes.insert(Arc::clone(pkg_id), node_id.clone());
        self.pending_canonical_nodes.push(node_id.clone());
        node_id
    }

    /// Walk every queued canonical back-edge target at importer-root
    /// context. Targets realized during these walks queue more, so the
    /// drain loops until quiet.
    pub(super) fn drain_pending_canonical_nodes(
        &mut self,
        importer_parents: &Arc<ParentRefs>,
        importer_parent_dep_paths: &Arc<HashMap<String, super::context::ParentPkgInfo>>,
    ) {
        self.in_canonical_drain = true;
        while let Some(node_id) = self.pending_canonical_nodes.pop() {
            if self.visited_this_call.contains(&node_id) {
                continue;
            }
            self.resolve_node(
                &node_id,
                importer_parents,
                importer_parent_dep_paths,
                &SharedChain::default(),
                &SharedChain::default(),
                &SharedChain::default(),
            );
        }
        self.in_canonical_drain = false;
    }
}

/// Output of [`Walker::resolve_node`] — the per-node result the parent
/// folds into its own state.
pub(super) struct NodeOutput {
    pub(super) dep_path: DepPath,
    /// Peers that this node + its subtree resolved against ancestors.
    /// Excludes peers resolved against this node's own children (those
    /// are absorbed into the children's depPaths).
    pub(super) external_resolved_peers: Arc<HashMap<String, NodeId>>,
    /// Real `peerDependencies` resolved anywhere in this node's
    /// subtree. This feeds the auto-install-peers loop.
    pub(super) auto_install_resolved_peers: HashMap<String, NodeId>,
    pub(super) missing_peers: Arc<HashMap<String, MissingPeerInfo>>,
    /// [`ResolvePeersResult::missing_names_by_pkg`]'s per-subtree
    /// slice, propagated bottom-up so discovery can aggregate it from
    /// the importer's direct deps alone.
    pub(super) subtree_missing_by_pkg: SubtreeMissingByPkg,
}

/// Sentinel for "this node's subtree is still missing peer `X`". The
/// `range` + `optional` payload is recorded for a future `peersCache`
/// lookup, but the issue-collection path uses
/// [`PeerDependencyIssues::missing`] directly, so neither field is read
/// after construction yet.
#[derive(Debug, Clone)]
pub(super) struct MissingPeerInfo {
    #[allow(dead_code, reason = "future peersCache validation")]
    pub(super) range: String,
    #[allow(dead_code, reason = "future peersCache validation")]
    pub(super) optional: bool,
}

/// Persistent summary of missing peers in a subtree. Child summaries
/// are shared with their parents instead of repeatedly copying every
/// descendant's package map into each ancestor and cache entry.
#[derive(Debug)]
pub(crate) struct MissingSummary {
    own: Option<(String, HashSet<String>)>,
    children: Vec<Arc<MissingSummary>>,
}

pub(super) type SubtreeMissingByPkg = Option<Arc<MissingSummary>>;

enum ChildAliases<'a> {
    Realized(&'a BTreeMap<String, NodeId>),
    Deferred(&'a [ChildEdge]),
}

impl ChildAliases<'_> {
    fn contains(&self, alias: &str) -> bool {
        match self {
            ChildAliases::Realized(children) => children.contains_key(alias),
            ChildAliases::Deferred(children) => children.iter().any(|edge| edge.alias == alias),
        }
    }
}

#[derive(Default)]
struct ChildOutputs {
    external_peers: HashMap<String, NodeId>,
    auto_install_resolved_peers: HashMap<String, NodeId>,
    missing_peers: HashMap<String, MissingPeerInfo>,
    dep_paths: BTreeMap<String, DepPath>,
    missing_summaries: Vec<Arc<MissingSummary>>,
}

impl ChildOutputs {
    fn push(
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

/// The [`ParentRefs`] view a node hands down to its descendants,
/// as [`Walker::build_child_parent_refs`] computes it.
struct ChildParentRefs {
    refs: Arc<ParentRefs>,
    /// Only what this node itself contributed: its peer-relevant
    /// children.
    own: ParentRefs,
    /// Whether `refs` says anything the caller's map didn't. `false`
    /// lets the caller pass its own parent-context snapshot down
    /// instead of rebuilding an identical one.
    changed: bool,
}

/// The peers of one node, as [`Walker::resolve_node_peers`] resolves
/// them, and the depPath the combined set renders to.
struct NodePeers {
    /// The node's own `peerDependencies`, resolved.
    own_resolved: HashMap<String, NodeId>,
    /// The above folded with what the subtree resolved against
    /// ancestors, minus the node's own name — the set `dep_path`'s
    /// suffix renders.
    all_resolved: HashMap<String, NodeId>,
    all_missing: HashMap<String, MissingPeerInfo>,
    dep_path: DepPath,
}

struct NodePeersContext<'a> {
    pkg: &'a ResolvedPackage,
    pkg_name: &'a str,
    /// The augmented refs visible at this node, including its own
    /// peer-relevant children.
    parent_refs: &'a ParentRefs,
    chain_names: &'a SharedChain<String>,
    ancestor_pkg_ids: &'a SharedChain<String>,
    /// Taken by value: it is the base the combined resolved-peer map is
    /// built on, so folding into it costs no extra map.
    external_from_children: HashMap<String, NodeId>,
    missing_from_children: &'a HashMap<String, MissingPeerInfo>,
}

impl Walker<'_> {
    pub(super) fn walk(mut self) -> ResolvePeersResult {
        let importer_parents = Arc::new(self.build_importer_parents());
        let parent_chain_names = SharedChain::default();
        let parent_node_ids = SharedChain::default();
        let parent_pkg_ids_chain = SharedChain::default();
        let mut direct_by_alias = BTreeMap::new();
        // Clone direct deps into an owned `Vec` so the recursion
        // below can mutate `self.tree` (realising lazy children)
        // without conflicting with this loop's borrow of
        // `self.tree.direct`.
        let direct: Vec<DirectDep> = self.tree.direct.clone();
        let importer_parent_dep_paths = self.parent_dep_paths_from_refs(&importer_parents);
        let (own_direct, provider_direct): (Vec<&DirectDep>, Vec<&DirectDep>) = direct
            .iter()
            .partition(|dep| !self.opts.hoisted_peer_provider_node_ids.contains(&dep.node_id));
        for dep in &own_direct {
            self.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &importer_parent_dep_paths,
            );
        }
        for dep in &own_direct {
            let output = self.resolve_node(
                &dep.node_id,
                &importer_parents,
                &importer_parent_dep_paths,
                &parent_chain_names,
                &parent_node_ids,
                &parent_pkg_ids_chain,
            );
            for (peer_alias, peer_node_id) in output.auto_install_resolved_peers {
                self.resolved_peer_providers_by_alias.insert(peer_alias, peer_node_id);
            }
        }
        self.drain_pending_canonical_nodes(&importer_parents, &importer_parent_dep_paths);
        // See ResolvePeersOptions::hoisted_peer_provider_node_ids — a
        // provider is normally resolved at its tree position during the
        // walk above; only one whose position was pruned still needs the
        // root-context fallback.
        for dep in &provider_direct {
            if self.visited_this_call.contains(&dep.node_id) {
                continue;
            }
            self.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &importer_parent_dep_paths,
            );
            let output = self.resolve_node(
                &dep.node_id,
                &importer_parents,
                &importer_parent_dep_paths,
                &parent_chain_names,
                &parent_node_ids,
                &parent_pkg_ids_chain,
            );
            for (peer_alias, peer_node_id) in output.auto_install_resolved_peers {
                self.resolved_peer_providers_by_alias.insert(peer_alias, peer_node_id);
            }
        }
        self.drain_pending_canonical_nodes(&importer_parents, &importer_parent_dep_paths);
        self.patch_pending_peer_edges();
        // Recompute depPaths so each resolved peer carries its full
        // suffix (the cycle fallback during the walk collapses peers
        // that are walk-ancestors), then rebuild the graph from the
        // per-node records keyed by the corrected depPaths.
        let final_dep_paths = self.build_final_dep_paths();
        let anchor = match (self.opts.project_dir.as_deref(), self.opts.lockfile_dir.as_deref()) {
            (Some(project_dir), Some(lockfile_dir)) => {
                crate::link_target::ImporterAnchor::new(project_dir, lockfile_dir)
            }
            _ => crate::link_target::ImporterAnchor::default(),
        };
        for dep in &direct {
            let dep_path = self.final_dep_path_of(&dep.node_id, &final_dep_paths);
            let dep_path = importer_relative_link_dep_path(
                &dep_path,
                &anchor,
                self.opts.lockfile_dir.as_deref(),
                self.opts.project_dir.as_deref(),
            );
            direct_by_alias.insert(dep.alias.clone(), dep_path);
        }
        let graph = self.build_final_graph(&final_dep_paths);
        let paths_by_node_id = self.final_paths_by_node_id(&final_dep_paths);
        let resolved_peer_providers_by_alias = self.resolved_peer_providers_by_alias;
        let mut missing_names_by_pkg: HashMap<String, HashSet<String>> = HashMap::default();
        for (node_id, missing) in &self.node_missing_peers_of_children {
            let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else { continue };
            missing_names_by_pkg
                .entry(std::sync::Arc::<str>::clone(&tree_node.resolved_package_id).to_string())
                .or_default()
                .extend(missing.keys().cloned());
        }
        ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct_by_alias,
            resolved_peer_providers_by_alias,
            peer_dependency_issues: self.issues,
            missing_names_by_pkg,
            paths_by_node_id,
        }
    }

    /// Build the seed [`ParentRefs`] from the importer's direct deps so
    /// a direct dep's peer requirements can be satisfied by a sibling
    /// direct dep.
    ///
    /// `link:` direct deps whose target lives outside
    /// [`ResolvePeersOptions::lockfile_dir`] are seeded with a node id
    /// rewritten to `link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`
    /// when [`ResolvePeersOptions::exclude_links_from_lockfile`] is on
    /// — keeping the peer-suffix segment stable across machines
    /// regardless of the absolute path of the external link.
    fn build_importer_parents(&self) -> ParentRefs {
        self.build_importer_parents_from(&self.tree.direct)
    }

    /// Whether a dependency installed under `alias` can provide a peer:
    /// its alias or its real package name (the two differ for npm-alias
    /// deps like `peer-c1@npm:@pnpm.e2e/peer-c@2.0.0`) is declared as a
    /// peer somewhere in the tree.
    pub(super) fn is_peer_relevant(&self, alias: &str, pkg: &ResolvedPackage) -> bool {
        if self.tree.all_peer_dep_names.contains(alias) {
            return true;
        }
        if self.tree.all_peer_dep_names.is_empty() {
            return false;
        }
        let (real_name, _) = pkg_name_version(&pkg.result);
        self.tree.all_peer_dep_names.contains(&real_name)
    }

    /// Same as [`Self::build_importer_parents`] but seeds from an
    /// externally-supplied direct-deps slice — used by the
    /// multi-importer
    /// [`resolve_peers_workspace`](fn@super::resolve_peers_workspace)
    /// where each importer's `direct` lives outside [`ResolvedTree`].
    pub(super) fn build_importer_parents_from(&self, direct_deps: &[DirectDep]) -> ParentRefs {
        let mut refs = ParentRefs::default();
        for direct in direct_deps {
            let Some(tree_node) = self.tree.dependencies_tree.get(&direct.node_id) else {
                continue;
            };
            let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id) else {
                continue;
            };
            if !self.is_peer_relevant(&direct.alias, pkg) {
                continue;
            }
            let parent_node_id = remap_link_node_id(&self.opts, &direct.alias, &pkg.result)
                .unwrap_or_else(|| direct.node_id.clone());
            insert_parent_ref(&mut refs, &direct.alias, parent_node_id, pkg, tree_node.depth);
        }
        refs
    }

    pub(super) fn resolve_node(
        &mut self,
        node_id: &NodeId,
        parent_parent_refs: &Arc<ParentRefs>,
        parent_dep_paths: &Arc<HashMap<String, ParentPkgInfo>>,
        parent_chain_names: &SharedChain<String>,
        parent_node_ids: &SharedChain<NodeId>,
        parent_pkg_ids_chain: &SharedChain<String>,
    ) -> NodeOutput {
        // `purePkgs` fast-path. When the subtree below this
        // `pkgIdWithPatchHash` resolved with zero external peers and
        // zero missing peers on a previous walk, AND this package
        // itself declares no `peerDependencies`, the `depPath` is the
        // bare `pkgIdWithPatchHash` regardless of parent context.
        // Skip recursion entirely.
        //
        let context_free_dep_path =
            self.tree.dependencies_tree.get(node_id).and_then(|tree_node| {
                if tree_node.depth == -1 {
                    return Some((
                        tree_node.depth,
                        DepPath::from(std::sync::Arc::<str>::clone(&tree_node.resolved_package_id)),
                    ));
                }
                let dep_path = self.pure_pkgs.get(&*tree_node.resolved_package_id)?;
                let own_peers_bind = !self.tree.packages[&tree_node.resolved_package_id]
                    .peer_dependencies
                    .is_empty();
                if own_peers_bind
                    || (!self.discovery
                        && self
                            .graph
                            .get(dep_path)
                            .is_none_or(|graph_node| graph_node.depth > tree_node.depth))
                {
                    return None;
                }
                Some((tree_node.depth, dep_path.clone()))
            });
        if let Some((tree_node_depth, dep_path)) = context_free_dep_path {
            self.remember_resolved_node(node_id, &dep_path);
            if let Some(node) = self.graph.get_mut(&dep_path)
                && node.depth > tree_node_depth
            {
                node.depth = tree_node_depth;
            }
            return NodeOutput {
                dep_path,
                external_resolved_peers: Arc::clone(&self.empty_resolved_peers),
                auto_install_resolved_peers: HashMap::default(),
                missing_peers: Arc::clone(&self.empty_missing_peers),
                subtree_missing_by_pkg: None,
            };
        }

        if self.in_progress.contains(node_id) {
            // Cycle: bottom out with the bare `pkgIdWithPatchHash` as
            // the depPath. The original visit (still on the stack) will
            // compute the real depPath and insert it into
            // `node_dep_paths`. Returning the bare id here ensures the
            // current ancestor's peer-suffix construction can use a
            // `name@version` PeerId — see [`build_peer_id`] for the
            // cycle handling.
            let tree_node = &self.tree.dependencies_tree[node_id];
            let pkg = &self.tree.packages[&tree_node.resolved_package_id];
            return NodeOutput {
                dep_path: DepPath::from(std::sync::Arc::<str>::clone(&pkg.id)),
                external_resolved_peers: Arc::clone(&self.empty_resolved_peers),
                auto_install_resolved_peers: HashMap::default(),
                missing_peers: Arc::clone(&self.empty_missing_peers),
                subtree_missing_by_pkg: None,
            };
        }
        self.in_progress.insert(node_id.clone());

        let fast_cached = {
            let tree_node = &self.tree.dependencies_tree[node_id];
            tree_node
                .has_no_locked_peer_context()
                .then(|| {
                    self.find_fast_hit(node_id, parent_parent_refs, &tree_node.resolved_package_id)
                })
                .flatten()
                .map(PeersCacheItem::to_cached_node_output)
        };
        if let Some(cached) = fast_cached {
            let tree_node_depth = self.tree.dependencies_tree[node_id].depth;
            return self.finish_cache_hit(
                cached,
                CacheHitContext {
                    node_id,
                    tree_node_depth,
                    parent_chain_names,
                    parent_pkg_ids_chain,
                    preview_undo: None,
                },
            );
        }
        let (pkg_id, tree_node_depth, tree_node_installable) = {
            let tree_node = &self.tree.dependencies_tree[node_id];
            (
                std::sync::Arc::<str>::clone(&tree_node.resolved_package_id),
                tree_node.depth,
                tree_node.installable,
            )
        };
        let pkg = self.owned_package(&pkg_id);
        let (provider_children, preview_undo) = self.preview_peer_provider_children(node_id);
        let (pkg_name, _pkg_version) = pkg_name_version(&pkg.result);
        let ChildParentRefs {
            refs: child_parent_refs,
            own: new_parent_refs,
            changed: refs_changed,
        } = self.build_child_parent_refs(
            node_id,
            &pkg,
            parent_parent_refs,
            &provider_children,
            parent_node_ids,
        );

        // Record this node's parent context for the descendants'
        // [`peers_cache`] lookups. We compute and store the snapshot
        // before recursing so a cycle re-entry on a child also has
        // access to its caller's parent context. Unchanged refs reuse the
        // caller's snapshot instead of rebuilding an identical map.
        let parent_dep_paths = if refs_changed {
            self.parent_dep_paths_from_refs(&child_parent_refs)
        } else {
            Arc::clone(parent_dep_paths)
        };
        for child_node_id in
            new_parent_refs.values().filter_map(|parent_ref| parent_ref.node_id.as_ref())
        {
            self.parent_pkgs_of_node.insert(child_node_id.clone(), Arc::clone(&parent_dep_paths));
        }

        // `peersCache` lookup. When an earlier walk of this same
        // `pkgIdWithPatchHash` produced a result whose resolved-peer
        // map and missing-peer set are compatible with the current
        // parent peer context, reuse the cached `depPath` and external
        // peer/missing maps without recursing.
        //
        // The cache lookup uses `child_parent_refs` (the augmented
        // view) because a node's own children count as parents for
        // its own descendants' peer resolution.
        let cached =
            self.find_hit(&child_parent_refs, &pkg.id).map(PeersCacheItem::to_cached_node_output);
        if let Some(cached) = cached {
            return self.finish_cache_hit(
                cached,
                CacheHitContext {
                    node_id,
                    tree_node_depth,
                    parent_chain_names,
                    parent_pkg_ids_chain,
                    preview_undo,
                },
            );
        }
        let discovery_children = if self.discovery {
            let node = &self.tree.dependencies_tree[node_id];
            if let TreeChildren::Lazy { parent_ids } = &node.children {
                Some((
                    self.tree.children_by_id.get(&*pkg.id).cloned().unwrap_or_default(),
                    parent_ids.pushed(std::sync::Arc::<str>::clone(&pkg.id).to_string()),
                ))
            } else {
                None
            }
        } else {
            None
        };
        let (children_map, realize_undo) = if discovery_children.is_some() {
            (Arc::new(BTreeMap::new()), None)
        } else {
            self.realize_children_with(node_id, Some(&provider_children))
        };
        let realize_undo = merge_realize_undo(preview_undo, realize_undo);
        let current_parent_node_ids = parent_node_ids.pushed(node_id.clone());
        let child_parent_pkg_ids_chain = if parent_pkg_ids_chain.contains_str(&pkg.id) {
            parent_pkg_ids_chain.clone()
        } else {
            parent_pkg_ids_chain.pushed(std::sync::Arc::<str>::clone(&pkg.id).to_string())
        };
        let child_chain_names = parent_chain_names.pushed(pkg_name.clone());

        // Recurse into children first (post-order). Discovery walks lazy
        // children directly so cache hits never need occurrence-tree nodes.
        let mut child_outputs = ChildOutputs::default();
        if let Some((children, parent_ids)) = &discovery_children {
            let canonical_scc = self.canonical_scc();
            let child_aliases = ChildAliases::Deferred(children);
            for repeated in [true, false] {
                for edge in children.iter() {
                    if child_parent_refs.contains_key(&edge.alias) != repeated
                        || Self::cuts_cycle_edge(&canonical_scc, &pkg.id, &edge.pkg_id)
                    {
                        continue;
                    }
                    let child_node_id =
                        provider_children.get(&edge.alias).cloned().unwrap_or_else(|| {
                            if self.tree.packages.get(&edge.pkg_id).is_some_and(|pkg| pkg.is_leaf) {
                                NodeId::leaf(&edge.pkg_id)
                            } else {
                                NodeId::next()
                            }
                        });
                    let child_output = if self.tree.dependencies_tree.contains_key(&child_node_id) {
                        self.resolve_node(
                            &child_node_id,
                            &child_parent_refs,
                            &parent_dep_paths,
                            &child_chain_names,
                            &current_parent_node_ids,
                            &child_parent_pkg_ids_chain,
                        )
                    } else {
                        self.resolve_deferred_child(DeferredChildContext {
                            edge,
                            node_id: child_node_id,
                            parent_ids,
                            parent_refs: &child_parent_refs,
                            parent_dep_paths: &parent_dep_paths,
                            chain_names: &child_chain_names,
                            parent_node_ids: &current_parent_node_ids,
                            parent_pkg_ids: &child_parent_pkg_ids_chain,
                            depth: tree_node_depth + 1,
                        })
                    };
                    child_outputs.push(&edge.alias, child_output, &child_aliases, false);
                }
            }
        } else {
            let canonical_scc = self.canonical_scc();
            let child_aliases = ChildAliases::Realized(&children_map);
            for repeated in [true, false] {
                for (alias, child_node_id) in children_map.iter() {
                    if child_parent_refs.contains_key(alias) != repeated {
                        continue;
                    }
                    // A canonical back-edge child is record-only: it is
                    // in the realized map for the snapshot edge, but the
                    // position walk contributes nothing through it. The
                    // drain still walks it once at importer context —
                    // eagerly realized trees reach their back-edge
                    // subtrees only through this queue.
                    // A canonical back-edge child is record-only: the
                    // position walk contributes nothing through it, and
                    // the record pass remaps it to the target's shared
                    // canonical occurrence.
                    if self.tree.dependencies_tree.get(child_node_id).is_some_and(|child| {
                        Self::cuts_cycle_edge(&canonical_scc, &pkg.id, &child.resolved_package_id)
                    }) {
                        continue;
                    }
                    let child_output = self.resolve_node(
                        child_node_id,
                        &child_parent_refs,
                        &parent_dep_paths,
                        &child_chain_names,
                        &current_parent_node_ids,
                        &child_parent_pkg_ids_chain,
                    );
                    child_outputs.push(alias, child_output, &child_aliases, !self.discovery);
                }
            }
        }
        let ChildOutputs {
            external_peers: external_from_children,
            mut auto_install_resolved_peers,
            missing_peers: missing_from_children,
            dep_paths: child_dep_paths,
            mut missing_summaries,
        } = child_outputs;

        let NodePeers {
            own_resolved: own_resolved_peers,
            all_resolved: all_resolved_peers,
            all_missing: all_missing_peers,
            dep_path,
        } = self.resolve_node_peers(NodePeersContext {
            pkg: &pkg,
            pkg_name: &pkg_name,
            parent_refs: &child_parent_refs,
            chain_names: &child_chain_names,
            ancestor_pkg_ids: parent_pkg_ids_chain,
            external_from_children,
            missing_from_children: &missing_from_children,
        });
        auto_install_resolved_peers.extend(
            own_resolved_peers
                .iter()
                .map(|(peer_name, peer_node_id)| (peer_name.clone(), peer_node_id.clone())),
        );

        // Register the depPath ↔ NodeId mapping and per-node
        // propagated state before inserting into the graph (so any
        // cycle the graph insert hits via `child_dep_paths` can find
        // this node's depPath).
        self.remember_resolved_node(node_id, &dep_path);

        let own_missing = (!missing_from_children.is_empty())
            .then(|| (pkg.id.to_string(), missing_from_children.keys().cloned().collect()));
        let subtree_missing_by_pkg = match (own_missing, missing_summaries.len()) {
            (None, 0) => None,
            (None, 1) => missing_summaries.pop(),
            (own, _) => Some(Arc::new(MissingSummary { own, children: missing_summaries })),
        };

        let is_pure = all_resolved_peers.is_empty() && all_missing_peers.is_empty();
        let all_resolved_peers = Arc::new(all_resolved_peers);
        let all_missing_peers = Arc::new(all_missing_peers);
        let missing_from_children = Arc::new(missing_from_children);

        if !self.discovery {
            self.node_external_peers.insert(node_id.clone(), Arc::clone(&all_resolved_peers));
            self.node_missing_peers.insert(node_id.clone(), Arc::clone(&all_missing_peers));
            self.node_missing_peers_of_children
                .insert(node_id.clone(), Arc::clone(&missing_from_children));
        }

        // Record this walk's outcome in the per-`pkgIdWithPatchHash`
        // caches. Pure subtrees go in [`Self::pure_pkgs`] for the
        // fast-path early return at the top of [`resolve_node`];
        // non-pure subtrees push a [`PeersCacheItem`] so a future
        // visit with a compatible parent context can short-circuit
        // via [`Self::find_hit`]. The canonical cycle gate makes every
        // occurrence of a package see the same subtree, so a verdict is
        // a plain function of the parent context.
        if is_pure {
            self.pure_pkgs
                .insert(std::sync::Arc::<str>::clone(&pkg.id).to_string(), dep_path.clone());
        } else {
            self.retained_peer_node_ids.extend(all_resolved_peers.values().cloned());
            self.peers_cache
                .entry(std::sync::Arc::<str>::clone(&pkg.id).to_string())
                .or_default()
                .push(PeersCacheItem {
                    owner_node_id: node_id.clone(),
                    dep_path: dep_path.clone(),
                    resolved_peers: Arc::clone(&all_resolved_peers),
                    missing_peers: Arc::clone(&all_missing_peers),
                    missing_peers_of_children: Arc::clone(&missing_from_children),
                    subtree_missing_by_pkg: subtree_missing_by_pkg.clone(),
                });
        }

        if !self.discovery {
            self.record_walked_node(WalkedNode {
                node_id,
                pkg: &pkg,
                dep_path: &dep_path,
                parent_node_ids,
                parent_pkg_ids_chain,
                children: &children_map,
                child_dep_paths,
                all_resolved_peers: &all_resolved_peers,
                all_missing_peers: &all_missing_peers,
                own_resolved_peers: &own_resolved_peers,
                depth: tree_node_depth,
                installable: tree_node_installable,
                is_pure,
            });
        }

        self.in_progress.remove(node_id);

        let external_to_report: HashMap<String, NodeId> = all_resolved_peers
            .iter()
            .filter(|(peer_alias, _)| {
                !children_map.contains_key(peer_alias.as_str())
                    && discovery_children.as_ref().is_none_or(|(children, _)| {
                        !children.iter().any(|edge| edge.alias == **peer_alias)
                    })
            })
            .map(|(peer_alias, peer_node_id)| (peer_alias.clone(), peer_node_id.clone()))
            .collect();

        let output = NodeOutput {
            dep_path,
            external_resolved_peers: Arc::new(external_to_report),
            auto_install_resolved_peers,
            missing_peers: all_missing_peers,
            subtree_missing_by_pkg,
        };
        if self.discovery {
            self.undo_realize(node_id, realize_undo, Some(&output));
        }
        output
    }

    /// Build the [`ParentRefs`] map that descendants of this node see:
    /// the parent's view, plus the node's own peer-relevant children,
    /// plus the pins the wanted lockfile locked in. Kept behind `Arc`
    /// copy-on-write: most nodes contribute nothing, so they pass the
    /// parent's map down by refcount instead of cloning it — the
    /// per-node map clones dominated the walker's CPU time on
    /// peer-heavy workspaces.
    fn build_child_parent_refs(
        &self,
        node_id: &NodeId,
        pkg: &ResolvedPackage,
        parent_parent_refs: &Arc<ParentRefs>,
        provider_children: &BTreeMap<String, NodeId>,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> ChildParentRefs {
        let mut refs_changed = false;
        let mut child_parent_refs = Arc::clone(parent_parent_refs);

        let mut new_parent_refs = ParentRefs::default();
        for (alias, child_node_id) in provider_children {
            let Some(child_tree) = self.tree.dependencies_tree.get(child_node_id) else { continue };
            let Some(child_pkg) = self.tree.packages.get(&child_tree.resolved_package_id) else {
                continue;
            };
            insert_parent_ref(
                &mut new_parent_refs,
                alias,
                child_node_id.clone(),
                child_pkg,
                child_tree.depth,
            );
        }
        if !new_parent_refs.is_empty() {
            refs_changed = true;
            let refs = Arc::make_mut(&mut child_parent_refs);
            // Built only when a name collision actually consults it — the
            // common no-collision node never pays for the extra map clone.
            let mut refs_with_new: Option<ParentRefs> = None;
            for (name, mut new_parent_ref) in new_parent_refs.clone() {
                if let Some(existing) = refs.get(&name) {
                    let with_new = refs_with_new.get_or_insert_with(|| {
                        let mut with_new = refs.clone();
                        with_new.extend(new_parent_refs.clone());
                        with_new
                    });
                    if !self.parent_refs_match(existing, &new_parent_ref)
                        || self.inherited_parent_pkg_breaks_peer_diamond(
                            with_new,
                            existing,
                            &new_parent_ref,
                            node_id,
                        )
                    {
                        new_parent_ref.occurrence = existing.occurrence + 1;
                        refs.insert(name, new_parent_ref);
                    }
                } else {
                    refs.insert(name, new_parent_ref);
                }
            }
        }

        let locked_pins =
            self.locked_peer_context_pins(node_id, pkg, &child_parent_refs, parent_node_ids);
        if !locked_pins.is_empty() {
            refs_changed = true;
            let refs = Arc::make_mut(&mut child_parent_refs);
            for (name, parent_ref) in locked_pins {
                refs.insert(name, parent_ref);
            }
        }

        ChildParentRefs { refs: child_parent_refs, own: new_parent_refs, changed: refs_changed }
    }

    /// Resolve this node's own peer requirements against the augmented
    /// [`ParentRefs`] visible at it, fold the result with what its
    /// children reported, and render the depPath. Empty resolved-peers
    /// ⇒ pure node: depPath = `pkgIdWithPatchHash`.
    fn resolve_node_peers(&mut self, context: NodePeersContext<'_>) -> NodePeers {
        let NodePeersContext {
            pkg,
            pkg_name,
            parent_refs,
            chain_names,
            ancestor_pkg_ids,
            external_from_children,
            missing_from_children,
        } = context;

        let mut own_resolved: HashMap<String, NodeId> = HashMap::default();
        let mut own_missing: HashMap<String, MissingPeerInfo> = HashMap::default();
        for (peer_name, peer_dep) in &pkg.peer_dependencies {
            self.resolve_one_peer(
                peer_name,
                peer_dep,
                parent_refs,
                chain_names,
                ancestor_pkg_ids,
                &mut own_resolved,
                &mut own_missing,
            );
        }

        // A package doesn't peer-depend on itself, so its own name never
        // enters its suffix.
        let mut all_resolved = external_from_children;
        for (peer_alias, peer_node_id) in &own_resolved {
            all_resolved.insert(peer_alias.clone(), peer_node_id.clone());
        }
        all_resolved.remove(pkg_name);

        let mut all_missing = missing_from_children.clone();
        for (peer_alias, info) in &own_missing {
            all_missing.insert(peer_alias.clone(), info.clone());
        }

        let dep_path = if all_resolved.is_empty() {
            DepPath::from(std::sync::Arc::<str>::clone(&pkg.id))
        } else {
            let peer_ids: Vec<PeerId> = all_resolved
                .iter()
                .map(|(peer_alias, peer_node_id)| self.build_peer_id(peer_alias, peer_node_id))
                .collect();
            let suffix = create_peer_dep_graph_hash(&peer_ids, self.opts.peers_suffix_max_length);
            DepPath::from(format!("{}{}", pkg.id, suffix))
        };

        NodePeers { own_resolved, all_resolved, all_missing, dep_path }
    }

    /// The upstream locked-peer-provider reuse block
    /// (`resolvePeers.ts:594`): for each `peer name → provider DepPath`
    /// the wanted lockfile recorded on this node, re-pin the provider
    /// into `parent_refs` when it is still reachable in the current
    /// tree, resolved to the same path in the previous pass, carries no
    /// peer suffix of its own, has not diverged in this pass, is not
    /// overridden by a current provider that must win, and satisfies
    /// the node's current peer range.
    /// The pins [`apply_locked_peer_context` upstream] would insert into
    /// `parent_refs`, computed without mutating it so the caller can keep
    /// sharing an unchanged map. Each pin reads only its own name's
    /// current binding, so collecting against the pre-pin map is
    /// equivalent to inserting while iterating.
    fn locked_peer_context_pins(
        &self,
        node_id: &NodeId,
        pkg: &ResolvedPackage,
        parent_refs: &ParentRefs,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> Vec<(String, ParentRef)> {
        let mut pins = Vec::new();
        let (Some(locked_peer_context), Some(provider_paths)) = (
            self.tree
                .dependencies_tree
                .get(node_id)
                .and_then(crate::resolved_tree::DependenciesTreeNode::locked_peer_context),
            self.opts.resolved_peer_provider_paths.as_ref(),
        ) else {
            return pins;
        };
        for (peer_name, previous_dep_path) in locked_peer_context {
            let Some(peer_node_id) = self.node_ids_by_previous_dep_path.get(previous_dep_path)
            else {
                continue;
            };
            let Some(peer_dep) = pkg.peer_dependencies.get(peer_name) else { continue };
            if provider_paths.get(peer_node_id) != Some(previous_dep_path) {
                continue;
            }
            // Only pin providers that have no peer context of their
            // own — a suffixed path depends on the very bindings this
            // pass is still computing.
            if index_of_dep_path_suffix(previous_dep_path.as_str()).peers_index.is_some() {
                continue;
            }
            // A provider that already resolved to a different path
            // this pass must not be rebound.
            if self
                .node_dep_paths
                .get(peer_node_id)
                .is_some_and(|current| current != previous_dep_path)
            {
                continue;
            }
            if self.has_current_peer_provider_that_must_win(peer_name, parent_refs, parent_node_ids)
            {
                continue;
            }
            let Some(peer_tree_node) = self.tree.dependencies_tree.get(peer_node_id) else {
                continue;
            };
            let Some(peer_pkg) = self.tree.packages.get(&peer_tree_node.resolved_package_id) else {
                continue;
            };
            let (_, peer_version) = pkg_name_version(&peer_pkg.result);
            if !satisfies_with_prereleases(
                &peer_version,
                &get_peer_version_range(&peer_dep.version),
            ) {
                continue;
            }
            // Upstream builds the pinned ref through `toPkgByName`,
            // which always starts at occurrence 0; the shadow counter
            // only tracks child-level replacements.
            pins.push((
                peer_name.clone(),
                ParentRef {
                    version: peer_version,
                    node_id: Some(peer_node_id.clone()),
                    alias: Some(peer_name.clone()),
                    depth: peer_tree_node.depth,
                    occurrence: 0,
                },
            ));
        }
        pins
    }

    /// The upstream `hasCurrentPeerProviderThatMustWin`: the current
    /// provider bound for `peer_name` wins over a locked one when it is
    /// an importer direct dep under a *different* alias, one the user
    /// explicitly requested, one the manifest declares with no
    /// wanted-lockfile resolution, or a child an ancestor re-resolved
    /// away from the lockfile.
    fn has_current_peer_provider_that_must_win(
        &self,
        peer_name: &str,
        parent_refs: &ParentRefs,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> bool {
        let Some(peer_node_id) =
            parent_refs.get(peer_name).and_then(|parent| parent.node_id.as_ref())
        else {
            return false;
        };
        for source in &self.current_provider_sources {
            for (alias, direct_node_id) in &source.direct_node_ids_by_alias {
                if direct_node_id == peer_node_id
                    && (alias != peer_name
                        || source.explicitly_requested_direct_dependencies.contains(alias)
                        || (source.declared_direct_dependencies.contains(alias)
                            && self
                                .tree
                                .dependencies_tree
                                .get(peer_node_id)
                                .is_none_or(|node| node.previous_dep_path().is_none())))
                {
                    return true;
                }
            }
        }
        for parent_node_id in parent_node_ids.iter() {
            let Some(parent_node) = self.tree.dependencies_tree.get(parent_node_id) else {
                continue;
            };
            let Some(must_win) = parent_node.must_win_dependency_names() else {
                continue;
            };
            // Ancestors on the walk path always have realized children.
            let TreeChildren::Realized(children) = &parent_node.children else { continue };
            if must_win.iter().any(|alias| children.get(alias) == Some(peer_node_id)) {
                return true;
            }
        }
        false
    }

    /// `true` when a missing-peer issue for `peer_name` under the
    /// given ancestor chain must not be emitted for the hoist input.
    /// See [`ResolvePeersOptions::hoist_missing_scope`].
    pub(super) fn missing_issue_suppressed(
        &self,
        ancestor_pkg_ids: &SharedChain<String>,
        peer_name: &str,
    ) -> bool {
        let Some(scope) = self.opts.hoist_missing_scope.as_ref() else { return false };
        scope.suppresses_iter(ancestor_pkg_ids.iter(), peer_name)
    }

    pub(super) fn record_missing_issue(
        &mut self,
        peer_name: &str,
        issue: MissingPeer,
        ancestor_pkg_ids: &SharedChain<String>,
    ) {
        if self.in_canonical_drain {
            return;
        }
        self.issues.missing.entry(peer_name.to_string()).or_default().push(issue);
        if self.discovery {
            self.missing_ancestor_pkg_ids
                .entry(peer_name.to_string())
                .or_default()
                .push(ancestor_pkg_ids.clone());
        }
    }

    pub(super) fn issue_parents(&self, chain: &SharedChain<String>) -> ParentChain {
        if self.discovery { ParentChain::default() } else { ParentChain(chain.clone()) }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal walker helper threading per-node context, mirrors the resolve_node parameter set"
    )]
    fn resolve_one_peer(
        &mut self,
        peer_name: &str,
        peer_dep: &PeerDep,
        parent_refs: &ParentRefs,
        chain: &SharedChain<String>,
        ancestor_pkg_ids: &SharedChain<String>,
        resolved: &mut HashMap<String, NodeId>,
        missing: &mut HashMap<String, MissingPeerInfo>,
    ) {
        let raw_range = peer_dep.version.as_str();
        // The stored range keeps the original scheme (only `workspace:` is
        // stripped) so it still selects the package to auto-install for a
        // missing peer, e.g. `work:5.x.x` fetches from the `work` registry.
        let range_for_match = raw_range.strip_prefix("workspace:").unwrap_or(raw_range);
        // The satisfaction check needs a comparable semver range, so
        // named-registry/`npm:` bodies are extracted and opaque specs become `*`.
        let comparable_range = self.comparable_peer_range(raw_range);
        let optional = peer_dep.optional;

        match parent_refs.get(peer_name) {
            None => {
                missing.insert(
                    peer_name.to_string(),
                    MissingPeerInfo { range: range_for_match.to_string(), optional },
                );
                if !self.missing_issue_suppressed(ancestor_pkg_ids, peer_name) {
                    self.record_missing_issue(
                        peer_name,
                        MissingPeer {
                            wanted_range: comparable_range.text.clone(),
                            raw_range: range_for_match.to_string(),
                            optional,
                            parents: self.issue_parents(chain),
                        },
                        ancestor_pkg_ids,
                    );
                }
            }
            Some(parent) => {
                if !comparable_range.satisfies(&parent.version) && !self.in_canonical_drain {
                    let parents = self.issue_parents(chain);
                    self.issues.bad.entry(peer_name.to_string()).or_default().push(
                        PeerDependencyIssue {
                            wanted_range: comparable_range.text.clone(),
                            found_version: parent.version.clone(),
                            optional,
                            parents,
                            resolved_from: ParentChain::default(),
                        },
                    );
                }
                if let Some(parent_node_id) = parent.node_id.as_ref() {
                    resolved.insert(peer_name.to_string(), parent_node_id.clone());
                }
            }
        }
    }

    /// Build the [`PeerId`] contribution for one resolved peer.
    ///
    /// Precedence:
    ///
    /// 1. **`link:<rel>` `NodeIds`** — emit
    ///    `PeerId::Pair { name: peer_alias, version: link_path_to_peer_version(rel) }`
    ///    so the peer-suffix segment reads as `name@encoded_path`
    ///    instead of carrying the raw link target. This branch fires
    ///    for both workspace-link parents and the
    ///    `excludeLinksFromLockfile` remap that points the parent at
    ///    `link:node_modules/<alias>`.
    /// 2. **`dedupe_peers` enabled** — emit `name@version` from the
    ///    resolved package so recursive peer suffixes collapse like
    ///    `(foo@1.0.0(bar@2.0.0))` → `(foo@1.0.0)`.
    /// 3. **The peer's `DepPath`** once it has been walked —
    ///    `node_dep_paths` lookup, emitted as [`PeerId::DepPath`].
    /// 4. **Cycle fallback** — `name@version` from the resolved package,
    ///    emitted as [`PeerId::Pair`].
    fn build_peer_id(&self, peer_alias: &str, peer_node_id: &NodeId) -> PeerId {
        if let NodeId::Leaf(id) = peer_node_id
            && let Some(rel) = id.strip_prefix("link:")
        {
            return PeerId::Pair {
                name: peer_alias.to_string(),
                version: link_path_to_peer_version(rel),
            };
        }
        if self.opts.dedupe_peers
            && let Some(tree_node) = self.tree.dependencies_tree.get(peer_node_id)
            && let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id)
        {
            return peer_id_pair(&pkg.result);
        }
        if let Some(dep_path) = self.node_dep_paths.get(peer_node_id) {
            return PeerId::DepPath(dep_path.clone());
        }
        let tree_node = &self.tree.dependencies_tree[peer_node_id];
        let pkg = &self.tree.packages[&tree_node.resolved_package_id];
        peer_id_pair(&pkg.result)
    }

    /// Resolve `node_id` to the depPath computed during the main walk.
    /// Peerless nodes are already final at this point; nodes with peers
    /// may still be missing a pending peer provider's own final suffix.
    pub(super) fn provisional_dep_path_of(&self, node_id: &NodeId) -> DepPath {
        if let Some(dep_path) = self.node_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        if let Some(dep_path) = link_node_id_as_dep_path(node_id) {
            return dep_path;
        }
        let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
        DepPath::from(std::sync::Arc::<str>::clone(&self.tree.packages[pkg_id].id))
    }

    pub(super) fn remember_parent_context_if_peer_provider(
        &mut self,
        alias: &str,
        node_id: &NodeId,
        parent_context: &Arc<HashMap<String, ParentPkgInfo>>,
    ) {
        let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else { return };
        let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id) else { return };
        if self.is_peer_relevant(alias, pkg) {
            self.parent_pkgs_of_node.insert(node_id.clone(), Arc::clone(parent_context));
        }
    }

    fn owned_package(&mut self, pkg_id: &str) -> Arc<ResolvedPackage> {
        if let Some(pkg) = self.packages_by_id.get(pkg_id) {
            return Arc::clone(pkg);
        }
        let pkg = Arc::new(self.tree.packages[pkg_id].clone());
        self.packages_by_id.insert(pkg_id.to_string(), Arc::clone(&pkg));
        pkg
    }

    pub(super) fn remember_resolved_node(&mut self, node_id: &NodeId, dep_path: &DepPath) {
        let retain = !self.discovery
            || self.parent_pkgs_of_node.contains_key(node_id)
            || self.opts.hoisted_peer_provider_node_ids.contains(node_id);
        if !retain {
            return;
        }
        self.node_dep_paths.insert(node_id.clone(), dep_path.clone());
        self.visited_this_call.insert(node_id.clone());
    }
}

/// The missing-peer names reported for one package by a walk. A
/// package's occurrences can appear in several subtree summaries, whose
/// reports are read as their union.
pub(crate) enum MissingNames<'a> {
    One(&'a HashSet<String>),
    Union(Vec<&'a HashSet<String>>),
}

impl<'a> MissingNames<'a> {
    fn add(&mut self, names: &'a HashSet<String>) {
        match self {
            MissingNames::One(first) => *self = MissingNames::Union(vec![first, names]),
            MissingNames::Union(all) => all.push(names),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        let (one, union) = match self {
            MissingNames::One(names) => (Some(*names), None),
            MissingNames::Union(all) => (None, Some(all)),
        };
        one.into_iter()
            .chain(union.into_iter().flatten().copied())
            .flat_map(|names| names.iter().map(String::as_str))
    }
}

/// Strongly-connected-component ids over the recorded children graph.
/// Iterative Tarjan, mirroring the peer-graph variant in the finalize
/// pass, so deep graphs cannot overflow the call stack.
fn children_scc_ids(tree: &ResolvedTree) -> HashMap<Arc<str>, usize> {
    let mut node_ids: Vec<Arc<str>> = Vec::new();
    let mut index_by_id: HashMap<Arc<str>, usize> = HashMap::default();
    let mut intern = |id: &Arc<str>, node_ids: &mut Vec<Arc<str>>| -> usize {
        if let Some(index) = index_by_id.get(id) {
            return *index;
        }
        let index = node_ids.len();
        node_ids.push(Arc::clone(id));
        index_by_id.insert(Arc::clone(id), index);
        index
    };
    let mut adjacency: Vec<Vec<usize>> = Vec::new();
    for (pkg_id, edges) in &tree.children_by_id {
        let node = intern(pkg_id, &mut node_ids);
        if adjacency.len() <= node {
            adjacency.resize_with(node + 1, Vec::new);
        }
        let targets: Vec<usize> =
            edges.iter().map(|edge| intern(&edge.pkg_id, &mut node_ids)).collect();
        adjacency[node] = targets;
    }
    adjacency.resize_with(node_ids.len(), Vec::new);

    let node_count = node_ids.len();
    let mut discovery = vec![u32::MAX; node_count];
    let mut lowlink = vec![0u32; node_count];
    let mut on_stack = vec![false; node_count];
    let mut tarjan_stack: Vec<usize> = Vec::new();
    let mut next_index = 0u32;
    let mut scc_of_index = vec![usize::MAX; node_count];
    let mut next_scc = 0usize;

    for root in 0..node_count {
        if discovery[root] != u32::MAX {
            continue;
        }
        // Explicit DFS stack of (node, edge cursor) so deep dependency
        // graphs don't overflow the call stack.
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        'dfs: while let Some(&mut (node, ref mut cursor)) = work.last_mut() {
            if *cursor == 0 {
                discovery[node] = next_index;
                lowlink[node] = next_index;
                next_index += 1;
                on_stack[node] = true;
                tarjan_stack.push(node);
            }
            while *cursor < adjacency[node].len() {
                let child = adjacency[node][*cursor];
                *cursor += 1;
                if discovery[child] == u32::MAX {
                    work.push((child, 0));
                    continue 'dfs;
                }
                if on_stack[child] {
                    lowlink[node] = lowlink[node].min(discovery[child]);
                }
            }
            if lowlink[node] == discovery[node] {
                loop {
                    let member = tarjan_stack.pop().expect("Tarjan stack holds the open SCC");
                    on_stack[member] = false;
                    scc_of_index[member] = next_scc;
                    if member == node {
                        break;
                    }
                }
                next_scc += 1;
            }
            work.pop();
            if let Some(&mut (parent, _)) = work.last_mut() {
                lowlink[parent] = lowlink[parent].min(lowlink[node]);
            }
        }
    }

    node_ids.into_iter().zip(scc_of_index).filter(|(_, scc)| *scc != usize::MAX).collect()
}

/// Index the per-package missing-peer names `roots` reported, borrowing/// Index the per-package missing-peer names `roots` reported, borrowing
/// from the summaries rather than copying every descendant's names into
/// an owned map: one of these is built per importer per hoist round, so
/// a copy would make each round cost the whole workspace.
pub(crate) fn index_missing_names(
    roots: &[Arc<MissingSummary>],
) -> HashMap<&str, MissingNames<'_>> {
    let mut index: HashMap<&str, MissingNames<'_>> = HashMap::default();
    let mut seen: HashSet<usize> = HashSet::default();
    let mut pending: Vec<&Arc<MissingSummary>> = roots.iter().collect();
    while let Some(summary) = pending.pop() {
        if !seen.insert(Arc::as_ptr(summary) as usize) {
            continue;
        }
        if let Some((pkg_id, names)) = &summary.own {
            index
                .entry(pkg_id.as_str())
                .and_modify(|entry| entry.add(names))
                .or_insert(MissingNames::One(names));
        }
        pending.extend(summary.children.iter());
    }
    index
}

#[cfg(test)]
mod tests;
