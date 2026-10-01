//! Peer-hoist discovery: the reduced walk the auto-install-peers /
//! hoist loop runs between rounds, plus the persistent tree view and
//! walker caches shared across those rounds.

use crate::{
    dependencies_graph::PeerDependencyIssues,
    node_id::NodeId,
    resolve_dependency_tree::SyncCursor,
    resolve_peers::{
        HoistMissingScope, ResolvePeersOptions,
        cache::{PeerProviderChildren, PeersCacheItem},
        context::{ChainSuffixMemo, CurrentProviderSource, ParentPkgInfo, SharedChain},
        walker::{
            MissingSummary, NodeOutput, RootWalk, Walker, children_scc_ids,
            index_peer_provider_children,
        },
    },
    resolved_tree::{DirectDep, ResolvedTree},
};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

/// Peer-hoist discovery engine: one persistent tree view + walker
/// caches shared by every hoist round of a workspace resolve. Replaces
/// the per-round full snapshot + full
/// [`resolve_peers`](fn@super::resolve_peers) walk that made
/// multi-importer hoist discovery quadratic in workspace size.
pub(crate) struct PeerHoistDiscovery {
    tree: ResolvedTree,
    caches: PeerDiscoveryCaches,
    synced_revision: Option<u64>,
    synced_children_rewrites: Option<u64>,
    /// How much of the workspace context's write log [`Self::tree`] has
    /// absorbed. Reset with the view.
    cursor: SyncCursor,
}

impl PeerHoistDiscovery {
    pub(crate) fn new() -> Self {
        PeerHoistDiscovery {
            tree: ResolvedTree::default(),
            caches: PeerDiscoveryCaches::default(),
            synced_revision: None,
            synced_children_rewrites: None,
            cursor: SyncCursor::default(),
        }
    }

    /// Run one discovery pass over `direct` (an importer's current
    /// direct-dep envelopes), refreshing the persistent tree view from
    /// `workspace` first when the context changed since the last pass.
    ///
    /// A children-ownership handover that rewrote existing occurrence
    /// nodes (the children-rewrite counter in [`crate::WorkspaceTreeCtx::tree`]) discards
    /// the whole view: the retained realized children and the walk
    /// verdicts derived from them predate the rewrite, and an
    /// incremental sync cannot tell which of them the rewrite
    /// invalidated.
    /// `parents_direct` is the importer's full direct set — it defines
    /// the importer-level peer providers every walked subtree matches
    /// against. `walk_direct` is the slice actually walked; a hoist
    /// round that only added new direct deps passes just the additions
    /// and merges the result into its previous rounds' output.
    pub(crate) fn discover(
        &mut self,
        workspace: &crate::resolve_dependency_tree::WorkspaceTreeCtx,
        parents_direct: &[DirectDep],
        walk_direct: &[DirectDep],
        opts: ResolvePeersOptions,
    ) -> PeerDiscoveryResult {
        let revision = workspace.tree.revision();
        if self.synced_revision != Some(revision) {
            let children_rewrites = workspace.tree.children_rewrites();
            let stale =
                self.synced_children_rewrites.is_some_and(|synced| synced != children_rewrites);
            if stale || !workspace.sync_discovery_tree(&mut self.tree, &mut self.cursor) {
                self.tree = ResolvedTree::default();
                self.caches = PeerDiscoveryCaches {
                    view_generation: self.caches.view_generation,
                    ..PeerDiscoveryCaches::default()
                };
                workspace.rebuild_discovery_tree(&mut self.tree, &mut self.cursor);
            }
            self.caches.view_generation.advance();
            self.synced_children_rewrites = Some(children_rewrites);
            self.synced_revision = Some(revision);
        }
        let (result, caches) = discover_peers(
            &mut self.tree,
            parents_direct,
            walk_direct,
            std::mem::take(&mut self.caches),
            opts,
        );
        self.caches = caches;
        result
    }
}

/// Walker state that stays valid across peer-hoist discovery passes of
/// one workspace resolve: the walked tree grows monotonically between
/// passes ([`crate::WorkspaceTreeCtx::sync_discovery_tree`] rebuilds
/// the engine's view on the incompatible exceptions), so a subtree
/// verdict recorded under one importer's walk short-circuits every
/// compatible revisit — [`Walker::find_hit`] re-validates each
/// [`PeersCacheItem`] against the current parent context, and a hoisted
/// provider that newly satisfies a cached missing peer rejects the
/// stale item. This is the same sharing
/// [`resolve_peers_workspace`](fn@super::resolve_peers_workspace)
/// already applies across importers within its single final pass.
#[derive(Debug, Default)]
pub(crate) struct PeerDiscoveryCaches {
    pub(super) node_dep_paths: HashMap<NodeId, DepPath>,
    pub(super) pure_pkgs: HashMap<Arc<str>, DepPath>,
    pub(super) peers_cache: HashMap<Arc<str>, Vec<PeersCacheItem>>,
    pub(super) parent_pkgs_of_node: HashMap<NodeId, Arc<HashMap<String, ParentPkgInfo>>>,
    pub(super) retained_peer_node_ids: HashSet<NodeId>,
    pub(super) peer_providers: PeerProviderIndex,
    pub(super) canonical_cycles: CanonicalCycleGate,
    view_generation: ViewGeneration,
}

