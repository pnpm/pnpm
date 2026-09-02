//! The walk's short-circuits: the `purePkgs` fast path, `peersCache`
//! matching against the current parent context, deferred realization of
//! lazy children, and the retention decisions that keep a materialized
//! node alive after its subtree is reused.

use crate::{
    dependencies_graph::MissingPeer,
    node_id::NodeId,
    resolve_peers::{
        context::{ParentPkgInfo, ParentRef, ParentRefs, SharedChain},
        walker::{MissingPeerInfo, NodeOutput, SubtreeMissingByPkg, Walker},
    },
    resolved_tree::{AncestorIds, ChildEdge, DependenciesTreeNode, TreeChildren},
};
use pnpm_deps_path::DepPath;
use pnpm_resolving_resolver_base::get_peer_version_range;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

/// One cached resolution of a non-pure subtree: the part of a walk's
/// verdict that holds in any compatible parent context, so a revisit —
/// by the same walk or another importer's — can reuse it.
///
/// `dep_path` is the value [`Walker::resolve_node`] would otherwise
/// recompute. `resolved_peers` is the external peer set (excluding
/// peers satisfied by this node's own children) — [`Walker::find_hit`]
/// uses it as the cache-match key against the current parent context.
/// `missing_peers` is the set of unmet peer requirements the original
/// walk surfaced — when a cache item carries a missing peer that the
/// current parent context *does* provide, the contexts are
/// incompatible and the item must be rejected. `missing_peers_of_children`
/// is the subset exposed as the package's children report.
///
/// The peer *providers* the walk resolved to are deliberately not part
/// of the verdict: they belong to the resolving walk's own context —
/// pnpm's resolver gives a not-new package `resolvedPeers: {}`
/// (`resolveDependencies.ts`) — so only the walk that first resolved a
/// subtree promotes its providers to importer level.
#[derive(Debug)]
pub(super) struct PeersCacheItem {
    /// The fully walked occurrence that produced this verdict. Cache
    /// hits are semantically equivalent to this node and must reuse its
    /// final depPath instead of being finalized as independent nodes.
    pub(super) owner_node_id: NodeId,
    pub(super) dep_path: DepPath,
    pub(super) resolved_peers: Arc<HashMap<String, NodeId>>,
    pub(super) missing_peers: Arc<HashMap<String, MissingPeerInfo>>,
    pub(super) missing_peers_of_children: Arc<HashMap<String, MissingPeerInfo>>,
    /// See [`NodeOutput::subtree_missing_by_pkg`]. Replayed on a cache
    /// hit so a discovery pass that never descends into the cached
    /// subtree still reports the same per-package missing breakdown a
    /// full walk of it would.
    pub(super) subtree_missing_by_pkg: SubtreeMissingByPkg,
}

impl PeersCacheItem {
    /// The [`NodeOutput`] a cache hit hands the reusing walk; output
    /// fields the verdict doesn't carry come back empty.
    fn to_node_output(&self) -> NodeOutput {
        NodeOutput {
            dep_path: self.dep_path.clone(),
            external_resolved_peers: Arc::clone(&self.resolved_peers),
            auto_install_resolved_peers: HashMap::default(),
            missing_peers: Arc::clone(&self.missing_peers),
            subtree_missing_by_pkg: self.subtree_missing_by_pkg.clone(),
        }
    }

    pub(super) fn to_cached_node_output(&self) -> CachedNodeOutput {
        CachedNodeOutput {
            owner_node_id: self.owner_node_id.clone(),
            output: self.to_node_output(),
            missing_peers_of_children: Arc::clone(&self.missing_peers_of_children),
        }
    }
}

pub(super) struct CachedNodeOutput {
    owner_node_id: NodeId,
    output: NodeOutput,
    missing_peers_of_children: Arc<HashMap<String, MissingPeerInfo>>,
}

enum DeferredChildResolution {
    Pure(DepPath),
    Cached(CachedNodeOutput),
    Materialize(Arc<str>),
}

pub(super) struct CacheHitContext<'a> {
    pub(super) node_id: &'a NodeId,
    pub(super) tree_node_depth: i32,
    pub(super) parent_chain_names: &'a SharedChain<String>,
    pub(super) parent_pkg_ids_chain: &'a SharedChain<String>,
    pub(super) preview_undo: Option<UndoRealize>,
}

