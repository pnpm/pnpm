//! The graph-entry capture each walked node performs, and the post-walk
//! passes it feeds: pending-edge repair, SCC-based cycle detection, the
//! final depPath recomputation that gives every resolved peer its full
//! suffix, and the [`DependenciesGraph`] built from the per-node records
//! keyed by those depPaths.

use crate::{
    dependencies_graph::{DependenciesGraph, DependenciesGraphNode},
    node_id::NodeId,
    resolve_peers::{
        context::{
            SharedChain, link_node_id_as_dep_path, peer_id_pair, peer_segment_names, pkg_name,
            pkg_name_version,
        },
        walker::{MissingPeerInfo, Walker},
    },
    resolved_tree::ResolvedPackage,
};
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash, link_path_to_peer_version};
use pnpm_resolving_resolver_base::ResolveResult;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Per-`NodeId` data captured during the walk so the post-walk
/// [`Walker::build_final_dep_paths`] pass can recompute each node's
/// depPath with its resolved peers' *full* suffixes, collapsing to
/// `name@version` only for genuinely detected cycles.
///
/// The walk itself is left untouched: it still computes the provisional
/// depPaths that [`Walker::find_hit`] reads, so peer-resolution and
/// cache decisions are byte-for-byte identical. Only the rendered
/// depPaths change, which is why a node whose suffix was previously
/// collapsed by the cycle fallback now splits into its own graph entry.
pub(super) struct NodeRecord {
    /// `alias → child/peer NodeId` edges, in the same shape the inline
    /// `graph_children` map carries (children overlaid with resolved-peer
    /// edges) but holding `NodeIds`, so the rebuild can map each edge to
    /// its final depPath.
    pub(super) edges: BTreeMap<String, NodeId>,
    pub(super) optional_child_aliases: HashSet<String>,
    pub(super) transitive_peer_dependencies: HashSet<String>,
    pub(super) depth: i32,
    pub(super) installable: bool,
    pub(super) is_pure: bool,
    pub(super) order: u64,
}

/// One walked node's contribution to the graph, as
/// [`Walker::record_walked_node`] receives it: the node's identity, the
/// depPath the walk gave it, and the edge and peer sets its graph entry
/// and [`NodeRecord`] are built from.
pub(super) struct WalkedNode<'a> {
    pub(super) node_id: &'a NodeId,
    pub(super) pkg: &'a ResolvedPackage,
    pub(super) dep_path: &'a DepPath,
    pub(super) parent_node_ids: &'a SharedChain<NodeId>,
    pub(super) parent_pkg_ids_chain: &'a SharedChain<String>,
    /// This node's realized `alias → NodeId` children.
    pub(super) children: &'a BTreeMap<String, NodeId>,
    /// The depPaths those same children resolved to.
    pub(super) child_dep_paths: BTreeMap<String, DepPath>,
    /// Every peer resolved anywhere in this node's subtree.
    pub(super) all_resolved_peers: &'a HashMap<String, NodeId>,
    pub(super) all_missing_peers: &'a HashMap<String, MissingPeerInfo>,
    /// The subset of the above this node declares itself.
    pub(super) own_resolved_peers: &'a HashMap<String, NodeId>,
    pub(super) depth: i32,
    pub(super) installable: bool,
    pub(super) is_pure: bool,
}

/// One `parent → child` edge whose target wasn't walked yet at the
/// time the parent's `graph_children` was built. Patched up by
/// [`Walker::patch_pending_peer_edges`] after the main walk completes.
pub(super) struct PendingPeerEdge {
    parent_dep_path: DepPath,
    child_alias: String,
    child_node_id: NodeId,
}

#[derive(Clone, Copy)]
struct FinalPeerContext<'a> {
    scc_of: &'a HashMap<NodeId, usize>,
    cyclic_peer_names: &'a HashSet<String>,
}

/// Iterative Tarjan over the peer graph. The DFS stack is explicit so deep
/// peer graphs don't overflow the call stack.
#[derive(Default)]
struct PeerSccPass {
    index_of: HashMap<NodeId, u32>,
    low_of: HashMap<NodeId, u32>,
    on_stack: HashSet<NodeId>,
    tarjan_stack: Vec<NodeId>,
    /// Reverse-topological order, as Tarjan closes them.
    sccs: Vec<Vec<NodeId>>,
    scc_of: HashMap<NodeId, usize>,
    next_index: u32,
}

