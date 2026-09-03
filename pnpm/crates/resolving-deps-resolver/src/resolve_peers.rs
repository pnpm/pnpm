//! Resolves peer dependencies for a resolved dependency tree.
//!
//! Walks the per-occurrence [`crate::ResolvedTree::dependencies_tree`]
//! depth-first, propagating a [`ParentRefs`](context::ParentRefs) map of available parents
//! down the chain, and matches each visited package's
//! [`crate::ResolvedPackage::peer_dependencies`] against that map.
//! Produces a [`DependenciesGraph`] keyed by depPath plus the
//! `direct → DepPath` map the install layer consumes.
//!
//! Beyond the correctness surface — peer matching, depPath construction
//! with per-occurrence variation, missing / bad peer issue collection,
//! transitive-peer propagation, and the basic cycle break — the walk
//! carries two performance caches:
//!
//! - **`peersCache`** — caches resolved peer combinations keyed by
//!   `pkgIdWithPatchHash` so a repeat visit short-circuits the walk
//!   when the current parent peer context matches one the cache has
//!   already seen. Stored on [`Walker::peers_cache`] and matched via
//!   [`Walker::find_hit`] + [`Walker::parent_packages_match`].
//! - **`purePkgs` fast path** — a pure package (no resolved / missing
//!   peers across its entire subtree) gets its `depPath` equal to its
//!   `pkgIdWithPatchHash` without recursing. Stored on
//!   [`Walker::pure_pkgs`] and consulted at the top of
//!   [`Walker::resolve_node`].
//!   The cache is populated bottom-up: a node lands in `purePkgs` only
//!   when both its own walked subtree and (transitively) every cached
//!   subtree it relies on report no resolved or missing peers.
//!
//! Cycle handling is synchronous: a post-order traversal with an
//! `in_progress` set, where a re-entry on the same `NodeId` falls back
//! to `name@version` as the peer-id.

mod cache;
mod context;
mod discovery;
mod finalize;
mod walker;

use crate::{
    dedupe_injected_deps::dedupe_injected_deps,
    dedupe_peer_dependents::dedupe_peer_dependents,
    dependencies_graph::{DependenciesGraph, PeerDependencyIssues},
    node_id::NodeId,
    resolved_tree::{DirectDep, ResolvedTree},
};
pub(crate) use context::SharedChain;
use context::{ChainSuffixMemo, CurrentProviderSource, importer_relative_link_dep_path};
use discovery::PeerDiscoveryCaches;
pub(crate) use discovery::{PeerDiscoveryResult, PeerHoistDiscovery, apply_hoist_missing_scope};
use pnpm_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use walker::Walker;
pub(crate) use walker::{MissingNames, index_missing_names};

/// Options threaded into [`fn@resolve_peers`].
#[derive(Debug, Clone)]
pub struct ResolvePeersOptions {
    /// Cap on the rendered peer-suffix length before pacquet swaps the
    /// suffix for its short hash (default 1000).
    pub peers_suffix_max_length: usize,

    /// When `true`, every resolved-peer slot in the depPath suffix
    /// renders as `name@version` instead of the peer's own depPath,
    /// collapsing recursive peer suffixes like
    /// `(foo@1.0.0(bar@2.0.0))` into `(foo@1.0.0)`.
    pub dedupe_peers: bool,

    /// When `true`, `link:` direct dependencies whose target lives
    /// outside [`lockfile_dir`](Self::lockfile_dir) are seeded into
    /// the peer-resolution parent map with a node id remapped to
    /// `link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`, so peer
    /// resolution against those parents stays stable across machines.
    pub exclude_links_from_lockfile: bool,

    /// Absolute path of the directory `pnpm-lock.yaml` lives in. Used
    /// (a) as the anchor for the subdir check that gates the remap,
    /// and (b) as the base for the relative path the remapped link
    /// node id encodes. `None` disables the remap.
    pub lockfile_dir: Option<std::path::PathBuf>,

