//! Pacquet port of pnpm's
//! [`resolvePeers`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts).
//!
//! Walks the per-occurrence [`crate::ResolvedTree::dependencies_tree`]
//! depth-first, propagating a [`ParentRefs`] map of available parents
//! down the chain, and matches each visited package's
//! [`crate::ResolvedPackage::peer_dependencies`] against that map.
//! Produces a [`DependenciesGraph`] keyed by depPath plus the
//! `direct → DepPath` map the install layer consumes.
//!
//! **Scope of this port.** The slice landing here covers the
//! correctness surface — peer matching, depPath construction with
//! per-occurrence variation, missing / bad peer issue collection,
//! transitive-peer propagation, and the basic cycle break — plus
//! upstream's two performance caches:
//!
//! - **`peersCache`** — caches resolved peer combinations keyed by
//!   `pkgIdWithPatchHash` so a repeat visit short-circuits the walk
//!   when the current parent peer context matches one the cache has
//!   already seen. Stored on [`Walker::peers_cache`] and matched via
//!   [`Walker::find_hit`] + [`Walker::parent_packages_match`].
//!   Ported from upstream's
//!   [`peersCache`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L342-L348).
//! - **`purePkgs` fast path** — a pure package (no resolved / missing
//!   peers across its entire subtree) gets its `depPath` equal to its
//!   `pkgIdWithPatchHash` without recursing. Stored on
//!   [`Walker::pure_pkgs`] and consulted at the top of
//!   [`Walker::resolve_node`]. Ported from upstream's
//!   [`purePkgs` early-return](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L398-L406).
//!   The set is populated bottom-up: a node lands in `purePkgs` only
//!   when both its own walked subtree and (transitively) every cached
//!   subtree it relies on report no resolved or missing peers.
//!
//! The one upstream optimisation pacquet does **not** port yet:
//!
//! - **`graph-cycles`-driven async deferment** — upstream's
//!   `pathsByNodeIdPromises` lets a cyclic peer pick a `name@version`
//!   peer-id once `analyzeGraph` confirms the cycle. Pacquet performs
//!   a synchronous post-order traversal with an `in_progress` set; a
//!   re-entry on the same `NodeId` falls back to `name@version` as
//!   the peer-id, which is what upstream's cycle resolution converges
//!   on anyway.

use crate::{
    dedupe_injected_deps::dedupe_injected_deps,
    dependencies_graph::{
        DependenciesGraph, DependenciesGraphNode, MissingPeer, ParentPackageRef,
        PeerDependencyIssue, PeerDependencyIssues,
    },
    node_id::NodeId,
    resolved_tree::{
        DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedTree, TreeChildren,
    },
};
use node_semver::{Range, Version};
use pacquet_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, link_path_to_peer_version};
use pacquet_resolving_resolver_base::ResolveResult;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Pull `(name, version)` out of a `ResolveResult` the peer-resolution
/// stage can hash and compare on.
///
/// The npm-registry resolver always fills [`ResolveResult::name_ver`],
/// so the fast path lifts it straight out. The git / tarball / local
/// resolvers leave it `None` (their canonical name lives in the
/// fetched manifest, which the resolver doesn't read at resolve
/// time); for those, fall back to `(alias, id-as-string)`. The peer
/// graph machinery only ever looks the name up in
/// [`ResolvedTree::all_peer_dep_names`] — a set that comes from
/// upstream's `parsePeerDependencies` over npm-shaped packages — so
/// the fallback's "name" will simply miss every lookup, naturally
/// short-circuiting peer propagation for non-npm packages without
/// panicking on `name_ver = None`.
/// Reinterpret a `link:<rel>` [`NodeId`] as a [`DepPath`].
///
/// Linked top-parent `NodeIds` (whether the workspace-link arm or the
/// `excludeLinksFromLockfile` remap) never enter the dependency tree,
/// so [`Walker::node_dep_paths`] never maps them. The `link:<rel>`
/// `NodeId` is itself a well-formed pnpm `DepPath`, so the snapshot
/// child edge can use it verbatim. Mirrors upstream's
/// [`pathsByNodeId.get(childNodeId) ?? (childNodeId as unknown as DepPath)`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/resolvePeers.ts#L164)
/// fallback in `resolveChildren`.
fn link_node_id_as_dep_path(node_id: &NodeId) -> Option<DepPath> {
    let NodeId::Leaf(id) = node_id else { return None };
    id.starts_with("link:").then(|| DepPath::from(id.to_string()))
}

fn pkg_name_version(result: &ResolveResult) -> (String, String) {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return (name_ver.name.to_string(), name_ver.suffix.to_string());
    }
    let fallback_name = result.alias.clone().unwrap_or_else(|| result.id.as_str().to_string());
    (fallback_name, result.id.as_str().to_string())
}

/// Options threaded into [`fn@resolve_peers`].
#[derive(Debug, Clone)]
pub struct ResolvePeersOptions {
    /// Cap on the rendered peer-suffix length before pacquet swaps the
    /// suffix for its short hash. Mirrors upstream's
    /// `peersSuffixMaxLength` (default 1000).
    pub peers_suffix_max_length: usize,

    /// When `true`, every resolved-peer slot in the depPath suffix
    /// renders as `name@version` instead of the peer's own depPath,
    /// collapsing recursive peer suffixes like
    /// `(foo@1.0.0(bar@2.0.0))` into `(foo@1.0.0)`. Mirrors pnpm's
    /// [`dedupePeers`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolvePeers.ts#L990-L997)
    /// branch in `peerNodeIdToPeerId`.
    pub dedupe_peers: bool,

    /// When `true`, `link:` direct dependencies whose target lives
    /// outside [`lockfile_dir`](Self::lockfile_dir) are seeded into
    /// the peer-resolution parent map with a node id remapped to
    /// `link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`, so peer
    /// resolution against those parents stays stable across machines.
    /// Mirrors upstream's
    /// [exclude-link `target` rewrite](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/index.ts#L232-L244).
    pub exclude_links_from_lockfile: bool,

    /// Absolute path of the directory `pnpm-lock.yaml` lives in. Used
    /// (a) as the anchor for the subdir check that gates the remap,
    /// and (b) as the base for the relative path the remapped link
    /// node id encodes. `None` disables the remap.
    pub lockfile_dir: Option<std::path::PathBuf>,

    /// Absolute path of the importer's `node_modules` directory. Used
    /// to compose `<modules_dir>/<alias>` as the remap target.
    /// `None` disables the remap.
    pub modules_dir: Option<std::path::PathBuf>,
}

impl Default for ResolvePeersOptions {
    fn default() -> Self {
        ResolvePeersOptions {
            peers_suffix_max_length: 1000,
            dedupe_peers: false,
            exclude_links_from_lockfile: false,
            lockfile_dir: None,
            modules_dir: None,
        }
    }
}

/// Output bag of [`fn@resolve_peers`]. Mirrors upstream's
/// [`resolvePeers` return shape](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L101-L102).
#[derive(Debug, Default)]
pub struct ResolvePeersResult {
    pub graph: DependenciesGraph,
    pub direct_dependencies_by_alias: BTreeMap<String, DepPath>,
    pub peer_dependency_issues: PeerDependencyIssues,
}

