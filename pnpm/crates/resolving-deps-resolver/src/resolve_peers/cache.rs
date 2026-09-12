//! The walk's short-circuits: the `purePkgs` fast path, `peersCache`
//! matching against the current parent context, deferred realization of
//! lazy children, and the retention decisions that keep a materialized
//! node alive after its subtree is reused.

mod realize_children;

mod fast_match;

use crate::{
    dependencies_graph::MissingPeer,
    node_id::NodeId,
    resolve_peers::{
        context::{ParentPkgInfo, ParentRef, ParentRefs, SharedChain},
        walker::{MissingPeerInfo, NodeOutput, NodeWalkContext, SubtreeMissingByPkg, Walker},
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

/// The per-node context every child edge of one `realize_children_with` call
/// is realized against.
struct EdgeRealization<'a> {
    canonical_scc: &'a HashMap<Arc<str>, usize>,
    full_chain: &'a AncestorIds,
    pkg_id: &'a Arc<str>,
    child_depth: i32,
    previewed: Option<&'a BTreeMap<String, NodeId>>,
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

/// A lazy node whose provider children are previewed.
struct LazyProviders {
    parent_ids: AncestorIds,
    pkg_id: std::sync::Arc<str>,
    depth: i32,
}

pub(super) struct UndoRealize {
    newly_inserted: Vec<NodeId>,
    prev_parent_ids: AncestorIds,
}

/// What compensates for the loss of single-occurrence guarantees on the
/// shallow-equality path: with a peer shadowed anywhere on either side, the
/// contexts must additionally agree on depth or be a pure package.
#[derive(Clone, Copy)]
struct ShadowingGuard {
    max_depth: i32,
    peer_deps_not_shadowed: bool,
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
        cache_items.iter().find(|item| self.item_matches(item, parent_refs))
    }

    fn item_matches(&self, item: &PeersCacheItem, parent_refs: &ParentRefs) -> bool {
        let resolved_still_match = item.resolved_peers.iter().all(|(name, cached_node_id)| {
            parent_refs.get(name).is_some_and(|current_ref| {
                self.parent_ref_matches_cached(current_ref, cached_node_id)
            })
        });
        resolved_still_match
            && !item.missing_peers.keys().any(|missing| parent_refs.contains_key(missing))
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
        let shadowing = ShadowingGuard {
            max_depth: current_parents.values().map(|info| info.depth).max().unwrap_or(0),
            peer_deps_not_shadowed: parent_pkgs_have_single_occurrence(cached_parents)
                && parent_pkgs_have_single_occurrence(current_parents),
        };
        cached_parents.iter().all(|(name, cached_info)| {
            current_parents.get(name).is_some_and(|current_info| {
                self.parent_pkg_matches(cached_info, current_info, shadowing)
            })
        })
    }

    /// One recorded parent package: a version-only match covers `link:`
    /// parents when both contexts are version-only; otherwise the package
    /// ids must agree and pass the shadowing guard.
    fn parent_pkg_matches(
        &self,
        cached_info: &ParentPkgInfo,
        current_info: &ParentPkgInfo,
        shadowing: ShadowingGuard,
    ) -> bool {
        if let (Some(cached_version), Some(current_version)) =
            (&cached_info.version, &current_info.version)
        {
            return cached_version == current_version;
        }
        let Some(cached_pkg_id) = cached_info.pkg_id.as_ref() else { return false };
        if cached_info.pkg_id != current_info.pkg_id {
            return false;
        }
        shadowing.peer_deps_not_shadowed
            || current_info.depth == shadowing.max_depth
            || self.pure_pkgs.contains_key(&**cached_pkg_id)
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

        self.record_cache_hit_missing_issues(
            node_id,
            &output,
            parent_chain_names,
            parent_pkg_ids_chain,
        );
        self.remember_resolved_node(node_id, &output.dep_path);
        self.remember_cache_hit_node(node_id, owner_node_id, &output, missing_peers_of_children);
        if let Some(node) = self.graph.get_mut(&output.dep_path)
            && node.depth > tree_node_depth
        {
            node.depth = tree_node_depth;
        }
        self.in_progress.remove(node_id);
        output
    }

    fn record_cache_hit_missing_issues(
        &mut self,
        node_id: &NodeId,
        output: &NodeOutput,
        parent_chain_names: &SharedChain<String>,
        parent_pkg_ids_chain: &SharedChain<String>,
    ) {
        if output.missing_peers.is_empty() {
            return;
        }
        let pkg_id = Arc::<str>::clone(&self.tree.dependencies_tree[node_id].resolved_package_id);
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

    /// Discovery runs throw their per-node bookkeeping away, so it is only
    /// recorded for the real walk.
    fn remember_cache_hit_node(
        &mut self,
        node_id: &NodeId,
        owner_node_id: NodeId,
        output: &NodeOutput,
        missing_peers_of_children: Arc<HashMap<String, MissingPeerInfo>>,
    ) {
        if self.discovery {
            return;
        }
        if &owner_node_id != node_id {
            let owner_is_fully_walked = !self.cache_owner_by_node_id.contains_key(&owner_node_id);
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