    /// Absolute root of the importer whose direct dependency map is
    /// being rendered. Snapshot graph edges remain lockfile-root-relative.
    /// Also the base a `link:` dependency's importer-relative target is
    /// resolved against before the `excludeLinksFromLockfile` remap
    /// tests it for containment in [`lockfile_dir`](Self::lockfile_dir).
    pub project_dir: Option<std::path::PathBuf>,

    /// Absolute path of the importer's `node_modules` directory. Used
    /// to compose `<modules_dir>/<alias>` as the remap target.
    /// `None` disables the remap.
    pub modules_dir: Option<std::path::PathBuf>,

    /// When `Some`, missing-peer issues declared inside a subtree
    /// whose root package's shared children context is owned by a
    /// *different* importer are not emitted. The peer-hoist loop sets
    /// this so a non-owner package occurrence keeps the missing-peer
    /// report computed under the owner occurrence's alias chain —
    /// only the package's *own* peers are re-evaluated per occurrence.
    /// The final workspace-wide pass leaves this `None` so warnings
    /// still cover every importer. Held by `Arc`: the maps inside are
    /// per-importer snapshots shared across every hoist-loop
    /// iteration.
    pub hoist_missing_scope: Option<std::sync::Arc<HoistMissingScope>>,

    /// `NodeIds` of the peer providers the auto-install-peers loop
    /// attached to importers' direct dependencies for reuse by other
    /// subtrees. Each such node keeps its original position inside
    /// the dependency tree, so the walk resolves its peers there;
    /// walking it a second time as a root child would bind its peers
    /// against the importer's own dependencies instead of the
    /// providers next to it in the tree, and the root-context result
    /// would overwrite the in-place one in `node_dep_paths`. The walk
    /// therefore skips these direct entries unless the tree position
    /// was pruned (an ancestor hit the peers cache) and nothing else
    /// resolved them.
    pub hoisted_peer_provider_node_ids: HashSet<NodeId>,

    /// Final `NodeId → DepPath` map produced by a previous
    /// peer-resolution pass over the same tree
    /// ([`ResolvePeersResult::paths_by_node_id`]). `Some` activates
    /// locked-peer-provider reuse: a node whose
    /// [`locked_peer_context`](crate::DependenciesTreeNode::locked_peer_context)
    /// names a provider is re-pinned to it when the provider is still
    /// reachable, resolved to the same path, and inside the current
    /// peer range. The upstream `resolvedPeerProviderPaths` option.
    pub resolved_peer_provider_paths: Option<HashMap<NodeId, DepPath>>,

    /// Populate [`ResolvePeersResult::paths_by_node_id`]. Off by
    /// default: the map is only consumed by a follow-up
    /// locked-peer-provider pass, and building it costs an extra pass
    /// over every walked node inside the hoist loop's hot path.
    pub collect_paths_by_node_id: bool,

    /// Direct-dependency aliases the importer's manifest declares.
    /// Input to the must-win guard: a declared current provider that
    /// has no wanted-lockfile resolution beats a locked one.
    pub declared_direct_dependencies: HashSet<String>,

    /// Direct-dependency aliases the user explicitly requested on the
    /// command line (`pnpm add foo`). Input to the must-win guard: an
    /// explicitly requested current provider always beats a locked one.
    pub explicitly_requested_direct_dependencies: HashSet<String>,
}

impl Default for ResolvePeersOptions {
    fn default() -> Self {
        ResolvePeersOptions {
            peers_suffix_max_length: 1000,
            dedupe_peers: false,
            exclude_links_from_lockfile: false,
            lockfile_dir: None,
            project_dir: None,
            modules_dir: None,
            hoist_missing_scope: None,
            hoisted_peer_provider_node_ids: HashSet::default(),
            resolved_peer_provider_paths: None,
            collect_paths_by_node_id: false,
            declared_direct_dependencies: HashSet::default(),
            explicitly_requested_direct_dependencies: HashSet::default(),
        }
    }
}