/// One importer's input to the multi-importer [`fn@resolve_peers_workspace`]
/// — the lockfile importer id, the importer's `directNodeIdsByAlias`
/// slice, the absolute project root, and the per-importer
/// `modules_dir` used by the `excludeLinksFromLockfile` link-remap.
/// Mirrors the per-project payload pnpm's `resolvePeers` reads off
/// `opts.projects`.
#[derive(Debug, Clone)]
pub struct ImporterPeerInput {
    pub id: String,
    pub direct: Vec<DirectDep>,
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
}

/// Resolve peer dependencies for `tree` and emit a depPath-keyed graph.
///
/// Takes `tree` by `&mut` because lazy [`TreeChildren`] entries are
/// realised in-place during the walk — every revisit's `(alias →
/// NodeId)` children map is allocated on first descent and the
/// parent's `TreeChildren::Lazy` flips to `Realized` so a second
/// visitor reuses the map without redoing the work. Pure subtrees
/// that the resolver short-circuits via its `purePkgs` set never get
/// realised.
pub fn resolve_peers(tree: &mut ResolvedTree, opts: ResolvePeersOptions) -> ResolvePeersResult {
    let walker = Walker {
        tree,
        opts,
        graph: DependenciesGraph::new(),
        issues: PeerDependencyIssues::default(),
        node_dep_paths: HashMap::new(),
        node_external_peers: HashMap::new(),
        node_missing_peers: HashMap::new(),
        in_progress: HashSet::new(),
        pending_peer_edges: Vec::new(),
        pure_pkgs: HashSet::new(),
        peers_cache: HashMap::new(),
        parent_pkgs_of_node: HashMap::new(),
    };
    walker.walk()
}

/// Resolve peer dependencies for every importer in `importers` against
/// the shared `tree`, then rewrite injected workspace deps that
/// dedupe back to `link:` symlinks.
///
/// Mirrors pnpm's
/// [`resolvePeers`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolvePeers.ts#L72)
/// multi-importer entry point: one Walker walks every importer's
/// direct deps in sequence so `peersCache` + `purePkgs` are shared
/// across importers, then the in-crate `dedupe_injected_deps` pass
/// runs once with all importers' direct deps in scope.
pub fn resolve_peers_workspace(
    tree: &mut ResolvedTree,
    importers: &[ImporterPeerInput],
    lockfile_dir: &Path,
    dedupe_injected_deps_enabled: bool,
    opts: ResolvePeersOptions,
) -> WorkspaceResolvePeersResult {
    let mut walker = Walker {
        tree,
        opts,
        graph: DependenciesGraph::new(),
        issues: PeerDependencyIssues::default(),
        node_dep_paths: HashMap::new(),
        node_external_peers: HashMap::new(),
        node_missing_peers: HashMap::new(),
        in_progress: HashSet::new(),
        pending_peer_edges: Vec::new(),
        pure_pkgs: HashSet::new(),
        peers_cache: HashMap::new(),
        parent_pkgs_of_node: HashMap::new(),
    };

    let mut direct_dependencies_by_importer: BTreeMap<String, BTreeMap<String, DepPath>> =
        BTreeMap::new();
    let mut peer_dependency_issues_by_importer: BTreeMap<String, PeerDependencyIssues> =
        BTreeMap::new();
    let mut importer_root_dirs: BTreeMap<String, PathBuf> = BTreeMap::new();
    for importer in importers {
        importer_root_dirs.insert(importer.id.clone(), importer.root_dir.clone());
        // Swap the per-importer `modules_dir` in before the walk so
        // the `excludeLinksFromLockfile` link-remap inside
        // `resolve_node` uses the correct importer-scoped target.
        walker.opts.modules_dir.clone_from(&importer.modules_dir);
        let importer_parents = walker.build_importer_parents_from(&importer.direct);
        let parent_chain_names: Vec<String> = Vec::new();
        let mut direct_by_alias = BTreeMap::new();
        for dep in &importer.direct {
            let output =
                walker.resolve_node(dep.node_id.clone(), &importer_parents, &parent_chain_names, 0);
            direct_by_alias.insert(dep.alias.clone(), output.dep_path);
        }
        direct_dependencies_by_importer.insert(importer.id.clone(), direct_by_alias);
        let issues = std::mem::take(&mut walker.issues);
        if !issues.bad.is_empty() || !issues.missing.is_empty() {
            peer_dependency_issues_by_importer.insert(importer.id.clone(), issues);
        }
    }
    walker.patch_pending_peer_edges();
    let mut graph = walker.graph;

    if dedupe_injected_deps_enabled {
        dedupe_injected_deps(
            &mut graph,
            &mut direct_dependencies_by_importer,
            &importer_root_dirs,
            lockfile_dir,
        );
    }

    WorkspaceResolvePeersResult {
        graph,
        direct_dependencies_by_importer,
        peer_dependency_issues_by_importer,
    }
}

/// Per-name entry in the propagating `ParentRefs` map. Mirrors upstream's
/// [`ParentRef`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L998-L1006).
#[derive(Debug, Clone)]
struct ParentRef {
    version: String,
    /// `None` for top-level deps that were already installed (upstream
    /// fills these from `topParents`). Pacquet doesn't surface those
    /// yet — `None` only appears on the importer-level cycle-break
    /// fallback, where the `name@version` form of the peer-id is the
    /// only useful representation.
    node_id: Option<NodeId>,
    /// Local install name in `node_modules`. May differ from the
    /// package's real name for npm-alias entries.
    ///
    /// Recorded for upstream parity but not read in this slice.
    /// Reserved for the npm-alias cache-validation path
    /// (pnpm/pnpm#11907).
    #[allow(dead_code, reason = "future npm-alias cache validation")]
    alias: Option<String>,
    /// Depth at which this parent was added. Threaded into
    /// [`ParentPkgInfo`] so [`Walker::parent_packages_match`] can
    /// apply upstream's depth-equality fallback when peer
    /// dependencies are shadowed across occurrences. Mirrors
    /// upstream's `ParentRef.depth`.
    depth: i32,
    /// Per-name shadowing counter. Incremented when a same-name
    /// parent is added at a deeper walk that doesn't match the
    /// existing entry. Mirrors upstream's `ParentRef.occurrence`;
    /// used by [`Walker::parent_packages_match`] to detect
    /// shadowed peers.
    occurrence: u32,
}

/// `name → ParentRef` map propagated down the walk. Entries are indexed
/// by both the package's real name and its alias when the two differ —
/// `react-dom@npm:next` resolves a `peerDependencies.react-dom`
/// requirement against the alias and `peerDependencies.next` against
/// the real name.
type ParentRefs = HashMap<String, ParentRef>;

