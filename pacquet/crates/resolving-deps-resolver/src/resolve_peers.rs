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
//! transitive-peer propagation, and the basic cycle break. Upstream
//! also runs three optimisations on top of the algorithm that pacquet
//! does **not** port yet:
//!
//! - **`peersCache`** — caches resolved peer combinations keyed by
//!   `pkgIdWithPatchHash` so a repeat visit short-circuits the walk.
//!   Pacquet always recomputes; correctness is unaffected.
//! - **`purePkgs` fast path** — a pure package (no resolved / missing
//!   peers) gets its depPath equal to its `pkgIdWithPatchHash` without
//!   recursing. The general path produces the same answer, just one
//!   walk later.
//! - **`graph-cycles`-driven async deferment** — upstream's
//!   `pathsByNodeIdPromises` lets a cyclic peer pick a `name@version`
//!   peer-id once `analyzeGraph` confirms the cycle. Pacquet performs
//!   a synchronous post-order traversal with an `in_progress` set; a
//!   re-entry on the same `NodeId` falls back to `name@version` as
//!   the peer-id, which is what upstream's cycle resolution converges
//!   on anyway.

use std::collections::{BTreeMap, HashMap, HashSet};

use node_semver::{Range, Version};
use pacquet_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash};

use crate::{
    dependencies_graph::{
        DependenciesGraph, DependenciesGraphNode, MissingPeer, ParentPackageRef,
        PeerDependencyIssue, PeerDependencyIssues,
    },
    node_id::NodeId,
    resolved_tree::{PeerDep, ResolvedPackage, ResolvedTree},
};
use pacquet_resolving_resolver_base::ResolveResult;

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
fn pkg_name_version(result: &ResolveResult) -> (String, String) {
    if let Some(name_ver) = result.name_ver.as_ref() {
        return (name_ver.name.to_string(), name_ver.suffix.to_string());
    }
    let fallback_name = result.alias.clone().unwrap_or_else(|| result.id.as_str().to_string());
    (fallback_name, result.id.as_str().to_string())
}

/// Options threaded into [`fn@resolve_peers`].
#[derive(Debug, Clone, Copy)]
pub struct ResolvePeersOptions {
    /// Cap on the rendered peer-suffix length before pacquet swaps the
    /// suffix for its short hash. Mirrors upstream's
    /// `peersSuffixMaxLength` (default 1000).
    pub peers_suffix_max_length: usize,
}

impl Default for ResolvePeersOptions {
    fn default() -> Self {
        ResolvePeersOptions { peers_suffix_max_length: 1000 }
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

/// Resolve peer dependencies for `tree` and emit a depPath-keyed graph.
pub fn resolve_peers(tree: &ResolvedTree, opts: ResolvePeersOptions) -> ResolvePeersResult {
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
    };
    walker.walk()
}

/// Per-name entry in the propagating `ParentRefs` map. Mirrors upstream's
/// [`ParentRef`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L998-L1006).
///
/// Pacquet's port flattens upstream's `occurrence` / `parentNodeIds`
/// fields — those feed the `peersCache` and `parentPackagesMatch`
/// validation, neither of which is ported in this slice.
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
    /// Recorded for upstream parity but not read in this slice. Used by
    /// `parentPkgsMatch` (cache validation), ported in a later slice.
    #[allow(dead_code, reason = "future peersCache validation")]
    alias: Option<String>,
}

/// `name → ParentRef` map propagated down the walk. Entries are indexed
/// by both the package's real name and its alias when the two differ —
/// `react-dom@npm:next` resolves a `peerDependencies.react-dom`
/// requirement against the alias and `peerDependencies.next` against
/// the real name.
type ParentRefs = HashMap<String, ParentRef>;

struct Walker<'tree> {
    tree: &'tree ResolvedTree,
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