/// See [`ResolvePeersOptions::hoist_missing_scope`].
#[derive(Debug, Clone)]
pub struct HoistMissingScope {
    /// The importer whose hoist input is being computed.
    pub importer_id: String,
    /// `pkg id → children-owner importer id`, from
    /// [`crate::WorkspaceTreeCtx::first_importer_by_pkg`]. Held by
    /// `Arc` (like the map below) so the workspace barrier snapshots
    /// the context once and shares it across every importer's scope.
    pub first_importer_by_pkg: Arc<HashMap<String, String>>,
    /// Per package: the missing-peer names reported under the current
    /// children-owner context, from
    /// [`crate::WorkspaceTreeCtx::first_walk_missing_by_pkg`]. A
    /// missing peer inside a foreign-claimed subtree is suppressed
    /// only when the owner's walk did *not* report it missing —
    /// i.e. that context satisfied it, so the shared children report
    /// filtered it out at walk time. Misses the owner walk could not
    /// satisfy stay visible to every importer (and each hoists its
    /// own copy).
    pub first_walk_missing_by_pkg: Arc<HashMap<String, HashSet<String>>>,
    /// Peers represented by the wanted lockfile must remain eligible
    /// for importer-local hoisting during lockfile re-resolution.
    pub locked_peer_names: Arc<HashSet<String>>,
}

impl HoistMissingScope {
    /// `true` when a miss of `peer_name` declared under the given
    /// ancestor chain is covered by another importer's shared walk.
    fn suppresses_chain(
        &self,
        ancestor_pkg_ids: &SharedChain<String>,
        peer_name: &str,
        memo: &mut ChainSuffixMemo<String>,
    ) -> bool {
        if self.locked_peer_names.contains(peer_name) {
            return false;
        }
        ancestor_pkg_ids.any_memoized(memo, |pkg_id| self.covers(pkg_id, peer_name))
    }

    /// The unmemoized form, for callers that cannot keep every queried
    /// chain alive (see [`SharedChain::any_memoized`]).
    fn suppresses_iter<'a>(
        &self,
        ancestor_pkg_ids: impl Iterator<Item = &'a String>,
        peer_name: &str,
    ) -> bool {
        if self.locked_peer_names.contains(peer_name) {
            return false;
        }
        ancestor_pkg_ids.into_iter().any(|pkg_id| self.covers(pkg_id, peer_name))
    }

    /// Whether another importer's shared walk of `pkg_id` already
    /// reported on `peer_name` — and found it satisfied.
    fn covers(&self, pkg_id: &str, peer_name: &str) -> bool {
        self.first_importer_by_pkg.get(pkg_id).is_some_and(|owner| {
            *owner != self.importer_id
                && self
                    .first_walk_missing_by_pkg
                    .get(pkg_id)
                    .is_some_and(|missing| !missing.contains(peer_name))
        })
    }
}

/// Output bag of [`fn@resolve_peers`].
#[derive(Debug, Default)]
pub struct ResolvePeersResult {
    pub graph: DependenciesGraph,
    pub direct_dependencies_by_alias: BTreeMap<String, DepPath>,
    /// Real peer providers that were resolved from inside the walked
    /// dependency tree. The auto-install-peers loop appends these to
    /// the importer's hidden direct-dep set.
    pub resolved_peer_providers_by_alias: BTreeMap<String, NodeId>,
    pub peer_dependency_issues: PeerDependencyIssues,
    /// Per resolved package: the union of missing-peer names its
    /// occurrences' children reported in this walk. The hoist loop
    /// persists the owner-context map per package so non-owner importers
    /// can tell which descendant misses the owner resolver's context
    /// already satisfied. See [`HoistMissingScope`].
    pub missing_names_by_pkg: HashMap<String, HashSet<String>>,
    /// Final `DepPath` of every walked node — the upstream
    /// `pathsByNodeId`. Feed it back through
    /// [`ResolvePeersOptions::resolved_peer_provider_paths`] to run a
    /// locked-peer-provider reuse pass.
    pub paths_by_node_id: HashMap<NodeId, DepPath>,
}

