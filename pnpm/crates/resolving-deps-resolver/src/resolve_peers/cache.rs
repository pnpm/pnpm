//! The walk's short-circuits: the `purePkgs` fast path, `peersCache`
//! matching against the current parent context, deferred realization of
//! lazy children, and the retention decisions that keep a materialized
//! node alive after its subtree is reused.

mod realize_children;

mod fast_match;
mod provider_match;

use crate::{
    dependencies_graph::MissingPeer,
    node_id::NodeId,
    resolve_peers::{
        context::{ParentRef, ParentRefs, SharedChain},
        walker::{
            MissingPeerInfo, NodeOutput, NodeWalkContext, PeerWalkMode, SubtreeMissingByPkg, Walker,
        },
    },
    resolved_tree::{ChildEdge, DependenciesTreeNode, TreeChildren},
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

/// The per-node context every child edge of one `realize_children_with` call
/// is realized against.
struct EdgeRealization<'a> {
    canonical_scc: &'a HashMap<Arc<str>, usize>,
    pkg_id: &'a Arc<str>,
    child_depth: i32,
    previewed: Option<&'a BTreeMap<String, NodeId>>,
}

pub(super) struct CacheHitContext<'a> {
    pub(super) node_id: &'a NodeId,
    pub(super) tree_node_depth: i32,
    pub(super) parent_chain_names: &'a SharedChain<Arc<str>>,
    pub(super) parent_pkg_ids_chain: &'a SharedChain<Arc<str>>,
    pub(super) preview_undo: Option<UndoRealize>,
}

pub(super) struct DeferredChildContext<'a> {
    pub(super) edge: &'a ChildEdge,
    pub(super) node_id: NodeId,
    pub(super) walk: &'a NodeWalkContext<'a>,
    pub(super) depth: i32,
}

/// The context a peer-provider lookup resolves against: everything except the
/// peer name being looked up.
#[derive(Clone, Copy)]
struct FastProviderQuery<'a> {
    canonical_scc: &'a HashMap<Arc<str>, usize>,
    parent_refs: &'a ParentRefs,
    pkg_id: &'a str,
}