impl PeerSccPass {
    fn visit_root<Neighbors>(&mut self, root: &NodeId, neighbors: &Neighbors)
    where
        Neighbors: Fn(&NodeId) -> Vec<NodeId>,
    {
        let mut work: Vec<(NodeId, Vec<NodeId>, usize)> = vec![(root.clone(), neighbors(root), 0)];
        while let Some((node_id, successors, cursor)) = work.last_mut() {
            if *cursor == 0 {
                self.open(node_id);
            }
            if let Some(child) = self.next_unvisited(node_id, successors, cursor) {
                let child_successors = neighbors(&child);
                work.push((child, child_successors, 0));
                continue;
            }
            let node_id = node_id.clone();
            self.close_scc(&node_id);
            work.pop();
            if let Some((parent, _, _)) = work.last() {
                let parent_low = self.low_of[parent];
                let node_low = self.low_of[&node_id];
                self.low_of.insert(parent.clone(), parent_low.min(node_low));
            }
        }
    }

    fn open(&mut self, node_id: &NodeId) {
        self.index_of.insert(node_id.clone(), self.next_index);
        self.low_of.insert(node_id.clone(), self.next_index);
        self.next_index += 1;
        self.on_stack.insert(node_id.clone());
        self.tarjan_stack.push(node_id.clone());
    }

    /// Advances `node_id`'s successor cursor to its next unvisited peer,
    /// folding every successor already on the stack into its lowlink on the
    /// way.
    fn next_unvisited(
        &mut self,
        node_id: &NodeId,
        successors: &[NodeId],
        cursor: &mut usize,
    ) -> Option<NodeId> {
        while *cursor < successors.len() {
            let child = successors[*cursor].clone();
            *cursor += 1;
            if !self.index_of.contains_key(&child) {
                return Some(child);
            }
            if self.on_stack.contains(&child) {
                let node_low = self.low_of[node_id];
                let child_index = self.index_of[&child];
                self.low_of.insert(node_id.clone(), node_low.min(child_index));
            }
        }
        None
    }

    fn close_scc(&mut self, root: &NodeId) {
        if self.low_of[root] != self.index_of[root] {
            return;
        }
        let scc_index = self.sccs.len();
        let mut component = Vec::new();
        while let Some(member) = self.tarjan_stack.pop() {
            self.on_stack.remove(&member);
            self.scc_of.insert(member.clone(), scc_index);
            let is_root = member == *root;
            component.push(member);
            if is_root {
                break;
            }
        }
        self.sccs.push(component);
    }
}