/// One importer's input to the multi-importer [`fn@resolve_peers_workspace`]
/// — the lockfile importer id, the importer's `directNodeIdsByAlias`
/// slice, the absolute project root, and the per-importer
/// `modules_dir` used by the `excludeLinksFromLockfile` link-remap.
#[derive(Debug, Clone)]
pub struct ImporterPeerInput {
    pub id: String,
    pub direct: Vec<DirectDep>,
    /// Absolute root of this importer. Threaded into
    /// [`ResolvePeersOptions::project_dir`] while this importer is being
    /// walked, and used to render its direct `link:` deps relative to
    /// itself.
    pub root_dir: PathBuf,
    /// Absolute path of this importer's `node_modules` directory.
    /// Threaded into [`ResolvePeersOptions::modules_dir`] while this
    /// importer is being walked so the `excludeLinksFromLockfile` link
    /// remap uses the correct per-importer target. `None` disables
    /// the remap for this importer.
    pub modules_dir: Option<PathBuf>,
}

/// Output of [`fn@resolve_peers_workspace`] — the cross-importer
/// dedupe map plus per-importer `direct_dependencies_by_alias` slices.
#[derive(Debug, Default)]
pub struct WorkspaceResolvePeersResult {
    pub graph: DependenciesGraph,
    pub direct_dependencies_by_importer: BTreeMap<String, BTreeMap<String, DepPath>>,
    pub peer_dependency_issues_by_importer: BTreeMap<String, PeerDependencyIssues>,
    /// Final `DepPath` of every walked node — the upstream
    /// `pathsByNodeId`. See [`ResolvePeersResult::paths_by_node_id`].
    pub paths_by_node_id: HashMap<NodeId, DepPath>,
}

/// Resolve peer dependencies for `tree` and emit a depPath-keyed graph.
///
/// Takes `tree` by `&mut` because lazy [`crate::TreeChildren`] entries are
/// realised in-place during the walk — every revisit's `(alias →
/// NodeId)` children map is allocated on first descent and the
/// parent's `TreeChildren::Lazy` flips to `Realized` so a second
/// visitor reuses the map without redoing the work. Pure subtrees
/// that the resolver short-circuits via its `purePkgs` cache never get
/// realised.
pub fn resolve_peers(tree: &mut ResolvedTree, opts: ResolvePeersOptions) -> ResolvePeersResult {
    let node_ids_by_previous_dep_path = build_node_ids_by_previous_dep_path(tree, &opts);
    let current_provider_sources = vec![CurrentProviderSource {
        direct_node_ids_by_alias: tree
            .direct
            .iter()
            .map(|dep| (dep.alias.clone(), dep.node_id.clone()))
            .collect(),
        declared_direct_dependencies: opts.declared_direct_dependencies.clone(),
        explicitly_requested_direct_dependencies: opts
            .explicitly_requested_direct_dependencies
            .clone(),
    }];
    let walker = Walker::new(
        tree,
        opts,
        node_ids_by_previous_dep_path,
        current_provider_sources,
        PeerDiscoveryCaches::default(),
        false,
    );
    walker.walk()
}

