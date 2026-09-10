//! The peer-resolution walk itself: [`Walker`], its state, the
//! per-importer entry [`Walker::walk`], the recursive
//! [`Walker::resolve_node`], and the peer matching each visited node
//! performs against its parent context.

pub(crate) use walk_context::MissingSummary;

pub(super) use walk_context::{
    MissingPeerInfo, NodeOutput, NodeWalkContext, RootWalk, SubtreeMissingByPkg,
};

pub(crate) use missing_names::{MissingNames, index_missing_names};

mod walk_context;
use walk_context::{
    ChildAliases, ChildChains, ChildOutputs, ChildParentRefs, ChildrenWalk, DeferredChildren,
    LockedPinContext, NodeEntry, NodePeers, NodePeersContext, SettledPeers, WalkResult,
    index_peer_provider_children,
};

mod missing_names;
use missing_names::{children_scc_ids, external_peers_to_report};

mod peer_issues;

mod peer_providers;

mod child_walk;

mod node_walk;

use crate::{
    dependencies_graph::{
        DependenciesGraph, MissingPeer, ParentChain, PeerDependencyIssue, PeerDependencyIssues,
    },
    node_id::NodeId,
    resolve_peers::{
        ResolvePeersOptions, ResolvePeersResult,
        cache::{
            CacheHitContext, DeferredChildContext, PeerProviderChildren, PeersCacheItem,
            UndoRealize, merge_realize_undo,
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
        let caches = prepare_discovery_caches(tree, caches);
        Walker {
            tree,
            opts,
            graph: DependenciesGraph::default(),
            issues: PeerDependencyIssues::default(),
            missing_ancestor_pkg_ids: HashMap::default(),
            node_dep_paths: caches.node_dep_paths,
            node_external_peers: HashMap::default(),
            cache_owner_by_node_id: HashMap::default(),
            node_missing_peers: HashMap::default(),
            node_missing_peers_of_children: HashMap::default(),
            resolved_peer_providers_by_alias: BTreeMap::new(),
            in_progress: HashSet::default(),
            pending_peer_edges: Vec::new(),
            pending_peer_edge_keys: HashSet::default(),
            pure_pkgs: caches.pure_pkgs,
            peers_cache: caches.peers_cache,
            parent_pkgs_of_node: caches.parent_pkgs_of_node,
            retained_peer_node_ids: caches.retained_peer_node_ids,
            node_records: HashMap::default(),
            next_record_order: 0,
            node_ids_by_previous_dep_path,
            current_provider_sources,
            discovery,
            visited_this_call: HashSet::default(),
            packages_by_id: HashMap::default(),
            peer_provider_children_by_pkg_id: caches.peer_provider_children_by_pkg_id,
            peer_provider_index_peer_names: caches.peer_provider_index_peer_names,
            children_sccs: std::cell::OnceCell::new(),
            empty_resolved_peers: Arc::new(HashMap::default()),
            empty_missing_peers: Arc::new(HashMap::default()),
            canonical_backedge_nodes: caches.canonical_backedge_nodes,
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
                &NodeWalkContext {
                    parent_refs: importer_parents,
                    parent_dep_paths: importer_parent_dep_paths,
                    chain_names: &SharedChain::default(),
                    parent_node_ids: &SharedChain::default(),
                    parent_pkg_ids: &SharedChain::default(),
                },
            );
        }
        self.in_canonical_drain = false;
    }
}