impl<'tree> Walker<'tree> {
    fn walk(mut self) -> ResolvePeersResult {
        let importer_parents = self.build_importer_parents();
        let parent_chain_names: Vec<String> = Vec::new();
        let mut direct_by_alias = BTreeMap::new();
        for direct in &self.tree.direct {
            let output =
                self.resolve_node(direct.node_id, &importer_parents, &parent_chain_names, 0);
            direct_by_alias.insert(direct.alias.clone(), output.dep_path);
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
    fn build_importer_parents(&self) -> ParentRefs {
        let mut refs = ParentRefs::new();
        for direct in &self.tree.direct {
            let Some(tree_node) = self.tree.dependencies_tree.get(&direct.node_id) else {
                continue;
            };
            let Some(pkg) = self.tree.packages.get(&tree_node.resolved_package_id) else {
                continue;
            };
            insert_parent_ref(&mut refs, direct, pkg, self.tree);
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
        if let Some(existing) = self.node_dep_paths.get(&node_id).cloned() {
            return NodeOutput {
                dep_path: existing,
                external_resolved_peers: self
                    .node_external_peers
                    .get(&node_id)
                    .cloned()
                    .unwrap_or_default(),
                missing_peers: self.node_missing_peers.get(&node_id).cloned().unwrap_or_default(),
            };
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
        self.in_progress.insert(node_id);

        let tree_node = self.tree.dependencies_tree[&node_id].clone();
        let pkg = self.tree.packages[&tree_node.resolved_package_id].clone();
        let (pkg_name, _pkg_version) = pkg_name_version(&pkg.result);

        // Build the ParentRefs map that descendants of this node see:
        // parent's view + this node's own children, restricted to
        // names that are declared as peers somewhere in the install.
        let mut child_parent_refs = parent_parent_refs.clone();
        for (alias, child_node_id) in &tree_node.children {
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
                node_id: Some(*child_node_id),
                alias: if alias != &child_real_name { Some(alias.clone()) } else { None },
            };
            if alias_relevant {
                child_parent_refs.insert(alias.clone(), parent_ref.clone());
            }
            if real_relevant && alias != &child_real_name {
                child_parent_refs.insert(child_real_name.clone(), parent_ref);
            }
        }

        let mut child_chain_names: Vec<String> = parent_chain_names.to_vec();
        child_chain_names.push(pkg_name.clone());

        // Recurse into children first (post-order). Collect each child's
        // depPath and the external peers / missing peers they propagate
        // up. Children's external peers may overlap with this node's
        // own children (e.g. a child resolved a peer to a sibling of
        // its parent — i.e., this node's child). Those are not external
        // *to this node* — they're internal here — so filter them out.
        let mut external_from_children: HashMap<String, NodeId> = HashMap::new();
        let mut missing_from_children: HashMap<String, MissingPeerInfo> = HashMap::new();
        let mut child_dep_paths: BTreeMap<String, DepPath> = BTreeMap::new();
        for (alias, child_node_id) in &tree_node.children {
            let child_output = self.resolve_node(
                *child_node_id,
                &child_parent_refs,
                &child_chain_names,
                depth + 1,
            );
            child_dep_paths.insert(alias.clone(), child_output.dep_path);
            for (peer_alias, peer_node_id) in child_output.external_resolved_peers {
                if tree_node.children.values().any(|id| *id == peer_node_id) {
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
            all_resolved_peers.insert(peer_alias.clone(), *peer_node_id);
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
                .values()
                .map(|peer_node_id| self.build_peer_id(*peer_node_id))
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
        self.node_dep_paths.insert(node_id, dep_path.clone());
        self.node_external_peers.insert(node_id, all_resolved_peers.clone());
        self.node_missing_peers.insert(node_id, all_missing_peers.clone());

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
            } else {
                self.pending_peer_edges.push(PendingPeerEdge {
                    parent_dep_path: dep_path.clone(),
                    peer_alias: peer_alias.clone(),
                    peer_node_id: *peer_node_id,
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
                resolve_result: pkg.result.clone(),
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
        let own_child_ids: HashSet<NodeId> = tree_node.children.values().copied().collect();
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
                if let Some(parent_node_id) = parent.node_id {
                    resolved.insert(peer_name.to_string(), parent_node_id);
                }
            }
        }
    }

    /// Build the [`PeerId`] contribution for one resolved peer. If the
    /// peer's depPath is already in `node_dep_paths`, use it (the
    /// `DepPath` form). Otherwise (the cycle path), fall back to
    /// `name@version` from the resolved package.
    fn build_peer_id(&self, peer_node_id: NodeId) -> PeerId {
        if let Some(dep_path) = self.node_dep_paths.get(&peer_node_id) {
            return PeerId::DepPath(dep_path.clone());
        }
        let tree_node = &self.tree.dependencies_tree[&peer_node_id];
        let pkg = &self.tree.packages[&tree_node.resolved_package_id];
        let (name, version) = pkg_name_version(&pkg.result);
        PeerId::Pair { name, version }
    }
}

/// Reproduce upstream's `Map<NodeId, ParentRef>` dual-keying: each
/// parent is recorded by its install alias *and* its real name when
/// the two differ. Mirrors
/// [`updateParentRefs`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L1035-L1044).
fn insert_parent_ref(
    refs: &mut ParentRefs,
    direct: &crate::resolved_tree::DirectDep,
    pkg: &ResolvedPackage,
    tree: &ResolvedTree,
) {
    let (real_name, version) = pkg_name_version(&pkg.result);
    let alias_relevant = tree.all_peer_dep_names.contains(&direct.alias);
    let real_relevant = tree.all_peer_dep_names.contains(&real_name);
    if !alias_relevant && !real_relevant {
        return;
    }
    let parent_ref = ParentRef {
        version: version.clone(),
        node_id: Some(direct.node_id),
        alias: if direct.alias != real_name { Some(direct.alias.clone()) } else { None },
    };
    if alias_relevant {
        refs.insert(direct.alias.clone(), parent_ref.clone());
    }
    if real_relevant && direct.alias != real_name {
        refs.insert(real_name, parent_ref);
    }
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
mod tests {
    use super::satisfies_with_prereleases;

    #[test]
    fn satisfies_handles_basic_ranges() {
        assert!(satisfies_with_prereleases("1.2.3", "^1.0.0"));
        assert!(!satisfies_with_prereleases("2.0.0", "^1.0.0"));
        assert!(satisfies_with_prereleases("18.0.0", "*"));
    }

    #[test]
    fn satisfies_falls_back_to_equality_for_unparsable_ranges() {
        assert!(satisfies_with_prereleases("workspace:^1.0.0", "workspace:^1.0.0"));
        assert!(!satisfies_with_prereleases("1.0.0", "workspace:^1.0.0"));
    }

    #[test]
    fn satisfies_accepts_prerelease_against_non_prerelease_range() {
        // Mirrors Yarn's `satisfiesWithPrereleases` carve-out: a peer
        // candidate at `18.0.0-rc.1` should satisfy a `^18.0.0` peer
        // requirement. node-semver's default `satisfies` rejects this
        // pairing, so the prerelease-strip retry has to catch it.
        assert!(satisfies_with_prereleases("18.0.0-rc.1", "^18.0.0"));
        assert!(satisfies_with_prereleases("1.2.3-beta.0", "^1.2.0"));
        // Out-of-range prereleases still fail.
        assert!(!satisfies_with_prereleases("19.0.0-rc.1", "^18.0.0"));
    }
}