/// Resolve peer dependencies for every importer in `importers` against
/// the shared `tree`, then rewrite injected workspace deps that
/// dedupe back to `link:` symlinks.
///
/// One Walker walks every importer's direct deps in sequence so
/// `peersCache` + `purePkgs` are shared across importers, then the
/// in-crate `dedupe_injected_deps` pass runs once with all importers'
/// direct deps in scope.
pub fn resolve_peers_workspace(
    tree: &mut ResolvedTree,
    importers: &[ImporterPeerInput],
    lockfile_dir: &Path,
    dedupe_injected_deps_enabled: bool,
    dedupe_peer_dependents_enabled: bool,
    resolve_peers_from_workspace_root: bool,
    opts: ResolvePeersOptions,
) -> WorkspaceResolvePeersResult {
    let node_ids_by_previous_dep_path = build_node_ids_by_previous_dep_path(tree, &opts);
    let mut walker = Walker::new(
        tree,
        opts,
        node_ids_by_previous_dep_path,
        Vec::new(),
        PeerDiscoveryCaches::default(),
        false,
    );

    // Walk importers in id order. Occurrence realization and the shared
    // verdict caches are first-writer-wins, so a stable walk order makes
    // the graph a function of the importer set rather than of the
    // caller's listing order (pnpm/pnpm#13846).
    let importers: Vec<&ImporterPeerInput> = {
        let mut sorted: Vec<&ImporterPeerInput> = importers.iter().collect();
        sorted.sort_by(|left, right| left.id.cmp(&right.id));
        sorted
    };

    let mut direct_dependencies_by_importer: BTreeMap<String, BTreeMap<String, DepPath>> =
        BTreeMap::new();
    let mut peer_dependency_issues_by_importer: BTreeMap<String, PeerDependencyIssues> =
        BTreeMap::new();
    let mut importer_root_dirs: BTreeMap<String, PathBuf> = BTreeMap::new();
    let root_importer = resolve_peers_from_workspace_root
        .then(|| importers.iter().copied().find(|importer| importer.id == "."))
        .flatten();
    let root_parents = root_importer.map(|importer| {
        let previous_dirs = (walker.opts.project_dir.clone(), walker.opts.modules_dir.clone());
        walker.opts.project_dir = Some(importer.root_dir.clone());
        walker.opts.modules_dir.clone_from(&importer.modules_dir);
        let parents = walker.build_importer_parents_from(&importer.direct);
        (walker.opts.project_dir, walker.opts.modules_dir) = previous_dirs;
        parents
    });
    for importer in &importers {
        importer_root_dirs.insert(importer.id.clone(), importer.root_dir.clone());
        // Swap the per-importer `project_dir` / `modules_dir` in before
        // the walk so the `excludeLinksFromLockfile` link-remap inside
        // `resolve_node` resolves link targets against the right
        // importer and encodes the correct importer-scoped target.
        walker.opts.project_dir = Some(importer.root_dir.clone());
        walker.opts.modules_dir.clone_from(&importer.modules_dir);
        walker.current_provider_sources = importer_provider_sources(importer, root_importer);
        let importer_parents =
            Arc::new(if root_importer.is_some_and(|root| root.id != importer.id) {
                let mut refs = root_parents.clone().unwrap_or_default();
                refs.extend(walker.build_importer_parents_from(&importer.direct));
                refs
            } else {
                walker.build_importer_parents_from(&importer.direct)
            });
        let parent_chain_names = SharedChain::default();
        let parent_node_ids = SharedChain::default();
        let parent_pkg_ids_chain = SharedChain::default();
        let importer_parent_dep_paths = walker.parent_dep_paths_from_refs(&importer_parents);
        let (own_direct, provider_direct): (Vec<&DirectDep>, Vec<&DirectDep>) = importer
            .direct
            .iter()
            .partition(|dep| !walker.opts.hoisted_peer_provider_node_ids.contains(&dep.node_id));
        for dep in &own_direct {
            walker.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &importer_parent_dep_paths,
            );
        }
        for dep in &own_direct {
            walker.resolve_node(
                &dep.node_id,
                &importer_parents,
                &importer_parent_dep_paths,
                &parent_chain_names,
                &parent_node_ids,
                &parent_pkg_ids_chain,
            );
        }
        // See ResolvePeersOptions::hoisted_peer_provider_node_ids — a
        // provider is normally resolved at its tree position during the
        // walk above; only one whose position was pruned still needs the
        // root-context fallback.
        for dep in &provider_direct {
            if walker.visited_this_call.contains(&dep.node_id) {
                continue;
            }
            walker.remember_parent_context_if_peer_provider(
                &dep.alias,
                &dep.node_id,
                &importer_parent_dep_paths,
            );
            walker.resolve_node(
                &dep.node_id,
                &importer_parents,
                &importer_parent_dep_paths,
                &parent_chain_names,
                &parent_node_ids,
                &parent_pkg_ids_chain,
            );
        }
        walker.drain_pending_canonical_nodes(&importer_parents, &importer_parent_dep_paths);
        let issues = std::mem::take(&mut walker.issues);
        if !issues.bad.is_empty() || !issues.missing.is_empty() {
            peer_dependency_issues_by_importer.insert(importer.id.clone(), issues);
        }
    }
    walker.patch_pending_peer_edges();
    // Recompute depPaths with full peer suffixes once, after every
    // importer is walked, then rebuild the graph and re-key each
    // importer's direct deps.
    let final_dep_paths = walker.build_final_dep_paths();
    for importer in &importers {
        let anchor = crate::link_target::ImporterAnchor::new(&importer.root_dir, lockfile_dir);
        let direct_by_alias: BTreeMap<String, DepPath> = importer
            .direct
            .iter()
            .map(|dep| {
                let dep_path = walker.final_dep_path_of(&dep.node_id, &final_dep_paths);
                let dep_path = importer_relative_link_dep_path(
                    &dep_path,
                    &anchor,
                    Some(lockfile_dir),
                    Some(&importer.root_dir),
                );
                (dep.alias.clone(), dep_path)
            })
            .collect();
        direct_dependencies_by_importer.insert(importer.id.clone(), direct_by_alias);
    }
    let mut graph = walker.build_final_graph(&final_dep_paths);
    let paths_by_node_id = walker.final_paths_by_node_id(&final_dep_paths);

    if dedupe_injected_deps_enabled {
        dedupe_injected_deps(
            &mut graph,
            &mut direct_dependencies_by_importer,
            &importer_root_dirs,
            lockfile_dir,
        );
    }

    if dedupe_peer_dependents_enabled {
        dedupe_peer_dependents(&mut graph, &mut direct_dependencies_by_importer);
    }

    WorkspaceResolvePeersResult {
        graph,
        direct_dependencies_by_importer,
        peer_dependency_issues_by_importer,
        paths_by_node_id,
    }
}

