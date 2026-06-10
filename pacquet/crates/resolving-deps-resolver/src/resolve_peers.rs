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
    dedupe_peer_dependents::dedupe_peer_dependents,
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
        node_records: HashMap::new(),
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
    dedupe_peer_dependents_enabled: bool,
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
        node_records: HashMap::new(),
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
        let parent_node_ids: Vec<NodeId> = Vec::new();
        let parent_pkg_ids_chain: Vec<String> = Vec::new();
        let importer_parent_dep_paths = walker.parent_dep_paths_from_refs(&importer_parents);
        for dep in &importer.direct {
            walker
                .parent_pkgs_of_node
                .insert(dep.node_id.clone(), importer_parent_dep_paths.clone());
        }
        for dep in &importer.direct {
            walker.resolve_node(
                dep.node_id.clone(),
                &importer_parents,
                &parent_chain_names,
                &parent_node_ids,
                &parent_pkg_ids_chain,
            );
        }
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
    for importer in importers {
        let direct_by_alias: BTreeMap<String, DepPath> = importer
            .direct
            .iter()
            .map(|dep| {
                (dep.alias.clone(), walker.final_dep_path_of(&dep.node_id, &final_dep_paths))
            })
            .collect();
        direct_dependencies_by_importer.insert(importer.id.clone(), direct_by_alias);
    }
    let mut graph = walker.build_final_graph(&final_dep_paths);

    if dedupe_injected_deps_enabled {
        dedupe_injected_deps(
            &mut graph,
            &mut direct_dependencies_by_importer,
            &importer_root_dirs,
            lockfile_dir,
        );
    }

    // Runs after the injected-deps dedupe (matching upstream's ordering)
    // so a `file:`→`link:` rewrite is already reflected in the graph
    // before peer-dependent variants collapse.
    if dedupe_peer_dependents_enabled {
        dedupe_peer_dependents(&mut graph, &mut direct_dependencies_by_importer);
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
    /// Graph edges whose target `NodeId` had no `DepPath` yet at the
    /// time we built the parent's `graph_children` map — typically
    /// because the target is a later sibling direct dep that the walker
    /// hasn't reached yet. `walk()` drains this list once every direct
    /// dep is walked and patches the recorded entries with the now-known
    /// `DepPath`. Without this post-pass the install layer
    /// would walk the parent's `children` map and find no symlink edge
    /// for the child, leaving the package without it in its slot.
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
    /// Per-`NodeId` snapshot captured at graph-insert time, consumed by
    /// the post-walk [`Walker::build_final_dep_paths`] /
    /// [`Walker::build_final_graph`] pass. See [`NodeRecord`].
    node_records: HashMap<NodeId, NodeRecord>,
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

/// One `parent → child` edge whose target wasn't walked yet at the
/// time the parent's `graph_children` was built. Patched up by
/// [`Walker::patch_pending_peer_edges`] after the main walk completes.
struct PendingPeerEdge {
    parent_dep_path: DepPath,
    child_alias: String,
    child_node_id: NodeId,
}

/// Per-`NodeId` data captured during the walk so the post-walk
/// [`Walker::build_final_dep_paths`] pass can recompute each node's
/// depPath with its resolved peers' *full* suffixes — matching pnpm's
/// deferred [`calculateDepPath`](https://github.com/pnpm/pnpm/blob/894ea6af2c/installing/deps-resolver/src/resolvePeers.ts#L629),
/// which awaits each pending peer's depPath and collapses to
/// `name@version` only for genuinely detected cycles.
///
/// The walk itself is left untouched: it still computes the provisional
/// depPaths that [`Walker::find_hit`] reads, so peer-resolution and
/// cache decisions are byte-for-byte identical. Only the rendered
/// depPaths change, which is why a node whose suffix was previously
/// collapsed by the cycle fallback now splits into its own graph entry.
struct NodeRecord {
    /// `alias → child/peer NodeId` edges, in the same shape the inline
    /// `graph_children` map carries (children overlaid with resolved-peer
    /// edges) but holding `NodeIds`, so the rebuild can map each edge to
    /// its final depPath.
    edges: BTreeMap<String, NodeId>,
    transitive_peer_dependencies: HashSet<String>,
    depth: i32,
    installable: bool,
    is_pure: bool,
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
        let parent_node_ids: Vec<NodeId> = Vec::new();
        let parent_pkg_ids_chain: Vec<String> = Vec::new();
        let mut direct_by_alias = BTreeMap::new();
        // Clone direct deps into an owned `Vec` so the recursion
        // below can mutate `self.tree` (realising lazy children)
        // without conflicting with this loop's borrow of
        // `self.tree.direct`.
        let direct: Vec<DirectDep> = self.tree.direct.clone();
        let importer_parent_dep_paths = self.parent_dep_paths_from_refs(&importer_parents);
        for dep in &direct {
            self.parent_pkgs_of_node.insert(dep.node_id.clone(), importer_parent_dep_paths.clone());
        }
        for dep in &direct {
            self.resolve_node(
                dep.node_id.clone(),
                &importer_parents,
                &parent_chain_names,
                &parent_node_ids,
                &parent_pkg_ids_chain,
            );
        }
        self.patch_pending_peer_edges();
        // Recompute depPaths so each resolved peer carries its full
        // suffix (the cycle fallback during the walk collapses peers
        // that are walk-ancestors), then rebuild the graph from the
        // per-node records keyed by the corrected depPaths.
        let final_dep_paths = self.build_final_dep_paths();
        for dep in &direct {
            direct_by_alias
                .insert(dep.alias.clone(), self.final_dep_path_of(&dep.node_id, &final_dep_paths));
        }
        let graph = self.build_final_graph(&final_dep_paths);
        ResolvePeersResult {
            graph,
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
            let Some(child_dep_path) = self.node_dep_paths.get(&edge.child_node_id).cloned() else {
                continue;
            };
            if let Some(node) = self.graph.get_mut(&edge.parent_dep_path) {
                // `entry().or_insert` rather than unconditional insert:
                // if a later walk of the same `dep_path` already
                // populated the edge (e.g. via the cycle path), we
                // don't want to overwrite a more specific entry.
                node.children.entry(edge.child_alias).or_insert(child_dep_path);
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
            let (real_name, _) = pkg_name_version(&pkg.result);
            if !self.tree.all_peer_dep_names.contains(&direct.alias)
                && !self.tree.all_peer_dep_names.contains(&real_name)
            {
                continue;
            }
            let parent_node_id = remap_link_node_id(&self.opts, &direct.alias, &pkg.result)
                .unwrap_or_else(|| direct.node_id.clone());
            insert_parent_ref(&mut refs, &direct.alias, parent_node_id, pkg, tree_node.depth);
        }
        refs
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "resolve_node is the recursive walk's core; threading &NodeId would ripple a borrow through every recursive call site for negligible gain on this small enum"
    )]
    fn resolve_node(
        &mut self,
        node_id: NodeId,
        parent_parent_refs: &ParentRefs,
        parent_chain_names: &[String],
        parent_node_ids: &[NodeId],
        parent_pkg_ids_chain: &[String],
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
            let bare_dep_path = DepPath::from(pkg_id.clone());
            if self.pure_pkgs.contains(&pkg_id)
                && pkg_peer_dependencies_empty
                && self.graph.get(&bare_dep_path).is_some_and(|node| node.depth <= tree_node_depth)
            {
                let dep_path = bare_dep_path;
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

        let mut current_parent_node_ids = parent_node_ids.to_vec();
        current_parent_node_ids.push(node_id.clone());
        let mut child_parent_pkg_ids_chain = parent_pkg_ids_chain.to_vec();
        if !child_parent_pkg_ids_chain.contains(&pkg.id) {
            child_parent_pkg_ids_chain.push(pkg.id.clone());
        }

        // Build the ParentRefs map that descendants of this node see:
        // parent's view + this node's own peer-relevant children.
        let mut child_parent_refs = parent_parent_refs.clone();
        let mut new_parent_refs = ParentRefs::new();
        for (alias, child_node_id) in &children_map {
            let alias_is_peer_relevant = self.tree.all_peer_dep_names.contains(alias);
            let Some(child_tree) = self.tree.dependencies_tree.get(child_node_id) else { continue };
            let Some(child_pkg) = self.tree.packages.get(&child_tree.resolved_package_id) else {
                continue;
            };
            if !alias_is_peer_relevant {
                let (child_name, _) = pkg_name_version(&child_pkg.result);
                if !self.tree.all_peer_dep_names.contains(&child_name) {
                    continue;
                }
            }
            insert_parent_ref(
                &mut new_parent_refs,
                alias,
                child_node_id.clone(),
                child_pkg,
                child_tree.depth,
            );
        }
        let mut child_parent_refs_with_new = child_parent_refs.clone();
        child_parent_refs_with_new.extend(new_parent_refs.clone());
        for (name, mut new_parent_ref) in new_parent_refs {
            if let Some(existing) = child_parent_refs.get(&name) {
                if !self.parent_refs_match(existing, &new_parent_ref)
                    || self.inherited_parent_pkg_breaks_peer_diamond(
                        &child_parent_refs_with_new,
                        existing,
                        &new_parent_ref,
                        &children_map,
                    )
                {
                    new_parent_ref.occurrence = existing.occurrence + 1;
                    child_parent_refs.insert(name, new_parent_ref);
                }
            } else {
                child_parent_refs.insert(name, new_parent_ref);
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
                &current_parent_node_ids,
                &child_parent_pkg_ids_chain,
            );
            child_dep_paths.insert(alias.clone(), child_output.dep_path);
            for (peer_alias, peer_node_id) in child_output.external_resolved_peers {
                if children_map.contains_key(&peer_alias) {
                    continue;
                }
                external_from_children.insert(peer_alias, peer_node_id);
            }
            for (peer_alias, info) in child_output.missing_peers {
                missing_from_children.insert(peer_alias, info);
            }
        }

        // Resolve this node's own peer requirements against the augmented
        // ParentRefs visible at this node, including peer-relevant children.
        let mut own_resolved_peers: HashMap<String, NodeId> = HashMap::new();
        let mut own_missing_peers: HashMap<String, MissingPeerInfo> = HashMap::new();
        for (peer_name, peer_dep) in &pkg.peer_dependencies {
            self.resolve_one_peer(
                &pkg_name,
                peer_name,
                peer_dep,
                &child_parent_refs,
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
            let peer_ids: Vec<PeerId> = all_resolved_peers
                .iter()
                .map(|(peer_alias, peer_node_id)| self.build_peer_id(peer_alias, peer_node_id))
                .collect();
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
        let mut graph_children = BTreeMap::new();
        for (alias, child_node_id) in
            self.previously_resolved_children(parent_node_ids, parent_pkg_ids_chain, &pkg.id)
        {
            self.add_graph_child_or_pending(&mut graph_children, &dep_path, alias, child_node_id);
        }
        for (alias, child_dep_path) in child_dep_paths {
            graph_children.insert(alias, child_dep_path);
        }
        for (peer_alias, peer_node_id) in &all_resolved_peers {
            self.add_graph_child_or_pending(
                &mut graph_children,
                &dep_path,
                peer_alias.clone(),
                peer_node_id.clone(),
            );
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

        // Capture this node's NodeId-level edges + metadata for the
        // post-walk [`Walker::build_final_dep_paths`] rebuild. Edges are
        // the node's regular children overlaid with its *own* resolved
        // peers — mirroring upstream's
        // [`childrenNodeIds: { ...children, ...resolvedPeers }`](https://github.com/pnpm/pnpm/blob/894ea6af2c/installing/deps-resolver/src/resolvePeers.ts#L700-L705),
        // where `resolvedPeers` is this node's own peer resolution, not
        // the descendants' peers bubbled up for the suffix. A peer a
        // descendant resolved (e.g. `debug`'s optional `supports-color`)
        // is symlinked at the descendant that declares it, so it must
        // not appear in this node's dependencies. Carries NodeIds so the
        // rebuild can resolve each to its corrected final depPath.
        let mut record_edges =
            self.previously_resolved_children(parent_node_ids, parent_pkg_ids_chain, &pkg.id);
        record_edges.extend(children_map.clone());
        for (peer_alias, peer_node_id) in &own_resolved_peers {
            record_edges.insert(peer_alias.clone(), peer_node_id.clone());
        }
        self.node_records.insert(
            node_id.clone(),
            NodeRecord {
                edges: record_edges,
                transitive_peer_dependencies: transitive_peer_dependencies.clone(),
                depth: tree_node.depth,
                installable: tree_node.installable,
                is_pure,
            },
        );

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

        let external_to_report: HashMap<String, NodeId> = all_resolved_peers
            .into_iter()
            .filter(|(peer_alias, _)| !children_map.contains_key(peer_alias))
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

    /// Resolve `node_id` to the depPath the rebuilt graph should key /
    /// reference it by. Prefers the corrected `final_dep_paths` entry,
    /// falls back to the provisional `node_dep_paths` value (correct for
    /// peerless nodes, whose depPath is just their `pkgIdWithPatchHash`),
    /// then to a bare `link:` id, then to the package id.
    fn final_dep_path_of(
        &self,
        node_id: &NodeId,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> DepPath {
        if let Some(dep_path) = final_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        if let Some(dep_path) = self.node_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        if let Some(dep_path) = link_node_id_as_dep_path(node_id) {
            return dep_path;
        }
        let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
        DepPath::from(self.tree.packages[pkg_id].id.clone())
    }

    /// One resolved-peer slot of `node_id`'s suffix, computed against the
    /// already-finalized depPaths. Mirrors [`Self::build_peer_id`] but
    /// substitutes the provisional cycle fallback with a strongly-
    /// connected-component test: a peer is collapsed to `name@version`
    /// only when it shares a peer-graph SCC with `node_id` (a genuine
    /// cycle, matching pnpm's `cyclicPeerAliases` branch). Non-cyclic
    /// peers carry their full depPath, which is the parity fix.
    fn final_peer_id(
        &self,
        peer_alias: &str,
        peer_node_id: &NodeId,
        node_scc: usize,
        scc_of: &HashMap<NodeId, usize>,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> PeerId {
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
            let (name, version) = pkg_name_version(&pkg.result);
            PeerId::Pair { name, version }
        };
        if self.opts.dedupe_peers && self.tree.dependencies_tree.contains_key(peer_node_id) {
            return pair();
        }
        if scc_of.get(peer_node_id) == Some(&node_scc) {
            return pair();
        }
        PeerId::DepPath(self.final_dep_path_of(peer_node_id, final_dep_paths))
    }

    /// Recompute every node's depPath with its resolved peers' *full*
    /// suffixes. Genuine peer cycles (detected as multi-node peer-graph
    /// SCCs, or self-loops) keep the `name@version` collapse; every other
    /// peer slot carries the peer's own depPath. This is the synchronous
    /// equivalent of pnpm's deferred `calculateDepPath` + `analyzeGraph`
    /// cycle detection.
    fn build_final_dep_paths(&self) -> HashMap<NodeId, DepPath> {
        let (sccs, scc_of) = self.peer_sccs();
        let mut final_dep_paths: HashMap<NodeId, DepPath> = HashMap::new();
        // SCCs come out of Tarjan in reverse-topological order, so a
        // node's cross-SCC peers are already finalized when we reach it.
        for (scc_index, scc) in sccs.iter().enumerate() {
            for node_id in scc {
                let Some(peers) = self.node_external_peers.get(node_id) else { continue };
                if peers.is_empty() {
                    continue;
                }
                let peer_ids: Vec<PeerId> = peers
                    .iter()
                    .map(|(peer_alias, peer_node_id)| {
                        self.final_peer_id(
                            peer_alias,
                            peer_node_id,
                            scc_index,
                            &scc_of,
                            &final_dep_paths,
                        )
                    })
                    .collect();
                let suffix =
                    create_peer_dep_graph_hash(&peer_ids, self.opts.peers_suffix_max_length);
                let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
                let dep_path =
                    DepPath::from(format!("{}{}", self.tree.packages[pkg_id].id, suffix));
                final_dep_paths.insert(node_id.clone(), dep_path);
            }
        }
        final_dep_paths
    }

    /// Strongly-connected components of the peer graph (node → resolved
    /// peers, restricted to peers that themselves carry peers — peerless
    /// peers can't close a cycle). Iterative Tarjan, returning the SCCs
    /// in reverse-topological order plus a `NodeId → SCC index` map.
    fn peer_sccs(&self) -> (Vec<Vec<NodeId>>, HashMap<NodeId, usize>) {
        let participants: HashSet<&NodeId> = self
            .node_external_peers
            .iter()
            .filter(|(_, peers)| !peers.is_empty())
            .map(|(node_id, _)| node_id)
            .collect();
        let neighbors = |node_id: &NodeId| -> Vec<NodeId> {
            self.node_external_peers
                .get(node_id)
                .into_iter()
                .flat_map(|peers| peers.values())
                .filter(|peer| participants.contains(*peer))
                .cloned()
                .collect()
        };

        let mut index_of: HashMap<NodeId, u32> = HashMap::new();
        let mut low_of: HashMap<NodeId, u32> = HashMap::new();
        let mut on_stack: HashSet<NodeId> = HashSet::new();
        let mut tarjan_stack: Vec<NodeId> = Vec::new();
        let mut sccs: Vec<Vec<NodeId>> = Vec::new();
        let mut scc_of: HashMap<NodeId, usize> = HashMap::new();
        let mut next_index: u32 = 0;

        // Explicit DFS stack of (node, neighbors, cursor) so deep peer
        // graphs don't overflow the call stack.
        for root in &participants {
            if index_of.contains_key(*root) {
                continue;
            }
            let mut work: Vec<(NodeId, Vec<NodeId>, usize)> =
                vec![((*root).clone(), neighbors(root), 0)];
            while let Some((node_id, succ, cursor)) = work.last_mut() {
                if *cursor == 0 {
                    index_of.insert(node_id.clone(), next_index);
                    low_of.insert(node_id.clone(), next_index);
                    next_index += 1;
                    on_stack.insert(node_id.clone());
                    tarjan_stack.push(node_id.clone());
                }
                if *cursor < succ.len() {
                    let child = succ[*cursor].clone();
                    *cursor += 1;
                    if !index_of.contains_key(&child) {
                        let child_succ = neighbors(&child);
                        work.push((child, child_succ, 0));
                    } else if on_stack.contains(&child) {
                        let node_low = low_of[node_id];
                        let child_index = index_of[&child];
                        low_of.insert(node_id.clone(), node_low.min(child_index));
                    }
                    continue;
                }
                // All successors visited — close this node.
                let node_id = node_id.clone();
                if low_of[&node_id] == index_of[&node_id] {
                    let scc_index = sccs.len();
                    let mut component = Vec::new();
                    while let Some(member) = tarjan_stack.pop() {
                        on_stack.remove(&member);
                        scc_of.insert(member.clone(), scc_index);
                        let is_root = member == node_id;
                        component.push(member);
                        if is_root {
                            break;
                        }
                    }
                    sccs.push(component);
                }
                work.pop();
                if let Some((parent, _, _)) = work.last() {
                    let parent_low = low_of[parent];
                    let node_low = low_of[&node_id];
                    low_of.insert(parent.clone(), parent_low.min(node_low));
                }
            }
        }
        (sccs, scc_of)
    }

    /// Rebuild the depPath-keyed graph from the per-`NodeId`
    /// [`NodeRecord`]s using the corrected `final_dep_paths`. Nodes that
    /// resolve to the same final depPath merge (taking the smallest
    /// `depth`, like the inline build); nodes whose suffix was
    /// previously collapsed by the cycle fallback now split into
    /// distinct entries.
    fn build_final_graph(&self, final_dep_paths: &HashMap<NodeId, DepPath>) -> DependenciesGraph {
        // Minimum tree depth across *every* occurrence that resolves to a
        // given final depPath. `pure_pkgs` / `find_hit` revisits
        // short-circuit before a [`NodeRecord`] is created, so iterating
        // `node_records` alone would restore the first (possibly deeper)
        // walk's depth and miss a later shallower revisit. `node_dep_paths`
        // carries every walked NodeId, so recompute the `Math.min` depth
        // tie-break here — the inline build threaded it through `self.graph`,
        // which this rebuild discards.
        let mut min_depth: HashMap<DepPath, i32> = HashMap::new();
        for node_id in self.node_dep_paths.keys() {
            let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else { continue };
            let dep_path = self.final_dep_path_of(node_id, final_dep_paths);
            min_depth
                .entry(dep_path)
                .and_modify(|depth| *depth = (*depth).min(tree_node.depth))
                .or_insert(tree_node.depth);
        }

        let mut graph = DependenciesGraph::new();
        for (node_id, record) in &self.node_records {
            let dep_path = self.final_dep_path_of(node_id, final_dep_paths);
            let depth = min_depth.get(&dep_path).copied().unwrap_or(record.depth);
            let pkg_id = self.tree.dependencies_tree[node_id].resolved_package_id.clone();
            let pkg = &self.tree.packages[&pkg_id];
            let children: BTreeMap<String, DepPath> = record
                .edges
                .iter()
                .map(|(alias, edge_node_id)| {
                    (alias.clone(), self.final_dep_path_of(edge_node_id, final_dep_paths))
                })
                .collect();
            let resolved_peer_names: HashSet<String> = self
                .node_external_peers
                .get(node_id)
                .map(|peers| peers.keys().cloned().collect())
                .unwrap_or_default();
            graph
                .entry(dep_path.clone())
                .and_modify(|node| {
                    if node.depth > depth {
                        node.depth = depth;
                    }
                })
                .or_insert(DependenciesGraphNode {
                    dep_path,
                    resolved_package_id: pkg_id.clone(),
                    resolve_result: Arc::clone(&pkg.result),
                    children,
                    peer_dependencies: pkg.peer_dependencies.clone(),
                    transitive_peer_dependencies: record.transitive_peer_dependencies.clone(),
                    resolved_peer_names,
                    depth,
                    installable: record.installable,
                    is_pure: record.is_pure,
                    optional: pkg.optional,
                });
        }
        graph
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

    fn previously_resolved_children(
        &mut self,
        parent_node_ids: &[NodeId],
        parent_pkg_ids_chain: &[String],
        current_pkg_id: &str,
    ) -> BTreeMap<String, NodeId> {
        let mut children = BTreeMap::new();
        if !parent_pkg_ids_chain.iter().any(|pkg_id| pkg_id == current_pkg_id) {
            return children;
        }
        for parent_node_id in parent_node_ids.iter().rev() {
            let same_pkg = self
                .tree
                .dependencies_tree
                .get(parent_node_id)
                .is_some_and(|node| node.resolved_package_id == current_pkg_id);
            if same_pkg {
                for (alias, child_node_id) in self.realize_children(parent_node_id) {
                    children.entry(alias).or_insert(child_node_id);
                }
            }
        }
        children
    }

    fn add_graph_child_or_pending(
        &mut self,
        graph_children: &mut BTreeMap<String, DepPath>,
        parent_dep_path: &DepPath,
        alias: String,
        node_id: NodeId,
    ) {
        if let Some(dep_path) = self.node_dep_paths.get(&node_id) {
            graph_children.insert(alias, dep_path.clone());
        } else if let Some(link_dep_path) = link_node_id_as_dep_path(&node_id) {
            // Mirrors upstream's
            // [`pathsByNodeId.get(childNodeId) ?? (childNodeId as unknown as DepPath)`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/resolvePeers.ts#L164)
            // fallback in `resolveChildren`. `topParents` linked-dep
            // NodeIds never enter the tree, so `node_dep_paths` is
            // empty for them; the `link:<rel>` NodeId is itself a
            // valid DepPath, so the snapshot's child edge can use
            // it verbatim.
            graph_children.insert(alias, link_dep_path);
        } else {
            self.pending_peer_edges.push(PendingPeerEdge {
                parent_dep_path: parent_dep_path.clone(),
                child_alias: alias,
                child_node_id: node_id,
            });
        }
    }

    fn parent_refs_match(&self, current: &ParentRef, new: &ParentRef) -> bool {
        if current.version != new.version || current.alias != new.alias {
            return false;
        }
        let Some(current_name) = self.parent_ref_package_name(current) else {
            return true;
        };
        let Some(new_name) = self.parent_ref_package_name(new) else {
            return true;
        };
        current_name == new_name
    }

    fn parent_ref_package_name(&self, parent_ref: &ParentRef) -> Option<String> {
        let node_id = parent_ref.node_id.as_ref()?;
        let tree_node = self.tree.dependencies_tree.get(node_id)?;
        let pkg = self.tree.packages.get(&tree_node.resolved_package_id)?;
        Some(pkg_name_version(&pkg.result).0)
    }

    fn inherited_parent_pkg_breaks_peer_diamond(
        &self,
        parent_refs: &ParentRefs,
        inherited_parent_pkg: &ParentRef,
        own_child_parent_pkg: &ParentRef,
        children: &BTreeMap<String, NodeId>,
    ) -> bool {
        let (Some(inherited_node_id), Some(own_child_node_id)) =
            (inherited_parent_pkg.node_id.as_ref(), own_child_parent_pkg.node_id.as_ref())
        else {
            return false;
        };
        if inherited_node_id == own_child_node_id {
            return false;
        }
        let Some(inherited_context) = self.parent_pkgs_of_node.get(inherited_node_id) else {
            return false;
        };
        let Some(parent_pkg) = self
            .tree
            .dependencies_tree
            .get(own_child_node_id)
            .and_then(|node| self.tree.packages.get(&node.resolved_package_id))
        else {
            return false;
        };
        let (parent_pkg_name, _) = pkg_name_version(&parent_pkg.result);

        let mut conflicting_peers = HashSet::new();
        for peer_name in parent_pkg.peer_dependencies.keys() {
            if !self.tree.all_peer_dep_names.contains(peer_name) {
                continue;
            }
            let Some(inherited_peer) = inherited_context.get(peer_name) else { continue };
            let Some(current_peer) = parent_refs.get(peer_name) else { continue };
            if self.parent_peer_differs(current_peer, inherited_peer) {
                conflicting_peers.insert(peer_name.clone());
            }
        }
        if conflicting_peers.is_empty() {
            return false;
        }

        for child_node_id in children.values() {
            let Some(child_pkg) = self
                .tree
                .dependencies_tree
                .get(child_node_id)
                .and_then(|node| self.tree.packages.get(&node.resolved_package_id))
            else {
                continue;
            };
            if !child_pkg.peer_dependencies.contains_key(&parent_pkg_name) {
                continue;
            }
            if conflicting_peers.iter().any(|peer| child_pkg.peer_dependencies.contains_key(peer)) {
                return true;
            }
        }
        false
    }

    fn parent_peer_differs(
        &self,
        current_peer: &ParentRef,
        inherited_peer: &ParentPkgInfo,
    ) -> bool {
        if let Some(inherited_pkg_id) = inherited_peer.pkg_id.as_ref() {
            let Some(current_node_id) = current_peer.node_id.as_ref() else {
                return true;
            };
            return self
                .tree
                .dependencies_tree
                .get(current_node_id)
                .is_none_or(|node| node.resolved_package_id != *inherited_pkg_id);
        }
        inherited_peer.version.as_ref() != Some(&current_peer.version)
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
            let version = pkg_id.is_none().then(|| parent_ref.version.clone());
            out.insert(
                name.clone(),
                ParentPkgInfo {
                    pkg_id,
                    version,
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
    ///    same name with a real `NodeId`, unless the package declares
    ///    that cached peer as optional and the current context has no
    ///    provider for it.
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
                let Some(current_ref) = parent_refs.get(name) else {
                    if self.peer_dependency_is_optional(pkg_id, name) {
                        continue;
                    }
                    return false;
                };
                let Some(current_node_id) = current_ref.node_id.as_ref() else {
                    return false;
                };
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

    fn peer_dependency_is_optional(&self, pkg_id: &str, peer_name: &str) -> bool {
        self.tree
            .packages
            .get(pkg_id)
            .and_then(|pkg| pkg.peer_dependencies.get(peer_name))
            .is_some_and(|peer| peer.optional)
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

/// Reproduce upstream's parent-ref dual-keying: each parent is recorded
/// by its install alias *and* its real name when the two differ.
/// Mirrors
/// [`toPkgByName`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L1015-L1033).
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
    depth: i32,
) {
    let (real_name, version) = pkg_name_version(&pkg.result);
    let parent_ref = ParentRef {
        version,
        node_id: Some(parent_node_id),
        alias: (direct_alias != real_name).then(|| direct_alias.to_string()),
        depth,
        occurrence: 0,
    };
    update_parent_refs(refs, direct_alias, &parent_ref);
    if direct_alias != real_name {
        update_parent_refs(refs, &real_name, &parent_ref);
    }
}

fn update_parent_refs(refs: &mut ParentRefs, new_alias: &str, parent_ref: &ParentRef) {
    if let Some(existing) = refs.get(new_alias) {
        let existing_has_alias = existing.alias.as_deref().is_some_and(|alias| alias != new_alias);
        if !existing_has_alias {
            return;
        }
        let new_has_alias = parent_ref.alias.as_deref().is_some_and(|alias| alias != new_alias);
        if new_has_alias && version_gte(&existing.version, &parent_ref.version) {
            return;
        }
    }
    refs.insert(new_alias.to_string(), parent_ref.clone());
}

fn version_gte(left: &str, right: &str) -> bool {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left >= right,
        _ => left >= right,
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