impl PeerDiscoveryCaches {
    pub(super) fn view_generation(&self) -> ViewGeneration {
        self.view_generation
    }
}

/// Which refresh of the discovery engine's tree view a cache was built
/// under; [`PeerHoistDiscovery::discover`] advances it with every
/// refresh or rebuild of the view.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ViewGeneration(u64);

impl ViewGeneration {
    /// Step to the generation of the next view.
    pub(super) fn advance(&mut self) {
        self.0 += 1;
    }
}

/// Which child edges of each package can stand in as peer-dependency
/// providers (see [`index_peer_provider_children`]), indexed for the
/// walkers of one tree view and valid for the peer names it was built
/// under; see [`Self::refresh`].
#[derive(Debug, Default)]
pub(super) struct PeerProviderIndex {
    pub(super) children_by_pkg_id: HashMap<Arc<str>, PeerProviderChildren>,
    peer_names: HashSet<String>,
}

impl PeerProviderIndex {
    /// Bring the index up to `tree` and its peer names.
    pub(super) fn refresh(&mut self, tree: &ResolvedTree) {
        if self.peer_names != tree.all_peer_dep_names {
            self.children_by_pkg_id.clear();
            self.peer_names.clone_from(&tree.all_peer_dep_names);
        }
        index_peer_provider_children(tree, &mut self.children_by_pkg_id);
    }
}

/// The persistent state of the canonical cycle gate
/// ([`Walker::cuts_cycle_edge`]): the SCC table the gate reads and the
/// shared record-only occurrence per canonical back-edge target.
#[derive(Debug, Default)]
pub(super) struct CanonicalCycleGate {
    /// The children-graph SCC ids [`Walker::cuts_cycle_edge`] reads, a
    /// function of the view's [`ResolvedTree::children_by_id`]; see
    /// [`Self::table`].
    sccs: RefCell<Option<SccTable>>,
    /// The shared record-only occurrence per canonical back-edge
    /// target; persisted so later rounds reuse instead of re-creating
    /// (and re-walking) them. Entries are validated against the current
    /// tree on lookup, so a walker over a different tree view recreates
    /// what its tree lacks.
    pub(super) backedge_nodes: HashMap<Arc<str>, NodeId>,
}

/// The SCC table of one view generation.
#[derive(Debug)]
struct SccTable {
    view_generation: ViewGeneration,
    ids: Arc<HashMap<Arc<str>, usize>>,
}

impl CanonicalCycleGate {
    /// The SCC table of `tree`'s children graph; reads under one
    /// `view_generation` share it.
    pub(super) fn table(
        &self,
        tree: &ResolvedTree,
        view_generation: ViewGeneration,
    ) -> Arc<HashMap<Arc<str>, usize>> {
        let mut sccs = self.sccs.borrow_mut();
        if let Some(table) = sccs.as_ref()
            && table.view_generation == view_generation
        {
            return Arc::clone(&table.ids);
        }
        let ids = Arc::new(children_scc_ids(tree));
        *sccs = Some(SccTable { view_generation, ids: Arc::clone(&ids) });
        ids
    }
}

/// What one peer-hoist discovery pass reports back to the hoist loop —
/// the subset of [`ResolvePeersResult`](super::ResolvePeersResult) the
/// loop actually consumes, so discovery never has to build a
/// [`crate::DependenciesGraph`].
#[derive(Debug, Default)]
pub(crate) struct PeerDiscoveryResult {
    /// See
    /// [`ResolvePeersResult::resolved_peer_providers_by_alias`](super::ResolvePeersResult::resolved_peer_providers_by_alias).
    pub(crate) resolved_peer_providers_by_alias: BTreeMap<String, NodeId>,
    pub(crate) peer_dependency_issues: PeerDependencyIssues,
    /// Ancestor `pkgIdWithPatchHash` chains recorded per missing-peer
    /// issue, consumed by [`fn@apply_hoist_missing_scope`].
    missing_ancestor_pkg_ids: HashMap<String, Vec<SharedChain<Arc<str>>>>,
    /// The pass's subtree missing-peer summaries, one per walked direct
    /// dep. Read through
    /// [`index_missing_names`](fn@crate::resolve_peers::index_missing_names)
    /// for the per-package view
    /// [`ResolvePeersResult::missing_names_by_pkg`](super::ResolvePeersResult::missing_names_by_pkg)
    /// materializes.
    pub(crate) missing_summaries: Vec<Arc<MissingSummary>>,
}