struct Walker<'tree> {
    tree: &'tree mut ResolvedTree,
    opts: ResolvePeersOptions,
    graph: DependenciesGraph,
    issues: PeerDependencyIssues,
    /// `NodeId → DepPath` once a node has been walked. Mirrors
    /// upstream's `pathsByNodeId` map. Lets repeated visits (an
    /// importer-direct dep that's also reached transitively) reuse the
    /// already-computed depPath.
    node_dep_paths: HashMap<NodeId, DepPath>,
    /// Peers each node and its subtree resolved against ancestors —
    /// the "unknown resolved peers" upstream propagates up so a parent
    /// can fold its descendants' peer dependencies into its own peer
    /// suffix. Indexed by `NodeId`; value's keys are peer aliases.
    node_external_peers: HashMap<NodeId, HashMap<String, NodeId>>,
    /// Peers each node and its subtree declared but couldn't find.
    /// Indexed by `NodeId`; value's keys are peer aliases.
    node_missing_peers: HashMap<NodeId, HashMap<String, MissingPeerInfo>>,
    /// Stack of nodes currently being walked. Re-entry on a node here
    /// is a cycle — the recursion bottoms out with a `name@version`
    /// peer-id and the original visit drives the actual graph insert.
    in_progress: HashSet<NodeId>,
    /// Peer edges whose target `NodeId` had no `DepPath` yet at the
    /// time we built the parent's `graph_children` map — typically
    /// because the peer is a later sibling direct dep that the walker
    /// hasn't reached yet. `walk()` drains this list once every direct
    /// dep is walked and patches the recorded entries with the now-
    /// known peer `DepPath`. Without this post-pass the install layer
    /// would walk the parent's `children` map and find no symlink edge
    /// for the peer, leaving the package without the peer in its slot.
    pending_peer_edges: Vec<PendingPeerEdge>,
    /// Set of `pkgIdWithPatchHash` values whose full subtree resolved
    /// with zero external peers and zero missing peers. A revisit of
    /// any such package whose own `peerDependencies` is empty
    /// short-circuits with `depPath = pkgIdWithPatchHash` — no
    /// recursion, no peersCache lookup. Mirrors upstream's
    /// [`purePkgs` early-return](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L398-L406).
    /// Populated bottom-up: a node is added when its local `is_pure`
    /// flag is true after its own walk completes.
    pure_pkgs: HashSet<String>,
    /// Per-`pkgIdWithPatchHash` cached results from earlier walks of
    /// non-pure subtrees. Each cache item records the `depPath`, the
    /// external `(peer_name → NodeId)` map, and the `(peer_name →
    /// info)` missing set produced by one specific parent peer
    /// context. [`Walker::find_hit`] iterates the bucket and accepts
    /// the first item whose cached context is compatible with the
    /// current call's `child_parent_refs`. Mirrors upstream's
    /// [`peersCache`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L342-L348)
    /// + [`findHit`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L660-L699).
    ///
    /// The matcher omits upstream's `parentPackagesMatch` deep check
    /// `find_hit` calls
    /// [`Walker::parent_packages_match`] for the deep check upstream
    /// runs via `parentPkgsOfNode`.
    peers_cache: HashMap<String, Vec<PeersCacheItem>>,
    /// Per-`NodeId` snapshot of the parent peer context (peer-relevant
    /// names → [`ParentPkgInfo`]) recorded at the moment the walker
    /// first descended into that node. Backs
    /// [`Walker::parent_packages_match`]: a [`PeersCacheItem`] is a
    /// cache hit only when each of its resolved-peer `NodeId`s has an
    /// entry here whose recorded parent context still matches the
    /// current walk's `parent_refs` (or, for `purePkgs` peers, the
    /// presence-and-pkg-id match short-circuit). Mirrors upstream's
    /// [`parentPkgsOfNode`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L356)
    /// + [`parentPackagesMatch`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L701-L731).
    parent_pkgs_of_node: HashMap<NodeId, HashMap<String, ParentPkgInfo>>,
}

/// Per-peer-name snapshot stored on [`Walker::parent_pkgs_of_node`].
///
/// Mirrors upstream's
/// [`ParentPkgInfo`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L364-L369).
/// `pkg_id` is `None` for parents that came in without a real
/// `NodeId` (the importer-level `topParents` path upstream); those
/// fall back to a pure `version` comparison.
#[derive(Debug, Clone)]
struct ParentPkgInfo {
    pkg_id: Option<String>,
    version: Option<String>,
    depth: i32,
    occurrence: u32,
}

/// One cached resolution of a non-pure subtree.
///
/// `dep_path` is the value [`Walker::resolve_node`] would otherwise
/// recompute. `resolved_peers` is the external peer set (excluding
/// peers satisfied by this node's own children) — [`Walker::find_hit`]
/// uses it as the cache-match key against the current parent context.
/// `missing_peers` is the set of unmet peer requirements the original
/// walk surfaced — when a cache item carries a missing peer that the
/// current parent context *does* provide, the contexts are
/// incompatible and the item must be rejected.
struct PeersCacheItem {
    dep_path: DepPath,
    resolved_peers: HashMap<String, NodeId>,
    missing_peers: HashMap<String, MissingPeerInfo>,
}

/// One `parent → peer` edge whose peer target wasn't walked yet at the
/// time the parent's `graph_children` was built. Patched up by
/// [`Walker::patch_pending_peer_edges`] after the main walk completes.
struct PendingPeerEdge {
    parent_dep_path: DepPath,
    peer_alias: String,
    peer_node_id: NodeId,
}

/// Sentinel for "this node's subtree is still missing peer `X`". The
/// `range` + `optional` payload mirrors upstream's `MissingPeers` map
/// value; pacquet records them for upstream parity (and for the
/// `peersCache` lookup ported in a later slice) but the
/// issue-collection path uses [`PeerDependencyIssues::missing`]
/// directly, so neither field is read after construction yet.
#[derive(Debug, Clone)]
struct MissingPeerInfo {
    #[allow(dead_code, reason = "future peersCache validation")]
    range: String,
    #[allow(dead_code, reason = "future peersCache validation")]
    optional: bool,
}

/// Output of [`Walker::resolve_node`] — the per-node result the parent
/// folds into its own state.
struct NodeOutput {
    dep_path: DepPath,
    /// Peers that this node + its subtree resolved against ancestors.
    /// Excludes peers resolved against this node's own children (those
    /// are absorbed into the children's depPaths).
    external_resolved_peers: HashMap<String, NodeId>,
    missing_peers: HashMap<String, MissingPeerInfo>,
}