enum FastProvider<'a> {
    Missing,
    Inherited(&'a ParentRef),
    Child(&'a str),
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheMatch {
    Match,
    NoMatch,
    Ambiguous,
}

#[derive(Debug, Default)]
pub(super) struct PeerProviderChildren {
    pub(super) relevant_edge_indices: Vec<usize>,
    pub(super) edge_indices_by_name: HashMap<String, Vec<usize>>,
}

/// A lazy node whose provider children are previewed.
struct LazyProviders {
    pkg_id: std::sync::Arc<str>,
    depth: i32,
}

pub(super) struct UndoRealize {
    newly_inserted: Vec<NodeId>,
}

pub(super) fn reuse_peer_subtrees(opts: &super::ResolvePeersOptions) -> bool {
    #[cfg(test)]
    {
        matches!(opts.cache_mode, super::test_support::PeerCacheMode::Enabled)
    }
    #[cfg(not(test))]
    {
        let _ = opts;
        true
    }
}

impl Walker<'_> {
    /// Reuse a cached subtree only when provider identities match and every
    /// missing peer remains missing.
    pub(super) fn find_hit(
        &self,
        parent_refs: &ParentRefs,
        pkg_id: &str,
    ) -> Option<&PeersCacheItem> {
        if !reuse_peer_subtrees(&self.opts) {
            return None;
        }
        let cache_items = self.caches.peers_cache.get(pkg_id)?;
        cache_items
            .iter()
            .find(|item| self.item_matches(item, parent_refs))
    }

    fn item_matches(&self, item: &PeersCacheItem, parent_refs: &ParentRefs) -> bool {
        let resolved_still_match = item.resolved_peers
            .iter()
            .all(|(name, cached_node_id)| {
                parent_refs
                    .get(name)
                    .is_some_and(|current_ref| {
                        self.parent_ref_matches_cached(current_ref, cached_node_id)
                    })
            });
        resolved_still_match
            && !item.missing_peers
                .keys()
                .any(|missing| parent_refs.contains_key(missing))
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
        let CachedNodeOutput {
            owner_node_id,
            output,
            missing_peers_of_children,
        } = cached;
        self.undo_realize(node_id, preview_undo, None);

        self.record_cache_hit_missing_issues(
            node_id,
            &output,
            &missing_peers_of_children,
            parent_chain_names,
            parent_pkg_ids_chain,
        );
        self.remember_resolved_node(node_id, &output.dep_path);
        self.remember_cache_hit_node(node_id, owner_node_id, &output, missing_peers_of_children);
        if let Some(node) = self.output.graph.get_mut(&output.dep_path)
            && node.depth > tree_node_depth
        {
            node.depth = tree_node_depth;
        }
        self.traversal.in_progress.remove(node_id);
        output
    }

    fn record_cache_hit_missing_issues(
        &mut self,
        node_id: &NodeId,
        output: &NodeOutput,
        missing_peers_of_children: &HashMap<String, MissingPeerInfo>,
        parent_chain_names: &SharedChain<Arc<str>>,
        parent_pkg_ids_chain: &SharedChain<Arc<str>>,
    ) {
        if output.missing_peers.is_empty() {
            return;
        }
        let pkg_id = Arc::<str>::clone(&self.tree.dependencies_tree[node_id].resolved_package_id);
        let pkg = self.owned_package(&pkg_id);
        let own_peers = pkg.peer_set.taken();
        let chain_with_self = parent_pkg_ids_chain.pushed(Arc::clone(&pkg_id));
        match self.traversal.mode {
            PeerWalkMode::Discovery => {
                self.record_discovery_cache_hit_issues(
                    &pkg,
                    output,
                    missing_peers_of_children,
                    parent_chain_names,
                    parent_pkg_ids_chain,
                    &chain_with_self,
                );
                return;
            }
            PeerWalkMode::Final => {}
        }
        for (peer_name, info) in output.missing_peers.iter() {
            let declaring_chain = if own_peers.contains_key(peer_name) {
                parent_pkg_ids_chain
            } else {
                &chain_with_self
            };
            self.record_cache_hit_issue_if_needed(
                peer_name,
                info,
                parent_chain_names,
                declaring_chain,
            );
        }
    }

    fn record_discovery_cache_hit_issues(
        &mut self,
        pkg: &crate::ResolvedPackage,
        output: &NodeOutput,
        missing_peers_of_children: &HashMap<String, MissingPeerInfo>,
        parent_chain_names: &SharedChain<Arc<str>>,
        parent_pkg_ids_chain: &SharedChain<Arc<str>>,
        chain_with_self: &SharedChain<Arc<str>>,
    ) {
        for (peer_name, peer_dep) in pkg.peer_set.taken() {
            if output.missing_peers.contains_key(peer_name) {
                self.record_cache_hit_issue_if_needed(
                    peer_name,
                    &MissingPeerInfo {
                        range: peer_dep.version.clone(),
                        declaring_pkg_id: Arc::clone(&pkg.id),
                        optional: peer_dep.optional,
                    },
                    parent_chain_names,
                    parent_pkg_ids_chain,
                );
            }
        }
        for (peer_name, info) in missing_peers_of_children {
            self.record_cache_hit_issue_if_needed(
                peer_name,
                info,
                parent_chain_names,
                chain_with_self,
            );
        }
    }

    fn record_cache_hit_issue_if_needed(
        &mut self,
        peer_name: &str,
        info: &MissingPeerInfo,
        parent_chain_names: &SharedChain<Arc<str>>,
        declaring_chain: &SharedChain<Arc<str>>,
    ) {
        if self.missing_issue_suppressed(declaring_chain, peer_name) {
            return;
        }
        self.record_missing_issue(
            peer_name,
            MissingPeer {
                wanted_range: get_peer_version_range(&info.range),
                raw_range: self.hoist_peer_specifier(info),
                optional: info.optional,
                parents: self.issue_parents(parent_chain_names),
            },
            declaring_chain,
        );
    }

    /// Discovery keeps provider bindings for retained paths. Final walks also
    /// record cache ownership and missing-peer bookkeeping.
    fn remember_cache_hit_node(
        &mut self,
        node_id: &NodeId,
        owner_node_id: NodeId,
        output: &NodeOutput,
        missing_peers_of_children: Arc<HashMap<String, MissingPeerInfo>>,
    ) {
        self.remember_provider_bindings(node_id, &output.external_resolved_peers);
        match self.traversal.mode {
            PeerWalkMode::Discovery => return,
            PeerWalkMode::Final => {}
        }
        if &owner_node_id != node_id {
            let owner_is_fully_walked = !self.nodes.cache_owners.contains_key(&owner_node_id);
            debug_assert!(
                owner_is_fully_walked,
                "cache owner {owner_node_id:?} of {node_id:?} is itself a cache hit",
            );
            self.nodes.cache_owners.insert(node_id.clone(), owner_node_id);
        }
        self.nodes.missing_peers.insert(node_id.clone(), Arc::clone(&output.missing_peers));
        self.nodes.children_missing_peers.insert(node_id.clone(), missing_peers_of_children);
    }
}

/// Combines preview and final-materialization undo logs for the same node.
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
            output.external_resolved_peers
                .values()
                .any(|resolved_id| resolved_id == node_id)
                || output.auto_install_resolved_peers
                    .values()
                    .any(|resolved_id| resolved_id == node_id)
        })
}

#[cfg(test)]
mod tests;