/// The current-provider sources visible while walking `importer`: its
/// own direct dependencies, plus the workspace root importer's when
/// that is a different project. The workspace path carries no
/// declared/explicitly-requested alias sets yet, so those guards stay
/// empty here.
fn importer_provider_sources(
    importer: &ImporterPeerInput,
    root_importer: Option<&ImporterPeerInput>,
) -> Vec<CurrentProviderSource> {
    let source_of = |importer: &ImporterPeerInput| CurrentProviderSource {
        direct_node_ids_by_alias: importer
            .direct
            .iter()
            .map(|dep| (dep.alias.clone(), dep.node_id.clone()))
            .collect(),
        declared_direct_dependencies: HashSet::default(),
        explicitly_requested_direct_dependencies: HashSet::default(),
    };
    let mut sources = vec![source_of(importer)];
    if let Some(root) = root_importer
        && root.id != importer.id
    {
        sources.push(source_of(root));
    }
    sources
}

/// The upstream `getNodeIdsByPreviousDepPath`: first node (in
/// deterministic id order) claiming each `previous_dep_path`. Empty
/// unless a locked-peer reuse pass was requested.
fn build_node_ids_by_previous_dep_path(
    tree: &ResolvedTree,
    opts: &ResolvePeersOptions,
) -> HashMap<DepPath, NodeId> {
    let mut map = HashMap::default();
    if opts.resolved_peer_provider_paths.is_none() {
        return map;
    }
    let mut node_ids: Vec<&NodeId> = tree.dependencies_tree.keys().collect();
    node_ids.sort();
    for node_id in node_ids {
        if let Some(previous) = tree.dependencies_tree[node_id].previous_dep_path()
            && !map.contains_key(previous)
        {
            map.insert(previous.clone(), node_id.clone());
        }
    }
    map
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