impl Walker<'_> {
    /// Return the fully walked occurrence whose peer-resolution verdict
    /// `node_id` reused. The cache hit is the same semantic node for
    /// final depPath construction even though it remains a distinct tree
    /// occurrence for traversal and edge labels.
    fn cache_owner_node_id<'a>(&'a self, node_id: &'a NodeId) -> &'a NodeId {
        self.cache_owner_by_node_id.get(node_id).unwrap_or(node_id)
    }

    /// Record one walked node: its entry in the provisional
    /// depPath-keyed graph, and the [`NodeRecord`] the post-walk rebuild
    /// consumes. A discovery pass runs neither of the passes that read
    /// these, so it never calls this.
    pub(super) fn record_walked_node(&mut self, mut node: WalkedNode<'_>) {
        // Seeds both the node's record edges and its graph children, so
        // it is computed once: a second call would rescan the ancestor
        // chain and re-realize every matching occurrence's children.
        let mut record_edges = self.previously_resolved_children(
            node.parent_node_ids,
            node.parent_pkg_ids_chain,
            &node.pkg.id,
        );
        let graph_children = self.graph_children_of(
            node.dep_path,
            &record_edges,
            std::mem::take(&mut node.child_dep_paths),
            node.all_resolved_peers,
        );
        let transitive_peer_dependencies =
            transitive_peer_names(node.pkg, node.all_resolved_peers, node.all_missing_peers);
        self.extend_record_edges(&mut record_edges, &node);
        let optional_child_aliases = self.optional_child_aliases(&node.pkg.id, &record_edges);
        let record_order = self.next_record_order;
        self.next_record_order += 1;
        self.node_records.insert(
            node.node_id.clone(),
            NodeRecord {
                edges: record_edges,
                optional_child_aliases: optional_child_aliases.clone(),
                transitive_peer_dependencies: transitive_peer_dependencies.clone(),
                depth: node.depth,
                installable: node.installable,
                is_pure: node.is_pure,
                order: record_order,
            },
        );

        self.record_graph_node(
            &node,
            graph_children,
            optional_child_aliases,
            transitive_peer_dependencies,
        );
    }

    /// Occurrences sharing a dependency path retain the shallowest install depth.
    fn record_graph_node(
        &mut self,
        node: &WalkedNode<'_>,
        graph_children: BTreeMap<String, DepPath>,
        optional_child_aliases: HashSet<String>,
        transitive_peer_dependencies: HashSet<String>,
    ) {
        self.graph
            .entry(node.dep_path.clone())
            .and_modify(|entry| {
                if entry.depth > node.depth {
                    entry.depth = node.depth;
                }
            })
            .or_insert(DependenciesGraphNode {
                dep_path: node.dep_path.clone(),
                resolved_package_id: node.pkg.id.to_string(),
                resolve_result: Arc::clone(&node.pkg.result),
                children: graph_children,
                optional_children: optional_child_aliases,
                peer_dependencies: node.pkg.peer_dependencies.clone(),
                transitive_peer_dependencies,
                resolved_peer_names: node.all_resolved_peers.keys().cloned().collect(),
                depth: node.depth,
                installable: node.installable,
                is_pure: node.is_pure,
                optional: node.pkg.optional,
            });
    }

    /// Finish this node's NodeId-level edges for the post-walk
    /// [`Walker::build_final_dep_paths`] rebuild: its regular children
    /// overlaid with its *own* resolved peers — this node's own peer
    /// resolution, not the descendants' peers bubbled up for the
    /// suffix. A peer a descendant resolved (e.g. `debug`'s optional
    /// `supports-color`) is symlinked at the descendant that declares
    /// it, so it must not appear in this node's dependencies.
    fn extend_record_edges(&mut self, edges: &mut BTreeMap<String, NodeId>, node: &WalkedNode<'_>) {
        let remapped_backedges =
            self.remapped_backedge_children(&node.pkg.id, node.children, node.depth + 1);
        edges.extend(node.children.clone());
        for (alias, node_id) in remapped_backedges {
            edges.insert(alias, node_id);
        }
        for (peer_alias, peer_node_id) in node.own_resolved_peers {
            edges.insert(peer_alias.clone(), peer_node_id.clone());
        }
    }

    /// The children's depPath edges become this node's graph children.
    /// Resolved peers become extra edges, aliased by peer name. If a peer's
    /// depPath isn't known yet — typically a later sibling direct dep — the
    /// edge is deferred to the post-walk patch pass; the install layer drives
    /// off `graph_children`, so skipping the edge entirely would leave the
    /// peer un-symlinked in the parent's slot.
    fn graph_children_of(
        &mut self,
        dep_path: &DepPath,
        record_edges: &BTreeMap<String, NodeId>,
        child_dep_paths: BTreeMap<String, DepPath>,
        all_resolved_peers: &HashMap<String, NodeId>,
    ) -> BTreeMap<String, DepPath> {
        let mut graph_children = BTreeMap::new();
        for (alias, child_node_id) in record_edges {
            self.add_graph_child_or_pending(
                &mut graph_children,
                dep_path,
                alias.clone(),
                child_node_id.clone(),
            );
        }
        for (alias, child_dep_path) in child_dep_paths {
            graph_children.insert(alias, child_dep_path);
        }
        for (peer_alias, peer_node_id) in all_resolved_peers {
            self.add_graph_child_or_pending(
                &mut graph_children,
                dep_path,
                peer_alias.clone(),
                peer_node_id.clone(),
            );
        }
        graph_children
    }

    /// A back-edge child whose occurrence node the walk skipped would render
    /// as its bare package id — a snapshot no variant of the target has.
    /// These remap it to the target's shared canonical occurrence, which the
    /// drivers walk at importer context.
    fn remapped_backedge_children(
        &mut self,
        pkg_id: &Arc<str>,
        children: &BTreeMap<String, NodeId>,
        child_depth: i32,
    ) -> Vec<(String, NodeId)> {
        let canonical_scc = self.canonical_scc();
        let mut remapped = Vec::new();
        for (alias, child_node_id) in children {
            if self.node_dep_paths.contains_key(child_node_id) {
                continue;
            }
            let Some(child_pkg_id) = self
                .tree
                .dependencies_tree
                .get(child_node_id)
                .map(|child| Arc::clone(&child.resolved_package_id))
            else {
                continue;
            };
            if Self::cuts_cycle_edge(&canonical_scc, pkg_id, &child_pkg_id) {
                remapped.push((
                    alias.clone(),
                    self.canonical_backedge_node(&child_pkg_id, child_depth),
                ));
            }
        }
        remapped
    }

    /// Fill in `graph_children` edges that were skipped during the main
    /// walk because the peer target's `DepPath` hadn't been computed
    /// yet. Each direct dep's subtree is fully walked by the time
    /// `walk()` drains this list, so every peer that was reachable
    /// from an ancestor's [`ParentRefs`](super::context::ParentRefs) has
    /// a `DepPath` now. Peers that still don't resolve here came from a
    /// `parent_chain` outside the walked set — there's nothing to patch,
    /// and the absence already surfaced via
    /// [`crate::PeerDependencyIssues::missing`].
    pub(super) fn patch_pending_peer_edges(&mut self) {
        // Cleared with the buffer: a triple pushed after this drain must be
        // applied again, since the graph it patches has moved on.
        self.pending_peer_edge_keys.clear();
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

    /// Recompute every node's depPath with its resolved peers' *full*
    /// suffixes. Genuine peer cycles (detected as multi-node peer-graph
    /// SCCs, or self-loops) keep the `name@version` collapse; every other
    /// peer slot carries the peer's own depPath. The cycle detection
    /// runs synchronously over the already-walked graph.
    pub(super) fn build_final_dep_paths(&self) -> HashMap<NodeId, DepPath> {
        let (_, scc_of) = self.peer_sccs();
        let cyclic_peer_names = self.cyclic_peer_names();
        let mut final_dep_paths: HashMap<NodeId, DepPath> = HashMap::default();
        let mut visiting = HashSet::default();
        let mut node_ids: Vec<NodeId> = self.node_external_peers.keys().cloned().collect();
        node_ids.sort();
        for node_id in node_ids {
            self.final_dep_path_for_node(
                &node_id,
                &scc_of,
                &cyclic_peer_names,
                &mut final_dep_paths,
                &mut visiting,
            );
        }
        final_dep_paths
    }

    fn final_dep_path_for_node(
        &self,
        node_id: &NodeId,
        scc_of: &HashMap<NodeId, usize>,
        cyclic_peer_names: &HashSet<String>,
        final_dep_paths: &mut HashMap<NodeId, DepPath>,
        visiting: &mut HashSet<NodeId>,
    ) -> DepPath {
        let node_id = self.cache_owner_node_id(node_id);
        if let Some(dep_path) = final_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        let Some(peers) = self.node_external_peers.get(node_id) else {
            return self.provisional_dep_path_of(node_id);
        };
        if peers.is_empty() {
            return self.provisional_dep_path_of(node_id);
        }
        if !visiting.insert(node_id.clone()) {
            return self.provisional_dep_path_of(node_id);
        }
        let peer_ids: Vec<PeerId> = peers
            .iter()
            .map(|(peer_alias, peer_node_id)| {
                self.final_peer_id(
                    node_id,
                    peer_alias,
                    peer_node_id,
                    FinalPeerContext { scc_of, cyclic_peer_names },
                    final_dep_paths,
                    visiting,
                )
            })
            .collect();
        let suffix = create_peer_dep_graph_hash(&peer_ids, self.opts.peers_suffix_max_length);
        let pkg_id = &self.tree.dependencies_tree[node_id].resolved_package_id;
        let dep_path = DepPath::from(format!("{}{}", self.tree.packages[pkg_id].id, suffix));
        final_dep_paths.insert(node_id.clone(), dep_path.clone());
        visiting.remove(node_id);
        dep_path
    }

    /// Resolve `node_id` to the depPath the rebuilt graph should key /
    /// reference it by. Prefers the corrected `final_dep_paths` entry
    /// and falls back to the provisional value for peerless nodes.
    pub(super) fn final_dep_path_of(
        &self,
        node_id: &NodeId,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> DepPath {
        let node_id = self.cache_owner_node_id(node_id);
        if let Some(dep_path) = final_dep_paths.get(node_id) {
            return dep_path.clone();
        }
        self.provisional_dep_path_of(node_id)
    }

    /// One resolved-peer slot of `node_id`'s suffix, computed against the
    /// already-finalized depPaths. Like [`Self::build_peer_id`] but
    /// substitutes the provisional cycle fallback with a strongly-
    /// connected-component test: a peer is collapsed to `name@version`
    /// only when it shares a peer-graph SCC with `node_id` (a genuine
    /// cycle). Non-cyclic peers carry their full depPath.
    fn final_peer_id(
        &self,
        node_id: &NodeId,
        peer_alias: &str,
        peer_node_id: &NodeId,
        context: FinalPeerContext<'_>,
        final_dep_paths: &mut HashMap<NodeId, DepPath>,
        visiting: &mut HashSet<NodeId>,
    ) -> PeerId {
        let peer_node_id = self.cache_owner_node_id(peer_node_id);
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
            peer_id_pair(&pkg.result)
        };
        if self.opts.dedupe_peers && self.tree.dependencies_tree.contains_key(peer_node_id) {
            return pair();
        }
        if context
            .scc_of
            .get(node_id)
            .is_some_and(|node_scc| context.scc_of.get(peer_node_id) == Some(node_scc))
        {
            return pair();
        }
        if context.cyclic_peer_names.contains(peer_alias) {
            return pair();
        }
        PeerId::DepPath(self.final_dep_path_for_node(
            peer_node_id,
            context.scc_of,
            context.cyclic_peer_names,
            final_dep_paths,
            visiting,
        ))
    }

    /// The upstream `pathsByNodeId`: every walked node's final
    /// `DepPath`. Empty unless
    /// [`ResolvePeersOptions::collect_paths_by_node_id`](super::ResolvePeersOptions::collect_paths_by_node_id)
    /// asked for it.
    pub(super) fn final_paths_by_node_id(
        &self,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> HashMap<NodeId, DepPath> {
        if !self.opts.collect_paths_by_node_id {
            return HashMap::default();
        }
        self.node_dep_paths
            .keys()
            .map(|node_id| (node_id.clone(), self.final_dep_path_of(node_id, final_dep_paths)))
            .collect()
    }

    fn cyclic_peer_names(&self) -> HashSet<String> {
        let graph = self.peer_name_graph();

        let mut tarjan = PeerNameTarjan {
            graph: &graph,
            index_of: HashMap::default(),
            low_of: HashMap::default(),
            on_stack: HashSet::default(),
            tarjan_stack: Vec::new(),
            cyclic: HashSet::default(),
            next_index: 0,
        };
        for name in graph.keys() {
            if !tarjan.index_of.contains_key(name.as_str()) {
                tarjan.strongconnect(name);
            }
        }
        tarjan.cyclic
    }

    /// The `package name → resolved peer names` graph, with every peer name
    /// present as a vertex of its own.
    ///
    /// Collected by package id, which every occurrence already carries, and
    /// rendered to names only when folding the result: the graph is over
    /// package *names*, of which a workspace has far fewer than the
    /// occurrences contributing to them, and rendering one costs an
    /// allocation (`PkgName` holds scope and bare name separately). Two
    /// package ids can share a name — different versions of one package — so
    /// the fold unions their edges.
    fn peer_name_graph(&self) -> BTreeMap<String, BTreeSet<&str>> {
        let mut edges_of_pkg: HashMap<&str, BTreeSet<&str>> = HashMap::default();
        for (node_id, peers) in &self.node_external_peers {
            if peers.is_empty() {
                continue;
            }
            let pkg_id = &*self.tree.dependencies_tree[node_id].resolved_package_id;
            let edges = edges_of_pkg.entry(pkg_id).or_default();
            for peer_alias in peers.keys() {
                edges.insert(peer_alias.as_str());
            }
        }

        let mut graph: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
        for (pkg_id, edges) in edges_of_pkg {
            graph.entry(pkg_name(&self.tree.packages[pkg_id].result)).or_default().extend(edges);
        }
        let peer_names: Vec<&str> =
            graph.values().flat_map(|edges| edges.iter().copied()).collect();
        for peer_name in peer_names {
            if !graph.contains_key(peer_name) {
                graph.insert(peer_name.to_string(), BTreeSet::default());
            }
        }
        graph
    }

    /// Strongly-connected components of the peer graph (node → resolved
    /// peers, restricted to peers that themselves carry peers — peerless
    /// peers can't close a cycle). Iterative Tarjan, returning the SCCs
    /// in reverse-topological order plus a `NodeId → SCC index` map.
    ///
    /// Vertices and edge targets are canonicalized through
    /// [`Self::cache_owner_node_id`] to match the owner-keyed lookups
    /// in [`Self::final_peer_id`]: a cycle through a cache-hit
    /// occurrence is a cycle through its owner.
    fn peer_sccs(&self) -> (Vec<Vec<NodeId>>, HashMap<NodeId, usize>) {
        let mut participants: Vec<NodeId> = self
            .node_external_peers
            .iter()
            .filter(|(_, peers)| !peers.is_empty())
            .map(|(node_id, _)| self.cache_owner_node_id(node_id).clone())
            .collect();
        participants.sort();
        participants.dedup();
        let participant_set: HashSet<NodeId> = participants.iter().cloned().collect();
        let neighbors = |node_id: &NodeId| -> Vec<NodeId> {
            let mut out: Vec<NodeId> = self
                .node_external_peers
                .get(node_id)
                .into_iter()
                .flat_map(|peers| peers.values())
                .map(|peer| self.cache_owner_node_id(peer))
                .filter(|peer| participant_set.contains(*peer))
                .cloned()
                .collect();
            out.sort();
            out.dedup();
            out
        };

        let mut pass = PeerSccPass::default();
        for root in &participants {
            if pass.index_of.contains_key(root) {
                continue;
            }
            pass.visit_root(root, &neighbors);
        }
        (pass.sccs, pass.scc_of)
    }

    /// Rebuild the depPath-keyed graph from the per-`NodeId`
    /// [`NodeRecord`]s using the corrected `final_dep_paths`. Nodes that
    /// resolve to the same final depPath merge (taking the smallest
    /// `depth`, like the inline build); nodes whose suffix was
    /// previously collapsed by the cycle fallback now split into
    /// distinct entries.
    ///
    /// Every edge — a regular child or a resolved peer — points at the
    /// depPath the edge's own node resolved to, matching upstream's
    /// `resolveChildren`, which maps each `childrenNodeIds` entry
    /// through `pathsByNodeId`. A peer provider therefore keeps its own
    /// peer suffix even where the consumer resolved none of those peers
    /// itself.
    pub(super) fn build_final_graph(
        &self,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> DependenciesGraph {
        let min_depth = self.min_depth_by_final_dep_path(final_dep_paths);
        let (record_dep_paths, transitive_peer_dependencies_by_dep_path) =
            self.final_record_dep_paths(final_dep_paths);

        let mut graph = DependenciesGraph::default();
        let mut graph_order: HashMap<DepPath, u64> = HashMap::default();
        for (node_id, record) in &self.node_records {
            let dep_path = record_dep_paths[node_id].clone();
            let depth = min_depth.get(&dep_path).copied().unwrap_or(record.depth);
            let candidate =
                self.graph_node_for_record(node_id, record, dep_path, depth, final_dep_paths);
            insert_graph_node(
                &mut graph,
                &mut graph_order,
                candidate,
                record.order,
                &transitive_peer_dependencies_by_dep_path,
            );
        }
        graph
    }

    /// Minimum tree depth across *every* occurrence that resolves to a given
    /// final depPath. `pure_pkgs` / `find_hit` revisits short-circuit before
    /// a [`NodeRecord`] is created, so iterating `node_records` alone would
    /// restore the first (possibly deeper) walk's depth and miss a later
    /// shallower revisit. `node_dep_paths` carries every walked `NodeId`, so
    /// the `Math.min` depth tie-break is recomputed here — the inline build
    /// threaded it through `self.graph`, which this rebuild discards.
    fn min_depth_by_final_dep_path(
        &self,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> HashMap<DepPath, i32> {
        let mut min_depth: HashMap<DepPath, i32> = HashMap::default();
        for node_id in self.node_dep_paths.keys() {
            let Some(tree_node) = self.tree.dependencies_tree.get(node_id) else { continue };
            let dep_path = self.final_dep_path_of(node_id, final_dep_paths);
            min_depth
                .entry(dep_path)
                .and_modify(|depth| *depth = (*depth).min(tree_node.depth))
                .or_insert(tree_node.depth);
        }
        min_depth
    }

    /// The final depPath each record resolves to, and the transitive peers
    /// every record sharing a depPath contributes to it.
    fn final_record_dep_paths(
        &self,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> (HashMap<NodeId, DepPath>, HashMap<DepPath, HashSet<String>>) {
        let mut record_dep_paths: HashMap<NodeId, DepPath> = HashMap::default();
        let mut transitive_by_dep_path: HashMap<DepPath, HashSet<String>> = HashMap::default();
        for (node_id, record) in &self.node_records {
            let dep_path = self.final_dep_path_of(node_id, final_dep_paths);
            transitive_by_dep_path
                .entry(dep_path.clone())
                .or_default()
                .extend(record.transitive_peer_dependencies.iter().cloned());
            record_dep_paths.insert(node_id.clone(), dep_path);
        }
        (record_dep_paths, transitive_by_dep_path)
    }

    fn graph_node_for_record(
        &self,
        node_id: &NodeId,
        record: &NodeRecord,
        dep_path: DepPath,
        depth: i32,
        final_dep_paths: &HashMap<NodeId, DepPath>,
    ) -> DependenciesGraphNode {
        let pkg_id = Arc::<str>::clone(&self.tree.dependencies_tree[node_id].resolved_package_id);
        let pkg = &self.tree.packages[&pkg_id];
        let mut children: BTreeMap<String, DepPath> = BTreeMap::new();
        for (alias, edge_node_id) in &record.edges {
            children.insert(alias.clone(), self.final_dep_path_of(edge_node_id, final_dep_paths));
        }
        let resolved_peer_names: HashSet<String> = self
            .node_external_peers
            .get(node_id)
            .map(|peers| peers.keys().cloned().collect())
            .unwrap_or_default();
        DependenciesGraphNode {
            dep_path,
            resolved_package_id: pkg_id.to_string(),
            resolve_result: Arc::clone(&pkg.result),
            children,
            optional_children: record.optional_child_aliases.clone(),
            peer_dependencies: pkg.peer_dependencies.clone(),
            transitive_peer_dependencies: record.transitive_peer_dependencies.clone(),
            resolved_peer_names,
            depth,
            installable: record.installable,
            is_pure: record.is_pure,
            optional: pkg.optional,
        }
    }

    pub(super) fn add_graph_child_or_pending(
        &mut self,
        graph_children: &mut BTreeMap<String, DepPath>,
        parent_dep_path: &DepPath,
        alias: String,
        node_id: NodeId,
    ) {
        if let Some(dep_path) = self.node_dep_paths.get(&node_id) {
            graph_children.insert(alias, dep_path.clone());
        } else if let Some(link_dep_path) = link_node_id_as_dep_path(&node_id) {
            // `topParents` linked-dep NodeIds never enter the tree, so
            // `node_dep_paths` is empty for them; the `link:<rel>`
            // NodeId is itself a valid DepPath, so the snapshot's child
            // edge can use it verbatim.
            graph_children.insert(alias, link_dep_path);
        } else {
            // Exact duplicates are no-ops: `patch_pending_peer_edges`
            // resolves the same `child_node_id` to the same `DepPath` and
            // then `or_insert`s the same `(parent, alias)` slot. A parent
            // reached through many occurrences pushes the same triple over
            // and over, so drop repeats instead of buffering millions.
            if self.pending_peer_edge_keys.insert((
                parent_dep_path.clone(),
                alias.clone(),
                node_id.clone(),
            )) {
                self.pending_peer_edges.push(PendingPeerEdge {
                    parent_dep_path: parent_dep_path.clone(),
                    child_alias: alias,
                    child_node_id: node_id,
                });
            }
        }
    }
}

/// Merge one record's graph node into the depPath-keyed graph. Records that
/// share a depPath collapse onto one entry, the shallowest — and among equals
/// the earliest walked — one supplying the node's own fields while the rest
/// still contribute their peers, optional children and child edges.
fn insert_graph_node(
    graph: &mut DependenciesGraph,
    graph_order: &mut HashMap<DepPath, u64>,
    mut candidate: DependenciesGraphNode,
    order: u64,
    transitive_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) {
    let dep_path = candidate.dep_path.clone();
    match graph.entry(dep_path.clone()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            graph_order.insert(dep_path, order);
            entry.insert(candidate);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            let existing = entry.get();
            let existing_order = graph_order.get(&dep_path).copied().unwrap_or(order);
            let replace = candidate.depth < existing.depth
                || (candidate.depth == existing.depth && order < existing_order);
            if !replace {
                let existing = entry.get_mut();
                existing
                    .transitive_peer_dependencies
                    .extend(candidate.transitive_peer_dependencies);
                existing.optional_children.extend(candidate.optional_children);
                merge_preferred_child_edges(existing, candidate.children, transitive_by_dep_path);
                return;
            }
            candidate
                .transitive_peer_dependencies
                .extend(existing.transitive_peer_dependencies.iter().cloned());
            candidate.optional_children.extend(existing.optional_children.iter().cloned());
            merge_preferred_child_edges(
                &mut candidate,
                existing.children.clone(),
                transitive_by_dep_path,
            );
            graph_order.insert(dep_path, order);
            entry.insert(candidate);
        }
    }
}

/// The peers visible in a node's subtree that its own manifest does not
/// declare.
fn transitive_peer_names(
    pkg: &ResolvedPackage,
    all_resolved_peers: &HashMap<String, NodeId>,
    all_missing_peers: &HashMap<String, MissingPeerInfo>,
) -> HashSet<String> {
    all_resolved_peers
        .keys()
        .chain(all_missing_peers.keys())
        .filter(|peer_alias| !pkg.peer_dependencies.contains_key(peer_alias.as_str()))
        .cloned()
        .collect()
}

fn merge_preferred_child_edges(
    target: &mut DependenciesGraphNode,
    children: BTreeMap<String, DepPath>,
    transitive_peer_dependencies_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) {
    let available_peer_names =
        available_peer_names_for_dep_path(&target.dep_path, &target.resolve_result);
    for (alias, candidate_dep_path) in children {
        match target.children.entry(alias) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidate_dep_path);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let preferred = child_dep_path_is_preferred(
                    entry.get(),
                    &candidate_dep_path,
                    &available_peer_names,
                    transitive_peer_dependencies_by_dep_path,
                );
                if preferred {
                    entry.insert(candidate_dep_path);
                }
            }
        }
    }
}