/// Walk `direct` in discovery mode: peer matching, caches, and issue
/// collection run exactly as in
/// [`resolve_peers`](fn@super::resolve_peers), but no
/// [`crate::DependenciesGraph`] is built and no final depPath pass
/// runs. The per-importer driver mirrors
/// [`resolve_peers_workspace`](fn@super::resolve_peers_workspace)'s
/// per-importer section.
fn discover_peers(
    tree: &mut ResolvedTree,
    parents_direct: &[DirectDep],
    walk_direct: &[DirectDep],
    caches: PeerDiscoveryCaches,
    opts: ResolvePeersOptions,
) -> (PeerDiscoveryResult, PeerDiscoveryCaches) {
    let current_provider_sources = discovery_provider_sources(parents_direct, &opts);
    let mut walker =
        Walker::new(tree, opts, HashMap::default(), current_provider_sources, caches, true);
    let root = RootWalk::of(&walker, parents_direct);
    let (own_direct, provider_direct): (Vec<&DirectDep>, Vec<&DirectDep>) = walk_direct
        .iter()
        .partition(|dep| !walker.opts.scope.hoisted_peer_provider_node_ids.contains(&dep.node_id));
    let mut result = PeerDiscoveryResult::default();
    for dep in &own_direct {
        walker.remember_parent_context_if_peer_provider(
            &dep.alias,
            &dep.node_id,
            &root.parent_dep_paths,
        );
    }
    for dep in &own_direct {
        result.fold(walker.resolve_node(&dep.node_id, &root.context()));
    }
    // See ResolvePeersOptions::hoisted_peer_provider_node_ids — a
    // provider is normally resolved at its tree position during the
    // walk above; only one whose position was pruned still needs the
    // root-context fallback.
    for dep in &provider_direct {
        if walker.traversal.visited_this_call.contains(&dep.node_id) {
            continue;
        }
        walker.remember_parent_context_if_peer_provider(
            &dep.alias,
            &dep.node_id,
            &root.parent_dep_paths,
        );
        result.fold(walker.resolve_node(&dep.node_id, &root.context()));
    }
    walker.drain_pending_canonical_nodes(&root.importer_parents, &root.parent_dep_paths);
    result.peer_dependency_issues = std::mem::take(&mut walker.output.issues);
    result.missing_ancestor_pkg_ids = std::mem::take(&mut walker.output.missing_ancestor_pkg_ids);
    (result, walker.into_caches())
}

impl PeerDiscoveryResult {
    fn fold(&mut self, output: NodeOutput) {
        for (peer_alias, peer_node_id) in output.auto_install_resolved_peers {
            self.resolved_peer_providers_by_alias.insert(peer_alias, peer_node_id);
        }
        if let Some(summary) = output.subtree_missing_by_pkg
            && !self.missing_summaries.iter().any(|seen| Arc::ptr_eq(seen, &summary))
        {
            self.missing_summaries.push(summary);
        }
    }
}

pub(crate) fn apply_hoist_missing_scope(
    result: &mut PeerDiscoveryResult,
    scope: &HoistMissingScope,
) {
    result.peer_dependency_issues.missing.retain(|peer_name, issues| {
        let ancestor_chains = result.missing_ancestor_pkg_ids.remove(peer_name).unwrap_or_default();
        // The issues reported for one peer come from occurrences spread
        // through the tree, whose ancestor chains share long suffixes.
        let mut memo = ChainSuffixMemo::default();
        *issues = std::mem::take(issues)
            .into_iter()
            .zip(ancestor_chains.iter())
            .filter_map(|(issue, ancestor_pkg_ids)| {
                (!scope.suppresses_chain(ancestor_pkg_ids, peer_name, &mut memo)).then_some(issue)
            })
            .collect();
        !issues.is_empty()
    });
}

#[cfg(test)]
mod tests;

fn discovery_provider_sources(
    parents_direct: &[DirectDep],
    opts: &ResolvePeersOptions,
) -> Vec<CurrentProviderSource> {
    vec![CurrentProviderSource {
        direct_node_ids_by_alias: parents_direct
            .iter()
            .map(|dep| (dep.alias.clone(), dep.node_id.clone()))
            .collect(),
        declared_direct_dependencies: opts.scope.declared_direct_dependencies.clone(),
        explicitly_requested_direct_dependencies: opts.scope
            .explicitly_requested_direct_dependencies
            .clone(),
    }]
}