impl Walker<'_> {
    fn walk(mut self) -> ResolvePeersResult {
        let importer_parents = self.build_importer_parents();
        let parent_chain_names: Vec<String> = Vec::new();
        let mut direct_by_alias = BTreeMap::new();
        // Clone direct deps into an owned `Vec` so the recursion
        // below can mutate `self.tree` (realising lazy children)
        // without conflicting with this loop's borrow of
        // `self.tree.direct`.
        let direct: Vec<DirectDep> = self.tree.direct.clone();
        for dep in &direct {
            let output =
                self.resolve_node(dep.node_id.clone(), &importer_parents, &parent_chain_names, 0);
            direct_by_alias.insert(dep.alias.clone(), output.dep_path);
        }
        self.patch_pending_peer_edges();
        ResolvePeersResult {
            graph: self.graph,
            direct_dependencies_by_alias: direct_by_alias,
            peer_dependency_issues: self.issues,
        }
    }

    /// Fill in `graph_children` edges that were skipped during the main
    /// walk because the peer target's `DepPath` hadn't been computed
    /// yet. Each direct dep's subtree is fully walked by the time
    /// `walk()` drains this list, so every peer that was reachable
    /// from an ancestor's `ParentRefs` has a `DepPath` now. Peers that
    /// still don't resolve here came from a `parent_chain` outside the
    /// walked set — there's nothing to patch, and the absence already
    /// surfaced via [`PeerDependencyIssues::missing`].
    fn patch_pending_peer_edges(&mut self) {
        for edge in std::mem::take(&mut self.pending_peer_edges) {
            let Some(peer_dep_path) = self.node_dep_paths.get(&edge.peer_node_id).cloned() else {
                continue;
            };
            if let Some(node) = self.graph.get_mut(&edge.parent_dep_path) {
                // `entry().or_insert` rather than unconditional insert:
                // if a later walk of the same `dep_path` already
                // populated the edge (e.g. via the cycle path), we
                // don't want to overwrite a more specific entry.
                node.children.entry(edge.peer_alias).or_insert(peer_dep_path);
            }
        }
    }

    /// Build the seed [`ParentRefs`] from the importer's direct deps so
    /// a direct dep's peer requirements can be satisfied by a sibling
    /// direct dep. Mirrors upstream's `pkgsByName` initialisation at
    /// the entry of `resolvePeers`.
    ///
    /// `link:` direct deps whose target lives outside
    /// [`ResolvePeersOptions::lockfile_dir`] are seeded with a node id
    /// rewritten to `link:<rel-from-lockfile_dir-to-modules_dir>/<alias>`
    /// when [`ResolvePeersOptions::exclude_links_from_lockfile`] is on
    /// — keeping the peer-suffix segment stable across machines
    /// regardless of the absolute path of the external link. Mirrors
    /// upstream's
    /// [`target` rewrite in `index.ts`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/index.ts#L232-L244).
    fn build_importer_parents(&self) -> ParentRefs {
        self.build_importer_parents_from(&self.tree.direct)
    }

    /// Same as [`Self::build_importer_parents`] but seeds from an
    /// externally-supplied direct-deps slice — used by the multi-importer
    /// [`fn@resolve_peers_workspace`] where each importer's `direct`
    /// lives outside [`ResolvedTree`].
    fn build_importer_parents_from(&self, direct_deps: &[DirectDep]) -> ParentRefs {
        let mut refs = ParentRefs::new();
        for direct in direct_deps {
            let Some(tree_node) = self.tree.dependencies_tree.get(&direct.node_id) else {
                continue;
            };
            let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id) else {
                continue;
            };
            let parent_node_id = remap_link_node_id(&self.opts, &direct.alias, &pkg.result)
                .unwrap_or_else(|| direct.node_id.clone());
            insert_parent_ref(&mut refs, &direct.alias, parent_node_id, pkg, self.tree);
        }
        refs
    }

    #[allow(
        clippy::only_used_in_recursion,
        reason = "`depth` is kept for upstream parity with `currentDepth`"
    )]
    fn resolve_node(
        &mut self,
        node_id: NodeId,
        parent_parent_refs: &ParentRefs,
        parent_chain_names: &[String],
        depth: i32,
    ) -> NodeOutput {
        // `purePkgs` fast-path. When the subtree below this
        // `pkgIdWithPatchHash` resolved with zero external peers and
        // zero missing peers on a previous walk, AND this package
        // itself declares no `peerDependencies`, the `depPath` is the
        // bare `pkgIdWithPatchHash` regardless of parent context.
        // Skip recursion entirely. Mirrors upstream's
        // [`purePkgs` short-circuit](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L398-L406).
        //
        // The pkg lookup happens before the existing in-progress
        // gate because: a re-entry on this same `NodeId` while it's
        // still on the call stack is a cycle (handled below); a
        // re-entry on a `NodeId` that's done and pure is exactly what
        // this fast-path should catch. The unconditional clone
        // mirrors the post-`in_progress` clone the rest of the
        // function already does — peer resolution is single-threaded
        // and the clones are cheap.
        if self.tree.dependencies_tree.contains_key(&node_id) {
            let tree_node_depth = self.tree.dependencies_tree[&node_id].depth;
            let pkg_id = self.tree.dependencies_tree[&node_id].resolved_package_id.clone();
            // Workspace-link short-circuit: mirrors upstream's
            // [`if (node.depth === -1) return ...`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/deps-resolver/src/resolvePeers.ts#L396)
            // in `resolvePeersOfNode`. The linked package's depPath is
            // its `link:<rel-path>` id verbatim — no peer-graph suffix,
            // no graph entry. Peer matching for the linked package is
            // the linked importer's responsibility, not the parent's.
            if tree_node_depth == -1 {
                let dep_path = DepPath::from(pkg_id);
                self.node_dep_paths.insert(node_id.clone(), dep_path.clone());
                return NodeOutput {
                    dep_path,
                    external_resolved_peers: HashMap::new(),
                    missing_peers: HashMap::new(),
                };
            }
            let pkg_peer_dependencies_empty =
                self.tree.packages[&pkg_id].peer_dependencies.is_empty();
            if self.pure_pkgs.contains(&pkg_id) && pkg_peer_dependencies_empty {
                let dep_path = DepPath::from(pkg_id);
                self.node_dep_paths.insert(node_id.clone(), dep_path.clone());
                // Lower the existing graph entry's `depth` if this
                // occurrence reached the package shallower than the
                // previous walk(s). Mirrors the same `Math.min`
                // tie-break the non-fast path runs via
                // `self.graph.entry(...).and_modify(...)`; without it
                // a shallow revisit through `pure_pkgs` would leave
                // the entry's depth stuck at the first walk's value.
                if let Some(node) = self.graph.get_mut(&dep_path)
                    && node.depth > tree_node_depth
                {
                    node.depth = tree_node_depth;
                }
                return NodeOutput {
                    dep_path,
                    external_resolved_peers: HashMap::new(),
                    missing_peers: HashMap::new(),
                };
            }
        }

        if self.in_progress.contains(&node_id) {
            // Cycle: bottom out with the bare `pkgIdWithPatchHash` as
            // the depPath. The original visit (still on the stack) will
            // compute the real depPath and insert it into
            // `node_dep_paths`. Returning the bare id here ensures the
            // current ancestor's peer-suffix construction can use a
            // `name@version` PeerId — see [`build_peer_id`] for the
            // cycle handling.
            let tree_node = &self.tree.dependencies_tree[&node_id];
            let pkg = &self.tree.packages[&tree_node.resolved_package_id];
            return NodeOutput {
                dep_path: DepPath::from(pkg.id.clone()),
                external_resolved_peers: HashMap::new(),
                missing_peers: HashMap::new(),
            };
        }
        self.in_progress.insert(node_id.clone());

        // Realize children for this node if they're still Lazy. The
        // returned `children_map` is an owned clone; later iteration
        // over it doesn't hold a borrow on `self.tree`, so the
        // recursion below can mutate the tree (realising
        // grandchildren) freely.
        let children_map = self.realize_children(&node_id);
        let tree_node = self.tree.dependencies_tree[&node_id].clone();
        let pkg = self.tree.packages[&tree_node.resolved_package_id].clone();
        let (pkg_name, _pkg_version) = pkg_name_version(&pkg.result);

        // Build the ParentRefs map that descendants of this node see:
        // parent's view + this node's own children, restricted to
        // names that are declared as peers somewhere in the install.
        // The `occurrence` counter follows upstream's shadowing rule:
        // adding a same-name parent whose `(pkg_id, version)` doesn't
        // match the existing entry bumps `occurrence` and replaces
        // the entry. `parentPackagesMatch` keys off the counter to
        // reject cache hits when shadowing differs.
        let mut child_parent_refs = parent_parent_refs.clone();
        let child_depth = depth + 1;
        for (alias, child_node_id) in &children_map {
            let Some(child_tree) = self.tree.dependencies_tree.get(child_node_id) else { continue };
            let Some(child_pkg) = self.tree.packages.get(&child_tree.resolved_package_id) else {
                continue;
            };
            let (child_real_name, child_version) = pkg_name_version(&child_pkg.result);
            // Only peer-relevant aliases need to land in `parentRefs`.
            // Pnpm filters with `allPeerDepNames` to keep the propagated
            // map small.
            let alias_relevant = self.tree.all_peer_dep_names.contains(alias);
            let real_relevant = self.tree.all_peer_dep_names.contains(&child_real_name);
            if !alias_relevant && !real_relevant {
                continue;
            }
            let parent_ref = ParentRef {
                version: child_version,
                node_id: Some(child_node_id.clone()),
                alias: (alias != &child_real_name).then(|| alias.clone()),
                depth: child_depth,
                occurrence: 0,
            };
            if alias_relevant {
                bump_occurrence_on_shadow(&mut child_parent_refs, alias, &parent_ref);
            }
            if real_relevant && alias != &child_real_name {
                bump_occurrence_on_shadow(&mut child_parent_refs, &child_real_name, &parent_ref);
            }
        }

        // Record this node's parent context for the descendants'
        // [`peers_cache`] lookups. Mirrors upstream's
        // `parentPkgsOfNode.set(childNodeId, parentDepPaths)` in
        // `resolvePeersOfChildren`. We compute and store the snapshot
        // before recursing so a cycle re-entry on a child also has
        // access to its caller's parent context.
        let parent_dep_paths = self.parent_dep_paths_from_refs(&child_parent_refs);
        for child_node_id in children_map.values() {
            self.parent_pkgs_of_node.insert(child_node_id.clone(), parent_dep_paths.clone());
        }

        let mut child_chain_names: Vec<String> = parent_chain_names.to_vec();
        child_chain_names.push(pkg_name.clone());

        // `peersCache` lookup. When an earlier walk of this same
        // `pkgIdWithPatchHash` produced a result whose resolved-peer
        // map and missing-peer set are compatible with the current
        // parent peer context, reuse the cached `depPath` and external
        // peer/missing maps without recursing. Mirrors upstream's
        // [`findHit` call](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L441-L467).
        //
        // The cache lookup uses `child_parent_refs` (the augmented
        // view) because a node's own children count as parents for
        // its own descendants' peer resolution.
        if let Some(cached) = self.find_hit(&child_parent_refs, &pkg.id) {
            let dep_path = cached.dep_path.clone();
            let resolved = cached.resolved_peers.clone();
            let missing = cached.missing_peers.clone();
            // Re-emit the missing-peer issues against the current
            // parent chain so each occurrence of the package shows up
            // in the diagnostic, mirroring upstream's behaviour at the
            // cache-hit branch. Without this, the first walk's parent
            // chain would be the only one ever reported.
            for (peer_name, info) in &missing {
                self.issues.missing.entry(peer_name.clone()).or_default().push(MissingPeer {
                    wanted_range: info.range.clone(),
                    optional: info.optional,
                    parents: parents_from_chain(parent_chain_names, &pkg_name),
                });
            }
            self.node_dep_paths.insert(node_id.clone(), dep_path.clone());
            self.node_external_peers.insert(node_id.clone(), resolved.clone());
            self.node_missing_peers.insert(node_id.clone(), missing.clone());
            // Same depth tie-break as the `purePkgs` fast path and the
            // non-fast `entry(...).and_modify(...)` write below — a
            // shallower revisit through the cache must still lower the
            // existing graph entry's `depth`.
            if let Some(node) = self.graph.get_mut(&dep_path)
                && node.depth > tree_node.depth
            {
                node.depth = tree_node.depth;
            }
            self.in_progress.remove(&node_id);
            return NodeOutput {
                dep_path,
                external_resolved_peers: resolved,
                missing_peers: missing,
            };
        }

        // Recurse into children first (post-order). Collect each child's
        // depPath and the external peers / missing peers they propagate
        // up. Children's external peers may overlap with this node's
        // own children (e.g. a child resolved a peer to a sibling of
        // its parent — i.e., this node's child). Those are not external
        // *to this node* — they're internal here — so filter them out.
        let mut external_from_children: HashMap<String, NodeId> = HashMap::new();
        let mut missing_from_children: HashMap<String, MissingPeerInfo> = HashMap::new();
        let mut child_dep_paths: BTreeMap<String, DepPath> = BTreeMap::new();
        for (alias, child_node_id) in &children_map {
            let child_output = self.resolve_node(
                child_node_id.clone(),
                &child_parent_refs,
                &child_chain_names,
                depth + 1,
            );
            child_dep_paths.insert(alias.clone(), child_output.dep_path);
            for (peer_alias, peer_node_id) in child_output.external_resolved_peers {
                if children_map.values().any(|id| id == &peer_node_id) {
                    // Resolved against one of *this node's* children —
                    // not external from this node's perspective.
                    // Compare by NodeId (not alias) because `children`
                    // is keyed by install alias while `peer_alias` can
                    // be the resolved package's real name (the
                    // [`ParentRefs`] map indexes parents under both
                    // alias and real name when they differ). An
                    // alias-only check misses npm-aliased children.
                    continue;
                }
                external_from_children.insert(peer_alias, peer_node_id);
            }
            for (peer_alias, info) in child_output.missing_peers {
                missing_from_children.insert(peer_alias, info);
            }
        }

        // Resolve this node's own peer requirements against the
        // ParentRefs visible at this node — that is, the ones its
        // parent passed in, *not* the augmented child view (a node
        // does not satisfy its own peers from its own children).
        let mut own_resolved_peers: HashMap<String, NodeId> = HashMap::new();
        let mut own_missing_peers: HashMap<String, MissingPeerInfo> = HashMap::new();
        for (peer_name, peer_dep) in &pkg.peer_dependencies {
            self.resolve_one_peer(
                &pkg_name,
                peer_name,
                peer_dep,
                parent_parent_refs,
                &child_chain_names,
                &mut own_resolved_peers,
                &mut own_missing_peers,
            );
        }

        // Combine all resolved peers (this node's own + descendants').
        // Filter out the node's own name (a package doesn't peer-depend
        // on itself).
        let mut all_resolved_peers = external_from_children;
        for (peer_alias, peer_node_id) in &own_resolved_peers {
            all_resolved_peers.insert(peer_alias.clone(), peer_node_id.clone());
        }
        all_resolved_peers.remove(&pkg_name);

        // Same for missing peers (children + own).
        let mut all_missing_peers = missing_from_children;
        for (peer_alias, info) in &own_missing_peers {
            all_missing_peers.insert(peer_alias.clone(), info.clone());
        }

        // Construct the depPath. Empty resolved-peers ⇒ pure node:
        // depPath = pkgIdWithPatchHash.
        let dep_path = if all_resolved_peers.is_empty() {
            DepPath::from(pkg.id.clone())
        } else {
            let mut peer_ids: Vec<PeerId> = all_resolved_peers
                .iter()
                .map(|(peer_alias, peer_node_id)| self.build_peer_id(peer_alias, peer_node_id))
                .collect();
            // Sorting happens inside `create_peer_dep_graph_hash`, but
            // we deduplicate by stringified form here to mirror
            // upstream's `Map<alias, NodeId>` semantics (each peer
            // contributes at most once).
            peer_ids.sort_by_key(PeerId::as_segment);
            peer_ids.dedup_by_key(|p| p.as_segment());
            let suffix = create_peer_dep_graph_hash(&peer_ids, self.opts.peers_suffix_max_length);
            DepPath::from(format!("{}{}", pkg.id, suffix))
        };

        // Register the depPath ↔ NodeId mapping and per-node
        // propagated state before inserting into the graph (so any
        // cycle the graph insert hits via `child_dep_paths` can find
        // this node's depPath).
        self.node_dep_paths.insert(node_id.clone(), dep_path.clone());
        self.node_external_peers.insert(node_id.clone(), all_resolved_peers.clone());
        self.node_missing_peers.insert(node_id.clone(), all_missing_peers.clone());

        // The children's depPath edges become this node's graph children.
        // Resolved peers become extra edges, aliased by peer name. If a
        // peer's depPath isn't known yet — typically a later sibling
        // direct dep — defer the edge to the post-walk patch pass; the
        // install layer drives off `graph_children`, so skipping the
        // edge entirely would leave the peer un-symlinked in the
        // parent's slot.
        let mut graph_children = child_dep_paths.clone();
        for (peer_alias, peer_node_id) in &all_resolved_peers {
            if let Some(peer_dep_path) = self.node_dep_paths.get(peer_node_id) {
                graph_children.insert(peer_alias.clone(), peer_dep_path.clone());
            } else if let Some(link_dep_path) = link_node_id_as_dep_path(peer_node_id) {
                // Mirrors upstream's
                // [`pathsByNodeId.get(childNodeId) ?? (childNodeId as unknown as DepPath)`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/resolvePeers.ts#L164)
                // fallback in `resolveChildren`. `topParents` linked-dep
                // NodeIds never enter the tree, so `node_dep_paths` is
                // empty for them; the `link:<rel>` NodeId is itself a
                // valid DepPath, so the snapshot's child edge can use
                // it verbatim.
                graph_children.insert(peer_alias.clone(), link_dep_path);
            } else {
                self.pending_peer_edges.push(PendingPeerEdge {
                    parent_dep_path: dep_path.clone(),
                    peer_alias: peer_alias.clone(),
                    peer_node_id: peer_node_id.clone(),
                });
            }
        }

        // Compute transitive peer set: peers visible in this subtree
        // that are NOT declared in this package's own peerDependencies.
        let mut transitive_peer_dependencies: HashSet<String> = HashSet::new();
        for peer_alias in all_resolved_peers.keys() {
            if !pkg.peer_dependencies.contains_key(peer_alias) {
                transitive_peer_dependencies.insert(peer_alias.clone());
            }
        }
        for peer_alias in all_missing_peers.keys() {
            if !pkg.peer_dependencies.contains_key(peer_alias) {
                transitive_peer_dependencies.insert(peer_alias.clone());
            }
        }

        let is_pure = all_resolved_peers.is_empty() && all_missing_peers.is_empty();

        // Record this walk's outcome in the per-`pkgIdWithPatchHash`
        // caches. Pure subtrees go in [`Self::pure_pkgs`] for the
        // fast-path early return at the top of [`resolve_node`];
        // non-pure subtrees push a [`PeersCacheItem`] so a future
        // visit with a compatible parent context can short-circuit
        // via [`Self::find_hit`]. Mirrors upstream's
        // [post-walk cache-population block](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L507-L522).
        if is_pure {
            self.pure_pkgs.insert(pkg.id.clone());
        } else {
            self.peers_cache.entry(pkg.id.clone()).or_default().push(PeersCacheItem {
                dep_path: dep_path.clone(),
                resolved_peers: all_resolved_peers.clone(),
                missing_peers: all_missing_peers.clone(),
            });
        }

        // Multiple visits with the same depPath collapse onto the same
        // graph entry. Upstream takes the entry with the smallest
        // `depth` when there's a conflict; pacquet ports the same
        // tie-break so install order matches.
        self.graph
            .entry(dep_path.clone())
            .and_modify(|node| {
                if node.depth > tree_node.depth {
                    node.depth = tree_node.depth;
                }
            })
            .or_insert(DependenciesGraphNode {
                dep_path: dep_path.clone(),
                resolved_package_id: pkg.id.clone(),
                resolve_result: Arc::clone(&pkg.result),
                children: graph_children,
                peer_dependencies: pkg.peer_dependencies.clone(),
                transitive_peer_dependencies,
                resolved_peer_names: all_resolved_peers.keys().cloned().collect(),
                depth: tree_node.depth,
                installable: tree_node.installable,
                is_pure,
                optional: pkg.optional,
            });

        self.in_progress.remove(&node_id);

        // External resolved peers reported up: this node's collected
        // peers minus any that map to this node's own children
        // (already covered by the children's depPaths). Filter by
        // NodeId rather than alias for the same reason as the
        // `external_from_children` filter above — `children` is keyed
        // by install alias while peers may be keyed by the resolved
        // package's real name.
        let own_child_ids: HashSet<&NodeId> = children_map.values().collect();
        let external_to_report: HashMap<String, NodeId> = all_resolved_peers
            .into_iter()
            .filter(|(_, peer_node_id)| !own_child_ids.contains(peer_node_id))
            .collect();

        NodeOutput {
            dep_path,
            external_resolved_peers: external_to_report,
            missing_peers: all_missing_peers,
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "splitting these into a struct would only obscure the call site"
    )]
    fn resolve_one_peer(
        &mut self,
        pkg_name: &str,
        peer_name: &str,
        peer_dep: &PeerDep,
        parent_refs: &ParentRefs,
        chain: &[String],
        resolved: &mut HashMap<String, NodeId>,
        missing: &mut HashMap<String, MissingPeerInfo>,
    ) {
        let raw_range = peer_dep.version.as_str();
        let range_for_match = raw_range.strip_prefix("workspace:").unwrap_or(raw_range);
        let optional = peer_dep.optional;

        match parent_refs.get(peer_name) {
            None => {
                missing.insert(
                    peer_name.to_string(),
                    MissingPeerInfo { range: range_for_match.to_string(), optional },
                );
                self.issues.missing.entry(peer_name.to_string()).or_default().push(MissingPeer {
                    wanted_range: range_for_match.to_string(),
                    optional,
                    parents: parents_from_chain(chain, pkg_name),
                });
            }
            Some(parent) => {
                if !satisfies_with_prereleases(&parent.version, range_for_match) {
                    self.issues.bad.entry(peer_name.to_string()).or_default().push(
                        PeerDependencyIssue {
                            wanted_range: range_for_match.to_string(),
                            found_version: parent.version.clone(),
                            optional,
                            parents: parents_from_chain(chain, pkg_name),
                            resolved_from: Vec::new(),
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
    /// Precedence (mirrors upstream's
    /// [`peerNodeIdToPeerId`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/resolvePeers.ts#L976-L998)):
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
    ///    `(foo@1.0.0(bar@2.0.0))` → `(foo@1.0.0)`. Mirrors upstream's
    ///    [`dedupePeers` branch](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolvePeers.ts#L990-L997).
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
            let (name, version) = pkg_name_version(&pkg.result);
            return PeerId::Pair { name, version };
        }
        if let Some(dep_path) = self.node_dep_paths.get(peer_node_id) {
            return PeerId::DepPath(dep_path.clone());
        }
        let tree_node = &self.tree.dependencies_tree[peer_node_id];
        let pkg = &self.tree.packages[&tree_node.resolved_package_id];
        let (name, version) = pkg_name_version(&pkg.result);
        PeerId::Pair { name, version }
    }

    /// Realize the `(alias → NodeId)` children of `node_id` if it's
    /// currently a [`TreeChildren::Lazy`] entry; return the realized
    /// map (cloned for the caller). On a [`TreeChildren::Realized`]
    /// entry, just clones and returns. Mirrors upstream's
    /// [`buildTree` thunk-on-demand expansion](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencyTree.ts#L371-L401):
    ///
    /// 1. Walk [`ResolvedTree::children_by_id`] for this node's
    ///    package id.
    /// 2. Skip any child whose pkg id appears in `parent_ids` — that
    ///    edge would form a cycle, matching upstream's
    ///    [`parentIdsContainSequence`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencyTree.ts#L378)
    ///    gate.
    /// 3. For each surviving child, allocate a per-occurrence
    ///    `NodeId` (leaves reuse the deterministic `NodeId::leaf`
    ///    for the leaf-collapse the eager walker does too) and
    ///    insert a fresh `dependencies_tree` entry with another
    ///    `Lazy` children variant that carries `parent_ids +
    ///    [self_pkg_id]` for cycle break on its own descendants.
    /// 4. Flip this node's `children` field to `Realized` so a
    ///    later visitor reuses the map.
    fn realize_children(&mut self, node_id: &NodeId) -> BTreeMap<String, NodeId> {
        // Snapshot the bits we need; we'll mutate `self.tree` below
        // and can't hold a borrow on the entry across the mutation.
        let (parent_ids, pkg_id, depth) = {
            let node = &self.tree.dependencies_tree[node_id];
            match &node.children {
                TreeChildren::Realized(map) => return map.clone(),
                TreeChildren::Lazy { parent_ids } => {
                    (Arc::clone(parent_ids), node.resolved_package_id.clone(), node.depth)
                }
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
        for edge in children_spec.iter() {
            if parent_ids.iter().any(|ancestor_id| ancestor_id == &edge.pkg_id) {
                continue;
            }
            // Reuse the first walk's classification (persisted on
            // `ResolvedPackage::is_leaf` by `pkg_is_leaf`). Defaults
            // to non-leaf when the package isn't in `packages` — same
            // shape as the eager walker's `manifest == None` arm,
            // and `NodeId::next()` keeps occurrences distinct so a
            // later visit can still observe per-call-site state.
            let is_leaf = self.tree.packages.get(&edge.pkg_id).is_some_and(|pkg| pkg.is_leaf);
            let child_node_id = if is_leaf { NodeId::leaf(&edge.pkg_id) } else { NodeId::next() };
            let child_parent_ids = {
                let mut next_ids = (*parent_ids).clone();
                next_ids.push(edge.pkg_id.clone());
                Arc::new(next_ids)
            };
            self.tree
                .dependencies_tree
                .entry(child_node_id.clone())
                .and_modify(|n| {
                    if n.depth > child_depth {
                        n.depth = child_depth;
                    }
                })
                .or_insert_with(|| DependenciesTreeNode {
                    resolved_package_id: edge.pkg_id.clone(),
                    children: TreeChildren::Lazy { parent_ids: child_parent_ids },
                    depth: child_depth,
                    installable: true,
                });
            realized.insert(edge.alias.clone(), child_node_id);
        }
        // Replace this node's `Lazy` with `Realized` so future
        // visitors reuse the work.
        if let Some(node) = self.tree.dependencies_tree.get_mut(node_id) {
            node.children = TreeChildren::Realized(realized.clone());
        }
        realized
    }

    /// Build the `(peer_name → ParentPkgInfo)` snapshot that gets
    /// stored on [`Self::parent_pkgs_of_node`] for each child the
    /// caller is about to descend into. Mirrors upstream's
    /// [`parentDepPaths` construction inside `resolvePeersOfChildren`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L817-L829).
    ///
    /// `link:` parents (upstream's `nodeId.startsWith('link:')`
    /// branch) don't have a real tree entry; pacquet's `ParentRef`
    /// keeps the `NodeId` but the tree-lookup falls back to a pure
    /// `version` comparison the same way upstream does.
    fn parent_dep_paths_from_refs(
        &self,
        parent_refs: &ParentRefs,
    ) -> HashMap<String, ParentPkgInfo> {
        let mut out = HashMap::new();
        for (name, parent_ref) in parent_refs {
            if !self.tree.all_peer_dep_names.contains(name) {
                continue;
            }
            let pkg_id = parent_ref
                .node_id
                .as_ref()
                .and_then(|nid| self.tree.dependencies_tree.get(nid))
                .map(|tn| tn.resolved_package_id.clone());
            out.insert(
                name.clone(),
                ParentPkgInfo {
                    pkg_id,
                    version: Some(parent_ref.version.clone()),
                    depth: parent_ref.depth,
                    occurrence: parent_ref.occurrence,
                },
            );
        }
        out
    }

    /// Look up [`Self::peers_cache`] for a cached resolution of
    /// `pkg_id` whose parent peer context is compatible with the
    /// current `parent_refs`. Mirrors upstream's
    /// [`findHit`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L660-L699).
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
    fn find_hit(&self, parent_refs: &ParentRefs, pkg_id: &str) -> Option<&PeersCacheItem> {
        let cache_items = self.peers_cache.get(pkg_id)?;
        cache_items.iter().find(|item| {
            for (name, cached_node_id) in &item.resolved_peers {
                let Some(current_ref) = parent_refs.get(name) else { return false };
                let Some(current_node_id) = current_ref.node_id.as_ref() else { return false };
                if current_node_id == cached_node_id {
                    continue;
                }
                // Same `DepPath` reached via a different `NodeId` — that's a
                // legitimate match (e.g. via the leaf-NodeId collapse).
                if let (Some(cached_dp), Some(current_dp)) = (
                    self.node_dep_paths.get(cached_node_id),
                    self.node_dep_paths.get(current_node_id),
                ) && cached_dp == current_dp
                {
                    continue;
                }
                // Different `NodeId`s — both must at least point to
                // packages with the same `pkgIdWithPatchHash`, and the
                // deep `parent_packages_match` check (or the
                // `purePkgs` shortcut) has to agree.
                let Some(cached_tree_node) = self.tree.dependencies_tree.get(cached_node_id) else {
                    return false;
                };
                let Some(current_tree_node) = self.tree.dependencies_tree.get(current_node_id)
                else {
                    return false;
                };
                let parent_pkg_id = &current_tree_node.resolved_package_id;
                if parent_pkg_id != &cached_tree_node.resolved_package_id {
                    return false;
                }
                if !self.pure_pkgs.contains(parent_pkg_id)
                    && !self.parent_packages_match(cached_node_id, current_node_id)
                {
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

    /// Compare two `NodeId`s' recorded parent peer contexts. Mirrors
    /// upstream's
    /// [`parentPackagesMatch`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L701-L731):
    /// both nodes' contexts must have the same set of peer-relevant
    /// names, every name must resolve to the same version or
    /// `pkgIdWithPatchHash`, and — when a peer is shadowed (an
    /// `occurrence > 0` somewhere on either side) — the contexts
    /// must additionally agree on depth/`purePkgs` to compensate
    /// for the loss of single-occurrence guarantees upstream relies
    /// on for the shallow-equality path.
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
        for (name, cached_info) in cached_parents {
            let Some(current_info) = current_parents.get(name) else { return false };
            // Version-only match: when both sides recorded a
            // `version`, pure version equality is enough (covers
            // `link:` parents whose nodeIds don't index into the
            // dependencies tree).
            if cached_info.version.is_some() && current_info.version.is_some() {
                if cached_info.version == current_info.version {
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
                || self.pure_pkgs.contains(cached_pkg_id))
            {
                return false;
            }
        }
        true
    }
}

/// Whether every entry in `parents` has `occurrence == 0`. Mirrors
/// upstream's
/// [`parentPkgsHaveSingleOccurrence`](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L733-L735).
fn parent_pkgs_have_single_occurrence(parents: &HashMap<String, ParentPkgInfo>) -> bool {
    parents.values().all(|info| info.occurrence == 0)
}

/// Reproduce upstream's `Map<NodeId, ParentRef>` dual-keying: each
/// parent is recorded by its install alias *and* its real name when
/// the two differ. Mirrors
/// [`updateParentRefs`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L1035-L1044).
///
/// `parent_node_id` is the [`NodeId`] the parent should appear under
/// in the [`ParentRefs`] map. For most parents this is just
/// `direct.node_id`, but `link:` parents may carry the remapped
/// node id produced by [`remap_link_node_id`] when
/// `excludeLinksFromLockfile` is on.
fn insert_parent_ref(
    refs: &mut ParentRefs,
    direct_alias: &str,
    parent_node_id: NodeId,
    pkg: &ResolvedPackage,
    tree: &ResolvedTree,
) {
    let (real_name, version) = pkg_name_version(&pkg.result);
    let alias_relevant = tree.all_peer_dep_names.contains(direct_alias);
    let real_relevant = tree.all_peer_dep_names.contains(&real_name);
    if !alias_relevant && !real_relevant {
        return;
    }
    let parent_ref = ParentRef {
        version,
        node_id: Some(parent_node_id),
        alias: (direct_alias != real_name).then(|| direct_alias.to_string()),
        depth: 0,
        occurrence: 0,
    };
    if alias_relevant {
        refs.insert(direct_alias.to_string(), parent_ref.clone());
    }
    if real_relevant && direct_alias != real_name {
        refs.insert(real_name, parent_ref);
    }
}

/// Compute the `link:` [`NodeId`] under which a workspace-link parent
/// should appear in [`ParentRefs`] when
/// [`ResolvePeersOptions::exclude_links_from_lockfile`] is on.
///
/// Returns `None` when:
///
/// - the dep isn't a `link:` directory resolution;
/// - the setting is off or the lockfile / modules dirs are missing;
/// - the link target lives under `lockfile_dir` (workspace-internal
///   link — already stable across machines, no remap needed).
///
/// On `Some`, the remap encodes `<modules_dir>/<alias>` as a path
/// relative to `lockfile_dir`, prefixed with `link:`. Mirrors
/// upstream's
/// [`target` rewrite in `index.ts`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/index.ts#L232-L244)
/// and the surrounding
/// [`createNodeIdForLinkedLocalPkg`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/resolveDependencies.ts#L976-L978)
/// helper.
fn remap_link_node_id(
    opts: &ResolvePeersOptions,
    alias: &str,
    result: &ResolveResult,
) -> Option<NodeId> {
    if !opts.exclude_links_from_lockfile {
        return None;
    }
    let lockfile_dir = opts.lockfile_dir.as_ref()?;
    let modules_dir = opts.modules_dir.as_ref()?;
    let directory = match &result.resolution {
        pacquet_lockfile::LockfileResolution::Directory(dir) => &dir.directory,
        _ => return None,
    };
    let link_target = std::path::Path::new(directory);
    if pacquet_fs::is_subdir(lockfile_dir, link_target) {
        return None;
    }
    let target = modules_dir.join(alias);
    let rel = pathdiff::diff_paths(&target, lockfile_dir)?;
    let rel = rel.display().to_string().replace('\\', "/");
    Some(NodeId::leaf(&format!("link:{rel}")))
}

/// Insert `parent_ref` under `name` in `refs`, bumping `occurrence`
/// when shadowing an existing entry whose `(pkg_id, version)` differs.
/// Mirrors upstream's
/// [`addParentPkg` shadowing arm](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolvePeers.ts#L430-L439):
/// when a same-name parent gets added at a deeper depth and doesn't
/// match the existing record, the new entry replaces with a higher
/// `occurrence` so [`Walker::parent_packages_match`] can flag the
/// shadowing.
fn bump_occurrence_on_shadow(refs: &mut ParentRefs, name: &str, parent_ref: &ParentRef) {
    let next = match refs.get(name) {
        Some(existing) if existing.node_id == parent_ref.node_id => {
            // Identical entry — keep the existing occurrence value.
            return;
        }
        Some(existing) => ParentRef { occurrence: existing.occurrence + 1, ..parent_ref.clone() },
        None => parent_ref.clone(),
    };
    refs.insert(name.to_string(), next);
}

/// Build the `parents` chain attached to a peer issue. Upstream uses
/// the `ResolvedPackage` of each parent; pacquet's slice records just
/// `name` and `version`, which is what the renderer downstream
/// consumes.
fn parents_from_chain(chain_names: &[String], _pkg_name: &str) -> Vec<ParentPackageRef> {
    // The chain pacquet tracks today is name-only — populating
    // `version` would need a parallel `Vec<String>` of versions or a
    // re-lookup against the tree. The issue-renderer consumes the
    // names primarily; expanding to versions is a follow-up.
    chain_names
        .iter()
        .map(|name| ParentPackageRef { name: name.clone(), version: String::new() })
        .collect()
}

/// Range-satisfaction check that tolerates prereleases the way pnpm's
/// `@yarnpkg/core/semverUtils.satisfiesWithPrereleases` does — falls
/// back to a literal-equality check when the range can't be parsed,
/// which lets non-semver peer ranges (`*`, git refs, etc.) still
/// match. Mirrors upstream's
/// [`semverUtils.satisfiesWithPrereleases`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L922)
/// call site.
///
/// **Prerelease tolerance.** `node-semver`'s [`Range::satisfies`]
/// rejects prerelease versions against non-prerelease comparators —
/// `18.0.0-rc.1` against `^18.0.0` returns `false`. Yarn's
/// `satisfiesWithPrereleases` (which pnpm imports here) explicitly
/// allows that pairing. We approximate it by retrying with the
/// prerelease tag stripped: if `version` is a prerelease and the
/// straight check fails, see whether the base `MAJOR.MINOR.PATCH`
/// satisfies the range. That covers the cases pnpm cares about for
/// peer-range matching (a candidate with a `-rc.N` / `-alpha.N` suffix
/// satisfying a regular `^X.Y` peer requirement) without pulling in
/// Yarn's full per-comparator algorithm.
fn satisfies_with_prereleases(version: &str, range: &str) -> bool {
    if range == "*" {
        return true;
    }
    let Ok(parsed_version) = Version::parse(version) else {
        return version == range;
    };
    let Ok(parsed_range) = Range::parse(range) else {
        return version == range;
    };
    if parsed_version.satisfies(&parsed_range) {
        return true;
    }
    if !parsed_version.is_prerelease() {
        return false;
    }
    let base = Version {
        major: parsed_version.major,
        minor: parsed_version.minor,
        patch: parsed_version.patch,
        pre_release: Vec::new(),
        build: Vec::new(),
    };
    base.satisfies(&parsed_range)
}

#[cfg(test)]
mod tests;