pub(super) struct DeferredChildContext<'a> {
    pub(super) edge: &'a ChildEdge,
    pub(super) node_id: NodeId,
    pub(super) parent_ids: &'a AncestorIds,
    pub(super) parent_refs: &'a Arc<ParentRefs>,
    pub(super) parent_dep_paths: &'a Arc<HashMap<String, ParentPkgInfo>>,
    pub(super) chain_names: &'a SharedChain<String>,
    pub(super) parent_node_ids: &'a SharedChain<NodeId>,
    pub(super) parent_pkg_ids: &'a SharedChain<String>,
    pub(super) depth: i32,
}

enum FastProvider<'a> {
    Missing,
    Inherited(&'a ParentRef),
    Child(&'a str),
    Ambiguous,
}

enum FastCacheMatch {
    Match,
    NoMatch,
    Ambiguous,
}

#[derive(Debug, Default)]
pub(super) struct PeerProviderChildren {
    pub(super) relevant_edge_indices: Vec<usize>,
    pub(super) edge_indices_by_name: HashMap<String, Vec<usize>>,
}

pub(super) struct UndoRealize {
    newly_inserted: Vec<NodeId>,
    prev_parent_ids: AncestorIds,
}

impl Walker<'_> {
    /// Look up [`Self::peers_cache`] for a cached resolution of
    /// `pkg_id` whose parent peer context is compatible with the
    /// current `parent_refs`.
    ///
    /// A cache item matches when, for every cached resolved peer:
    ///
    /// 1. The current `parent_refs` has a counterpart entry for the
    ///    same name with a real `NodeId`.
    /// 2. Either the two `NodeId`s are equal, OR they map to the
    ///    same already-computed [`DepPath`] in
    ///    [`Self::node_dep_paths`], OR the two tree-nodes' resolved
    ///    package ids match — and in the package-id match case, the
    ///    deep [`Self::parent_packages_match`] check on the two
    ///    parents' own recorded contexts also succeeds (unless the
    ///    package id is itself in [`Self::pure_pkgs`], which makes
    ///    the deep check vacuous).
    /// 3. None of the cache item's missing-peer names are satisfied
    ///    by the current `parent_refs` — a name the cache walk
    ///    recorded as missing must still be missing here.
    pub(super) fn find_hit(
        &self,
        parent_refs: &ParentRefs,
        pkg_id: &str,
    ) -> Option<&PeersCacheItem> {
        let cache_items = self.peers_cache.get(pkg_id)?;
        cache_items.iter().find(|item| {
            for (name, cached_node_id) in item.resolved_peers.iter() {
                let Some(current_ref) = parent_refs.get(name) else {
                    return false;
                };
                if !self.parent_ref_matches_cached(current_ref, cached_node_id) {
                    return false;
                }
            }
            for missing_name in item.missing_peers.keys() {
                if parent_refs.contains_key(missing_name) {
                    return false;
                }
            }
            true
        })
    }

    /// Compare two `NodeId`s' recorded parent peer contexts:
    /// both nodes' contexts must have the same set of peer-relevant
    /// names, every name must resolve to the same version or
    /// `pkgIdWithPatchHash`, and — when a peer is shadowed (an
    /// `occurrence > 0` somewhere on either side) — the contexts
    /// must additionally agree on depth/`purePkgs` to compensate
    /// for the loss of single-occurrence guarantees on the
    /// shallow-equality path.
    fn parent_packages_match(&self, cached_node_id: &NodeId, current_node_id: &NodeId) -> bool {
        let Some(cached_parents) = self.parent_pkgs_of_node.get(cached_node_id) else {
            return false;
        };
        let Some(current_parents) = self.parent_pkgs_of_node.get(current_node_id) else {
            return false;
        };
        if cached_parents.len() != current_parents.len() {
            return false;
        }
        let max_depth = current_parents.values().map(|info| info.depth).max().unwrap_or(0);
        let peer_deps_not_shadowed = parent_pkgs_have_single_occurrence(cached_parents)
            && parent_pkgs_have_single_occurrence(current_parents);
        for (name, cached_info) in cached_parents.iter() {
            let Some(current_info) = current_parents.get(name) else { return false };
            // Version-only match covers `link:` parents only when
            // both recorded contexts are version-only.
            if let (Some(cached_version), Some(current_version)) =
                (&cached_info.version, &current_info.version)
            {
                if cached_version == current_version {
                    continue;
                }
                return false;
            }
            // Package-id match with shadowing guard.
            let Some(cached_pkg_id) = cached_info.pkg_id.as_ref() else { return false };
            if cached_info.pkg_id != current_info.pkg_id {
                return false;
            }
            if !(peer_deps_not_shadowed
                || current_info.depth == max_depth
                || self.pure_pkgs.contains_key(&**cached_pkg_id))
            {
                return false;
            }
        }
        true
    }

    fn parent_ref_matches_cached(&self, current_ref: &ParentRef, cached_node_id: &NodeId) -> bool {
        let Some(current_node_id) = current_ref.node_id.as_ref() else {
            return false;
        };
        if current_node_id == cached_node_id {
            return true;
        }
        if let (Some(cached_dp), Some(current_dp)) =
            (self.node_dep_paths.get(cached_node_id), self.node_dep_paths.get(current_node_id))
            && cached_dp == current_dp
        {
            return true;
        }
        let (Some(cached_tree_node), Some(current_tree_node)) = (
            self.tree.dependencies_tree.get(cached_node_id),
            self.tree.dependencies_tree.get(current_node_id),
        ) else {
            return false;
        };
        let parent_pkg_id = &current_tree_node.resolved_package_id;
        if parent_pkg_id != &cached_tree_node.resolved_package_id {
            return false;
        }
        self.pure_pkgs.contains_key(&**parent_pkg_id)
            || self.parent_packages_match(cached_node_id, current_node_id)
    }

    pub(super) fn find_fast_hit(
        &self,
        node_id: &NodeId,
        parent_refs: &ParentRefs,
        pkg_id: &str,
    ) -> Option<&PeersCacheItem> {
        let TreeChildren::Lazy { .. } = &self.tree.dependencies_tree.get(node_id)?.children else {
            return None;
        };
        self.find_fast_hit_for_lazy(parent_refs, pkg_id)
    }

    fn find_fast_hit_for_lazy(
        &self,
        parent_refs: &ParentRefs,
        pkg_id: &str,
    ) -> Option<&PeersCacheItem> {
        let canonical_scc = self.canonical_scc();
        self.peers_cache.get(pkg_id)?.iter().find(|item| {
            matches!(
                self.fast_cache_item_matches(&canonical_scc, parent_refs, pkg_id, item),
                FastCacheMatch::Match,
            )
        })
    }

    fn fast_cache_item_matches(
        &self,
        canonical_scc: &HashMap<Arc<str>, usize>,
        parent_refs: &ParentRefs,
        pkg_id: &str,
        item: &PeersCacheItem,
    ) -> FastCacheMatch {
        let mut ambiguous = false;
        for (name, cached_node_id) in item.resolved_peers.iter() {
            match self.fast_provider_for_name(canonical_scc, parent_refs, pkg_id, name) {
                FastProvider::Missing => return FastCacheMatch::NoMatch,
                FastProvider::Inherited(current_ref) => {
                    if !self.parent_ref_matches_cached(current_ref, cached_node_id) {
                        return FastCacheMatch::NoMatch;
                    }
                }
                FastProvider::Child(child_pkg_id) => {
                    let Some(cached_tree_node) = self.tree.dependencies_tree.get(cached_node_id)
                    else {
                        return FastCacheMatch::NoMatch;
                    };
                    if &*cached_tree_node.resolved_package_id != child_pkg_id {
                        return FastCacheMatch::NoMatch;
                    }
                    let child_is_stable = self.pure_pkgs.contains_key(child_pkg_id)
                        || matches!(
                            cached_node_id,
                            NodeId::Leaf(cached_pkg_id) if cached_pkg_id.as_ref() == child_pkg_id,
                        );
                    if !child_is_stable {
                        ambiguous = true;
                    }
                }
                FastProvider::Ambiguous => ambiguous = true,
            }
        }
        for name in item.missing_peers.keys() {
            match self.fast_provider_for_name(canonical_scc, parent_refs, pkg_id, name) {
                FastProvider::Missing => {}
                FastProvider::Inherited(_) | FastProvider::Child(_) => {
                    return FastCacheMatch::NoMatch;
                }
                FastProvider::Ambiguous => ambiguous = true,
            }
        }
        if ambiguous { FastCacheMatch::Ambiguous } else { FastCacheMatch::Match }
    }

    fn fast_provider_for_name<'a>(
        &'a self,
        canonical_scc: &HashMap<Arc<str>, usize>,
        parent_refs: &'a ParentRefs,
        pkg_id: &str,
        name: &str,
    ) -> FastProvider<'a> {
        let inherited = parent_refs.get(name);
        let Some(children) = self.tree.children_by_id.get(pkg_id) else {
            return inherited.map_or(FastProvider::Missing, FastProvider::Inherited);
        };
        let Some(edge_indices) = self
            .peer_provider_children_by_pkg_id
            .get(pkg_id)
            .and_then(|providers| providers.edge_indices_by_name.get(name))
        else {
            return inherited.map_or(FastProvider::Missing, FastProvider::Inherited);
        };

        let mut child_pkg_id = None;
        for &edge_index in edge_indices {
            let edge = &children[edge_index];
            if Self::cuts_cycle_edge(canonical_scc, pkg_id, &edge.pkg_id) {
                continue;
            }
            if child_pkg_id.is_some() {
                return FastProvider::Ambiguous;
            }
            child_pkg_id = Some(&*edge.pkg_id);
        }

        match (inherited, child_pkg_id) {
            (Some(_), Some(_)) => FastProvider::Ambiguous,
            (Some(parent_ref), None) => FastProvider::Inherited(parent_ref),
            (None, Some(pkg_id)) => FastProvider::Child(pkg_id),
            (None, None) => FastProvider::Missing,
        }
    }

    pub(super) fn finish_cache_hit(
        &mut self,
        cached: CachedNodeOutput,
        context: CacheHitContext<'_>,
    ) -> NodeOutput {
        let CacheHitContext {
            node_id,
            tree_node_depth,
            parent_chain_names,
            parent_pkg_ids_chain,
            preview_undo,
        } = context;
        let CachedNodeOutput { owner_node_id, output, missing_peers_of_children } = cached;
        self.undo_realize(node_id, preview_undo, None);

        if !output.missing_peers.is_empty() {
            let pkg_id = std::sync::Arc::<str>::clone(
                &self.tree.dependencies_tree[node_id].resolved_package_id,
            );
            let chain_with_self = parent_pkg_ids_chain.pushed(pkg_id.to_string());
            for (peer_name, info) in output.missing_peers.iter() {
                if self.missing_issue_suppressed(&chain_with_self, peer_name) {
                    continue;
                }
                self.record_missing_issue(
                    peer_name,
                    MissingPeer {
                        wanted_range: get_peer_version_range(&info.range),
                        raw_range: info.range.clone(),
                        optional: info.optional,
                        parents: self.issue_parents(parent_chain_names),
                    },
                    &chain_with_self,
                );
            }
        }
        self.remember_resolved_node(node_id, &output.dep_path);
        if !self.discovery {
            if &owner_node_id != node_id {
                let owner_is_fully_walked =
                    !self.cache_owner_by_node_id.contains_key(&owner_node_id);
                debug_assert!(
                    owner_is_fully_walked,
                    "cache owner {owner_node_id:?} of {node_id:?} is itself a cache hit",
                );
                self.cache_owner_by_node_id.insert(node_id.clone(), owner_node_id);
            }
            self.node_external_peers
                .insert(node_id.clone(), Arc::clone(&output.external_resolved_peers));
            self.node_missing_peers.insert(node_id.clone(), Arc::clone(&output.missing_peers));
            self.node_missing_peers_of_children.insert(node_id.clone(), missing_peers_of_children);
        }
        if let Some(node) = self.graph.get_mut(&output.dep_path)
            && node.depth > tree_node_depth
        {
            node.depth = tree_node_depth;
        }
        self.in_progress.remove(node_id);
        output
    }

    fn deferred_child_resolution(
        &self,
        parent_refs: &ParentRefs,
        pkg_id: &Arc<str>,
    ) -> DeferredChildResolution {
        if let Some(dep_path) = self.pure_pkgs.get(&**pkg_id)
            && self.tree.packages[&**pkg_id].peer_dependencies.is_empty()
        {
            return DeferredChildResolution::Pure(dep_path.clone());
        }
        if let Some(cached) = self
            .find_fast_hit_for_lazy(parent_refs, pkg_id)
            .map(PeersCacheItem::to_cached_node_output)
            && cached.output.missing_peers.is_empty()
        {
            return DeferredChildResolution::Cached(cached);
        }
        DeferredChildResolution::Materialize(Arc::<str>::clone(pkg_id))
    }

    pub(super) fn resolve_deferred_child(
        &mut self,
        context: DeferredChildContext<'_>,
    ) -> NodeOutput {
        let DeferredChildContext {
            edge,
            node_id,
            parent_ids,
            parent_refs,
            parent_dep_paths,
            chain_names,
            parent_node_ids,
            parent_pkg_ids,
            depth,
        } = context;
        match self.deferred_child_resolution(parent_refs, &edge.pkg_id) {
            DeferredChildResolution::Pure(dep_path) => NodeOutput {
                dep_path,
                external_resolved_peers: Arc::clone(&self.empty_resolved_peers),
                auto_install_resolved_peers: HashMap::default(),
                missing_peers: Arc::clone(&self.empty_missing_peers),
                subtree_missing_by_pkg: None,
            },
            DeferredChildResolution::Cached(cached) => cached.output,
            DeferredChildResolution::Materialize(pkg_id) => {
                self.tree.dependencies_tree.insert(
                    node_id.clone(),
                    DependenciesTreeNode::new(
                        pkg_id,
                        TreeChildren::Lazy { parent_ids: parent_ids.clone() },
                        depth,
                        true,
                    ),
                );
                let output = self.resolve_node(
                    &node_id,
                    parent_refs,
                    parent_dep_paths,
                    chain_names,
                    parent_node_ids,
                    parent_pkg_ids,
                );
                if !self.parent_pkgs_of_node.contains_key(&node_id)
                    && !should_retain_materialized_node(
                        &self.retained_peer_node_ids,
                        Some(&output),
                        &node_id,
                    )
                {
                    self.tree.dependencies_tree.remove(&node_id);
                    self.node_dep_paths.remove(&node_id);
                    self.visited_this_call.remove(&node_id);
                }
                output
            }
        }
    }

    pub(super) fn previously_resolved_children(
        &mut self,
        parent_node_ids: &SharedChain<NodeId>,
        parent_pkg_ids_chain: &SharedChain<String>,
        current_pkg_id: &str,
    ) -> BTreeMap<String, NodeId> {
        let mut children = BTreeMap::new();
        if !parent_pkg_ids_chain.iter().any(|pkg_id| pkg_id == current_pkg_id) {
            return children;
        }
        for parent_node_id in parent_node_ids.iter() {
            let same_pkg = self
                .tree
                .dependencies_tree
                .get(parent_node_id)
                .is_some_and(|node| &*node.resolved_package_id == current_pkg_id);
            if same_pkg {
                for (alias, child_node_id) in self.realize_children(parent_node_id).0.iter() {
                    children.entry(alias.clone()).or_insert_with(|| child_node_id.clone());
                }
            }
        }
        children
    }

    pub(super) fn optional_child_aliases(
        &self,
        pkg_id: &str,
        edges: &BTreeMap<String, NodeId>,
    ) -> HashSet<String> {
        self.tree
            .children_by_id
            .get(pkg_id)
            .into_iter()
            .flat_map(|children| children.iter())
            .filter(|edge| edge.optional && edges.contains_key(&edge.alias))
            .map(|edge| edge.alias.clone())
            .collect()
    }

    pub(super) fn preview_peer_provider_children(
        &mut self,
        node_id: &NodeId,
    ) -> (BTreeMap<String, NodeId>, Option<UndoRealize>) {
        let (parent_ids, pkg_id, depth) = {
            let node = &self.tree.dependencies_tree[node_id];
            match &node.children {
                TreeChildren::Realized(children) => {
                    let providers = children
                        .iter()
                        .filter(|(alias, child_node_id)| {
                            self.tree
                                .dependencies_tree
                                .get(*child_node_id)
                                .and_then(|child| {
                                    self.tree.packages.get(&child.resolved_package_id)
                                })
                                .is_some_and(|pkg| self.is_peer_relevant(alias, pkg))
                        })
                        .map(|(alias, child_node_id)| (alias.clone(), child_node_id.clone()))
                        .collect();
                    return (providers, None);
                }
                TreeChildren::Lazy { parent_ids } => (
                    parent_ids.clone(),
                    std::sync::Arc::<str>::clone(&node.resolved_package_id),
                    node.depth,
                ),
            }
        };
        let children = self.tree.children_by_id.get(&pkg_id).cloned().unwrap_or_default();
        let provider_edge_indices = self
            .peer_provider_children_by_pkg_id
            .get(&*pkg_id)
            .map_or(&[][..], |providers| providers.relevant_edge_indices.as_slice());
        let canonical_scc = self.canonical_scc();
        let full_chain = parent_ids.pushed(pkg_id.to_string());
        let mut providers = BTreeMap::new();
        let mut newly_inserted = Vec::new();
        for &edge_index in provider_edge_indices {
            let edge = &children[edge_index];
            let Some(pkg) = self.tree.packages.get(&edge.pkg_id) else { continue };
            if Self::cuts_cycle_edge(&canonical_scc, &pkg_id, &edge.pkg_id) {
                continue;
            }
            let child_node_id =
                if pkg.is_leaf { NodeId::leaf(&edge.pkg_id) } else { NodeId::next() };
            if !self.tree.dependencies_tree.contains_key(&child_node_id) {
                self.tree.dependencies_tree.insert(
                    child_node_id.clone(),
                    DependenciesTreeNode::new(
                        std::sync::Arc::<str>::clone(&edge.pkg_id),
                        TreeChildren::Lazy { parent_ids: full_chain.clone() },
                        depth + 1,
                        true,
                    ),
                );
                newly_inserted.push(child_node_id.clone());
            }
            providers.insert(edge.alias.clone(), child_node_id);
        }
        (providers, Some(UndoRealize { newly_inserted, prev_parent_ids: parent_ids }))
    }

    /// Realize the `(alias → NodeId)` children of `node_id` if it's
    /// currently a [`TreeChildren::Lazy`] entry; return the realized
    /// map (cloned for the caller). On a [`TreeChildren::Realized`]
    /// entry, just clones and returns. Expands the thunk on demand:
    ///
    /// 1. Walk [`crate::ResolvedTree::children_by_id`] for this node's
    ///    package id.
    /// 2. Skip any child whose pkg id appears in `parent_ids` — that
    ///    edge would form a cycle.
    /// 3. For each surviving child, allocate a per-occurrence
    ///    `NodeId` (leaves reuse the deterministic `NodeId::leaf`
    ///    for the leaf-collapse the eager walker does too) and
    ///    insert a fresh `dependencies_tree` entry with another
    ///    `Lazy` children variant that carries `parent_ids +
    ///    [self_pkg_id]` for cycle break on its own descendants.
    /// 4. Flip this node's `children` field to `Realized` so a
    ///    later visitor reuses the map.
    fn realize_children(
        &mut self,
        node_id: &NodeId,
    ) -> (Arc<BTreeMap<String, NodeId>>, Option<UndoRealize>) {
        self.realize_children_with(node_id, None)
    }

    pub(super) fn realize_children_with(
        &mut self,
        node_id: &NodeId,
        previewed: Option<&BTreeMap<String, NodeId>>,
    ) -> (Arc<BTreeMap<String, NodeId>>, Option<UndoRealize>) {
        // Snapshot the bits we need; we'll mutate `self.tree` below
        // and can't hold a borrow on the entry across the mutation.
        let (parent_ids, pkg_id, depth) = {
            let node = &self.tree.dependencies_tree[node_id];
            match &node.children {
                // Cheap: the realized map is shared, not copied per revisit.
                TreeChildren::Realized(map) => {
                    return (Arc::clone(map), None);
                }
                TreeChildren::Lazy { parent_ids } => (
                    parent_ids.clone(),
                    std::sync::Arc::<str>::clone(&node.resolved_package_id),
                    node.depth,
                ),
            }
        };
        let children_spec = match self.tree.children_by_id.get(&pkg_id) {
            Some(spec) => Arc::clone(spec),
            // No spec means the first walk never recorded children
            // for this package id — defensive empty case.
            None => Arc::new(Vec::new()),
        };
        let child_depth = depth + 1;
        let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
        let mut newly_inserted: Vec<NodeId> = Vec::new();
        let canonical_scc = self.canonical_scc();
        let full_chain = parent_ids.pushed(pkg_id.to_string());
        for edge in children_spec.iter() {
            if Self::cuts_cycle_edge(&canonical_scc, &pkg_id, &edge.pkg_id) {
                // A canonical back-edge is still a real dependency edge:
                // record it against the target's shared canonical
                // occurrence without giving the walk a path through it.
                if pkg_id != edge.pkg_id {
                    let node_id = self.canonical_backedge_node(&edge.pkg_id, child_depth);
                    realized.insert(edge.alias.clone(), node_id);
                }
                continue;
            }
            // Reuse the first walk's classification (persisted on
            // `ResolvedPackage::is_leaf` by `pkg_is_leaf`). Defaults
            // to non-leaf when the package isn't in `packages` — same
            // shape as the eager walker's `manifest == None` arm,
            // and `NodeId::next()` keeps occurrences distinct so a
            // later visit can still observe per-call-site state.
            let previewed_node_id = previewed.and_then(|previewed| previewed.get(&edge.alias));
            let child_node_id = if let Some(previewed_node_id) = previewed_node_id {
                previewed_node_id.clone()
            } else {
                let is_leaf = self.tree.packages.get(&edge.pkg_id).is_some_and(|pkg| pkg.is_leaf);
                if is_leaf { NodeId::leaf(&edge.pkg_id) } else { NodeId::next() }
            };
            let child_parent_ids = full_chain.clone();
            if let Some(node) = self.tree.dependencies_tree.get_mut(&child_node_id) {
                if node.depth > child_depth {
                    node.depth = child_depth;
                }
            } else {
                self.tree.dependencies_tree.insert(
                    child_node_id.clone(),
                    DependenciesTreeNode::new(
                        std::sync::Arc::<str>::clone(&edge.pkg_id),
                        TreeChildren::Lazy { parent_ids: child_parent_ids },
                        child_depth,
                        true,
                    ),
                );
                newly_inserted.push(child_node_id.clone());
            }
            realized.insert(edge.alias.clone(), child_node_id);
        }
        let realized = Arc::new(realized);
        // Replace this node's `Lazy` with `Realized` so future
        // visitors reuse the work.
        if let Some(node) = self.tree.dependencies_tree.get_mut(node_id) {
            node.children = TreeChildren::Realized(Arc::clone(&realized));
        }
        (realized, Some(UndoRealize { newly_inserted, prev_parent_ids: parent_ids }))
    }

    pub(super) fn undo_realize(
        &mut self,
        node_id: &NodeId,
        undo: Option<UndoRealize>,
        output: Option<&NodeOutput>,
    ) {
        let Some(undo) = undo else { return };
        for child_id in &undo.newly_inserted {
            if should_retain_materialized_node(&self.retained_peer_node_ids, output, child_id) {
                continue;
            }
            self.tree.dependencies_tree.remove(child_id);
            self.parent_pkgs_of_node.remove(child_id);
            self.node_dep_paths.remove(child_id);
            self.visited_this_call.remove(child_id);
        }
        if let Some(node) = self.tree.dependencies_tree.get_mut(node_id) {
            node.children = TreeChildren::Lazy { parent_ids: undo.prev_parent_ids };
        }
    }
}