fn available_peer_names_for_dep_path(
    dep_path: &DepPath,
    resolve_result: &ResolveResult,
) -> HashSet<String> {
    let mut names: HashSet<String> =
        peer_segment_names(dep_path).unwrap_or_default().into_iter().collect();
    names.insert(pkg_name_version(resolve_result).0);
    names
}

fn child_dep_path_is_preferred(
    current: &DepPath,
    candidate: &DepPath,
    available_peer_names: &HashSet<String>,
    transitive_peer_dependencies_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) -> bool {
    if current == candidate {
        return false;
    }
    let current_unavailable = unavailable_non_transitive_peer_segment_names(
        current,
        available_peer_names,
        transitive_peer_dependencies_by_dep_path,
    )
    .unwrap_or_default();
    let candidate_unavailable = unavailable_non_transitive_peer_segment_names(
        candidate,
        available_peer_names,
        transitive_peer_dependencies_by_dep_path,
    )
    .unwrap_or_default();
    if candidate_unavailable.len() != current_unavailable.len() {
        return candidate_unavailable.len() < current_unavailable.len();
    }
    if !candidate_unavailable.is_empty() {
        return false;
    }
    let current_peer_count =
        available_peer_segment_count(current, available_peer_names).unwrap_or(0);
    let candidate_peer_count =
        available_peer_segment_count(candidate, available_peer_names).unwrap_or(0);
    candidate_peer_count > current_peer_count
}