impl Walker<'_> {
    pub(super) fn walk(mut self) -> ResolvePeersResult {
        // Clone direct deps into an owned `Vec` so the recursion
        // below can mutate `self.tree` (realising lazy children)
        // without conflicting with this loop's borrow of
        // `self.tree.direct`.
        let direct: Vec<DirectDep> = self.tree.direct.clone();
        let root = RootWalk::of(&self, &direct);
        self.walk_direct(&direct, &root);
        self.patch_pending_peer_edges();
        // Recompute depPaths so each resolved peer carries its full
        // suffix (the cycle fallback during the walk collapses peers
        // that are walk-ancestors), then rebuild the graph from the
        // per-node records keyed by the corrected depPaths.
        let final_dep_paths = self.build_final_dep_paths();
        let direct_by_alias = self.importer_direct_dep_paths(&direct, &final_dep_paths);
        let graph = self.build_final_graph(&final_dep_paths);
        let paths_by_node_id = self.final_paths_by_node_id(&final_dep_paths);
        ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct_by_alias,
            missing_names_by_pkg: self.missing_names_by_pkg(),
            resolved_peer_providers_by_alias: self.resolved_peer_providers_by_alias,
            peer_dependency_issues: self.issues,
            paths_by_node_id,
        }
    }

    /// The importer's own direct deps first, then the pruned peer
    /// providers at root context, draining the canonical queue after
    /// each.
    fn walk_direct(&mut self, direct: &[DirectDep], root: &RootWalk) {
        let (own_direct, provider_direct): (Vec<&DirectDep>, Vec<&DirectDep>) = direct
            .iter()
            .partition(|dep| !self.opts.hoisted_peer_provider_node_ids.contains(&dep.node_id));
        for dep in &own_direct {
            self.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &root.parent_dep_paths,
            );
        }
        for dep in &own_direct {
            self.resolve_importer_dep(dep, &root.context());
        }
        self.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
        self.resolve_pruned_peer_providers(&provider_direct, &root.context());
        self.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
    }

    fn importer_direct_dep_paths(
        &self,
        direct: &[DirectDep],
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> BTreeMap<String, DepPath> {
        let anchor = match (self.opts.project_dir.as_deref(), self.opts.lockfile_dir.as_deref()) {
            (Some(project_dir), Some(lockfile_dir)) => {
                crate::link_target::ImporterAnchor::new(project_dir, lockfile_dir)
            }
            _ => crate::link_target::ImporterAnchor::default(),
        };
        direct
            .iter()
            .map(|dep| {
                let dep_path = importer_relative_link_dep_path(
                    &self.final_dep_path_of(&dep.node_id, final_dep_paths),
                    &anchor,
                    self.opts.lockfile_dir.as_deref(),
                    self.opts.project_dir.as_deref(),
                );
                (dep.alias.clone(), dep_path)
            })
            .collect()
    }

    pub(super) fn resolve_importer_dep(&mut self, dep: &DirectDep, walk: &NodeWalkContext<'_>) {
        let output = self.resolve_node(&dep.node_id, walk);
        for (peer_alias, peer_node_id) in output.auto_install_resolved_peers {
            self.resolved_peer_providers_by_alias.insert(peer_alias, peer_node_id);
        }
    }

    /// See [`ResolvePeersOptions::hoisted_peer_provider_node_ids`] — a
    /// provider is normally resolved at its tree position during the main
    /// walk; only one whose position was pruned still needs this root-context
    /// fallback.
    pub(super) fn resolve_pruned_peer_providers(
        &mut self,
        provider_direct: &[&DirectDep],
        walk: &NodeWalkContext<'_>,
    ) {
        for dep in provider_direct {
            if self.visited_this_call.contains(&dep.node_id) {
                continue;
            }
            self.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                walk.parent_dep_paths,
            );
            self.resolve_importer_dep(dep, walk);
        }
    }

    /// The missing peer names each package accumulated across every
    /// occurrence the walk visited.
    fn missing_names_by_pkg(&self) -> HashMap<String, HashSet<String>> {
        let mut missing_names_by_pkg: HashMap<String, HashSet<String>> = HashMap::default();
        for (node_id, missing) in &self.node_missing_peers_of_children {
            let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else { continue };
            missing_names_by_pkg
                .entry(tree_node.resolved_package_id.to_string())
                .or_default()
                .extend(missing.keys().cloned());
        }
        missing_names_by_pkg
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

    /// The parent refs an importer's direct deps seed the peer walk with.
    /// The multi-importer
    /// [`resolve_peers_workspace`](fn@super::resolve_peers_workspace)
    /// supplies each importer's `direct` from outside [`ResolvedTree`].
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
}

/// The ancestor package-id chain a node's children see. A package already on
/// the chain is not repeated, so a cycle cannot grow it without bound.
fn chain_with_pkg_id(chain: &SharedChain<String>, pkg_id: &Arc<str>) -> SharedChain<String> {
    if chain.contains_str(pkg_id) { chain.clone() } else { chain.pushed(pkg_id.to_string()) }
}

#[cfg(test)]
mod tests;

fn prepare_discovery_caches(
    tree: &ResolvedTree,
    mut caches: PeerDiscoveryCaches,
) -> PeerDiscoveryCaches {
    if caches.peer_provider_index_peer_names != tree.all_peer_dep_names {
        caches.peer_provider_children_by_pkg_id.clear();
        caches.peer_provider_index_peer_names.clone_from(&tree.all_peer_dep_names);
    }
    index_peer_provider_children(tree, &mut caches.peer_provider_children_by_pkg_id);
    caches
}

impl SettledPeers {
    fn node_output(self, walked: &mut ChildrenWalk) -> NodeOutput {
        NodeOutput {
            dep_path: self.dep_path,
            external_resolved_peers: Arc::new(external_peers_to_report(
                &self.all_resolved,
                &walked.children_map,
                walked.discovery_children.as_ref(),
            )),
            auto_install_resolved_peers: std::mem::take(
                &mut walked.outputs.auto_install_resolved_peers,
            ),
            missing_peers: self.all_missing,
            subtree_missing_by_pkg: self.subtree_missing_by_pkg,
        }
    }
}