/// Combines preview and final-materialization undo logs for the same node.
/// Both logs restore the same pre-realization ancestor chain; previewing
/// does not change the node's lazy parent state.
pub(super) fn merge_realize_undo(
    first: Option<UndoRealize>,
    second: Option<UndoRealize>,
) -> Option<UndoRealize> {
    match (first, second) {
        (None, undo) | (undo, None) => undo,
        (Some(mut first), Some(second)) => {
            first.newly_inserted.extend(second.newly_inserted);
            Some(first)
        }
    }
}

fn should_retain_materialized_node(
    retained_peer_node_ids: &HashSet<NodeId>,
    output: Option<&NodeOutput>,
    node_id: &NodeId,
) -> bool {
    retained_peer_node_ids.contains(node_id)
        || output.is_some_and(|output| {
            output.external_resolved_peers.values().any(|resolved_id| resolved_id == node_id)
                || output
                    .auto_install_resolved_peers
                    .values()
                    .any(|resolved_id| resolved_id == node_id)
        })
}

/// Whether every entry in `parents` has `occurrence == 0`.
fn parent_pkgs_have_single_occurrence(parents: &HashMap<String, ParentPkgInfo>) -> bool {
    parents.values().all(|info| info.occurrence == 0)
}

#[cfg(test)]
mod tests;