fn available_peer_segment_count(
    dep_path: &DepPath,
    available_peer_names: &HashSet<String>,
) -> Option<usize> {
    let names = peer_segment_names(dep_path)?;
    Some(names.into_iter().filter(|name| available_peer_names.contains(name)).count())
}

fn unavailable_non_transitive_peer_segment_names(
    dep_path: &DepPath,
    available_peer_names: &HashSet<String>,
    transitive_peer_dependencies_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) -> Option<Vec<String>> {
    let transitive_peer_dependencies = transitive_peer_dependencies_by_dep_path.get(dep_path);
    let names = peer_segment_names(dep_path)?;
    Some(
        names
            .into_iter()
            .filter(|name| {
                !available_peer_names.contains(name)
                    && transitive_peer_dependencies
                        .is_none_or(|transitive| !transitive.contains(name))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;

struct PeerNameTarjan<'a> {
    graph: &'a BTreeMap<String, BTreeSet<&'a str>>,
    index_of: HashMap<&'a str, u32>,
    low_of: HashMap<&'a str, u32>,
    on_stack: HashSet<&'a str>,
    tarjan_stack: Vec<&'a str>,
    cyclic: HashSet<String>,
    next_index: u32,
}

impl<'a> PeerNameTarjan<'a> {
    fn strongconnect(&mut self, name: &'a str) {
        self.index_of.insert(name, self.next_index);
        self.low_of.insert(name, self.next_index);
        self.next_index += 1;
        self.on_stack.insert(name);
        self.tarjan_stack.push(name);
        self.visit_neighbors(name);
        self.close_component(name);
    }

    fn visit_neighbors(&mut self, name: &'a str) {
        let Some(neighbors) = self.graph.get(name) else { return };
        for child in neighbors {
            if !self.index_of.contains_key(child) {
                self.strongconnect(child);
                let name_low = self.low_of[name];
                let child_low = self.low_of[child];
                self.low_of.insert(name, name_low.min(child_low));
            } else if self.on_stack.contains(child) {
                let name_low = self.low_of[name];
                let child_index = self.index_of[child];
                self.low_of.insert(name, name_low.min(child_index));
            }
        }
    }

    /// A component is cyclic when more than one name takes part in
    /// it, or when its single name depends on itself.
    fn close_component(&mut self, name: &'a str) {
        if self.low_of[name] != self.index_of[name] {
            return;
        }
        let mut component = Vec::new();
        while let Some(member) = self.tarjan_stack.pop() {
            self.on_stack.remove(&member);
            let is_root = member == name;
            component.push(member);
            if is_root {
                break;
            }
        }
        let self_loop = component.first().is_some_and(|member| {
            self.graph.get(*member).is_some_and(|edges| edges.contains(member))
        });
        if component.len() > 1 || self_loop {
            self.cyclic.extend(component.into_iter().map(str::to_owned));
        }
    }
}
