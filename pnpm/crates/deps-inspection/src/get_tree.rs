//! Materialize the dependency graph into renderable [`DependencyNode`]
//! trees, with subtree deduplication, depth limiting, search pruning,
//! and circular-reference marking. Rust counterpart of the TypeScript
//! tree-builder's `getTree` / `materializeChildren` / `fixCircularRefs`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use super::{
    DependencyNode, TreeNodeId,
    graph::{DependencyGraph, GraphEdge},
    pkg_info::{EdgeContext, PkgInfoEnv, get_pkg_info},
    search::Searcher,
};

/// Remaining tree depth. `Unlimited` corresponds to the TypeScript
/// `Infinity` depth, which the materialization cache keys differently
/// from any finite depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaxDepth {
    Finite(u64),
    Unlimited,
}

impl MaxDepth {
    fn is_exhausted(self) -> bool {
        matches!(self, MaxDepth::Finite(0))
    }

    fn decrement(self) -> MaxDepth {
        match self {
            MaxDepth::Finite(depth) => MaxDepth::Finite(depth.saturating_sub(1)),
            MaxDepth::Unlimited => MaxDepth::Unlimited,
        }
    }

    fn cache_depth(self) -> Option<u64> {
        match self {
            MaxDepth::Finite(depth) => Some(depth),
            MaxDepth::Unlimited => None,
        }
    }
}

#[derive(Debug, Clone)]
struct CachedSubtree {
    count: u64,
    has_search_match: bool,
    search_messages: Vec<String>,
}

/// Caches already-materialized subtrees keyed by `(node, remaining
/// depth)`. A cache hit elides the subtree (the node is marked
/// `deduped`), bounding the total output to `O(N)` nodes.
pub type MaterializationCache = HashMap<(TreeNodeId, Option<u64>), CachedSubtreeOpaque>;

/// Opaque wrapper so the cache type can be shared without exposing the
/// bookkeeping fields.
#[derive(Debug, Clone)]
pub struct CachedSubtreeOpaque(CachedSubtree);

pub struct GetTreeOptions<'a> {
    pub env: &'a PkgInfoEnv<'a>,
    pub graph: &'a DependencyGraph,
    pub exclude_peer_dependencies: bool,
    pub only_projects: bool,
    pub search: Option<&'a Searcher>,
    pub show_deduped_search_matches: bool,
    /// Directory `link:` versions are rewritten relative to.
    pub rewrite_link_version_dir: PathBuf,
}

#[derive(Default)]
struct MaterializationResult {
    nodes: Vec<DependencyNode>,
    count: u64,
    has_search_match: bool,
    search_messages: Vec<String>,
}

pub fn get_tree(
    opts: &GetTreeOptions<'_>,
    cache: &mut MaterializationCache,
    parent_id: &TreeNodeId,
    max_depth: MaxDepth,
    parent_dir: Option<&Path>,
) -> Vec<DependencyNode> {
    let mut ancestors = HashSet::new();
    ancestors.insert(parent_id.clone());

    let result =
        materialize_children(opts, cache, &mut ancestors, parent_id, max_depth, parent_dir, 0);

    // Circular back-edges are marked in a post-pass: materialization
    // truncates dependencies at cycle boundaries but leaves cached
    // subtrees free of context-dependent circular flags.
    let mut circular_ancestors = HashSet::new();
    if let Some(parent_dir) = parent_dir {
        circular_ancestors.insert(parent_dir.to_string_lossy().into_owned());
    }
    fix_circular_refs(result.nodes, &mut circular_ancestors)
}

fn materialize_children(
    opts: &GetTreeOptions<'_>,
    cache: &mut MaterializationCache,
    ancestors: &mut HashSet<TreeNodeId>,
    parent_id: &TreeNodeId,
    max_depth: MaxDepth,
    parent_dir: Option<&Path>,
    guard_depth: usize,
) -> MaterializationResult {
    if max_depth.is_exhausted() || guard_depth >= super::MAX_WALK_DEPTH {
        return MaterializationResult::default();
    }
    let Some(graph_node) = opts.graph.nodes.get(parent_id) else {
        return MaterializationResult::default();
    };

    let linked_path_base_dir = match parent_id {
        TreeNodeId::Importer(importer_id) => {
            super::build::safe_importer_dir(&opts.env.lockfile_dir, importer_id)
                .unwrap_or_else(|| opts.env.lockfile_dir.clone())
        }
        TreeNodeId::Package(_) => opts.env.lockfile_dir.clone(),
    };

    // Sort edges by alias so that deduplication is deterministic: the
    // alphabetically-first dependency always gets fully expanded.
    let mut sorted_edges: Vec<_> = graph_node.edges.iter().collect();
    sorted_edges.sort_by(|a, b| a.alias.cmp(&b.alias));

    let mut result = MaterializationResult::default();
    for edge in sorted_edges {
        if opts.only_projects && !matches!(edge.target, Some(TreeNodeId::Importer(_))) {
            continue;
        }
        materialize_edge(MaterializeEdge {
            opts,
            cache,
            ancestors,
            edge,
            peers: &graph_node.peers,
            linked_path_base_dir: &linked_path_base_dir,
            parent_dir,
            max_depth: max_depth.decrement(),
            guard_depth,
            result: &mut result,
        });
    }
    result
}

/// One edge of the node being materialized, and where its output goes.
struct MaterializeEdge<'a> {
    opts: &'a GetTreeOptions<'a>,
    cache: &'a mut MaterializationCache,
    ancestors: &'a mut HashSet<TreeNodeId>,
    edge: &'a GraphEdge,
    peers: &'a HashSet<String>,
    linked_path_base_dir: &'a Path,
    parent_dir: Option<&'a Path>,
    /// Depth budget for the edge's own subtree.
    max_depth: MaxDepth,
    guard_depth: usize,
    result: &'a mut MaterializationResult,
}

fn materialize_edge(inputs: MaterializeEdge<'_>) {
    let MaterializeEdge { opts, edge, result, .. } = inputs;
    let edge_ctx = EdgeContext {
        peers: Some(inputs.peers),
        linked_path_base_dir: inputs.linked_path_base_dir.to_path_buf(),
        rewrite_link_version_dir: Some(opts.rewrite_link_version_dir.clone()),
        parent_dir: inputs.parent_dir.map(Path::to_path_buf),
    };
    let (package_info, _) = get_pkg_info(opts.env, edge, &edge_ctx);
    let search_match = opts.search.map(|search| {
        search.matches(&edge.alias, &package_info.name, &package_info.version, edge.target.as_ref())
    });

    // An edge with no target is an external link or an unresolvable
    // reference: there is nothing to traverse into.
    let subtree = match &edge.target {
        None => Subtree::default(),
        Some(target) => materialize_subtree(SubtreeWalk {
            opts,
            cache: inputs.cache,
            ancestors: inputs.ancestors,
            target,
            package_path: &package_info.path,
            max_depth: inputs.max_depth,
            guard_depth: inputs.guard_depth,
        }),
    };
    result.has_search_match |= subtree.walked_has_search_match || subtree.deduped_has_search_match;
    result.search_messages.extend(subtree.walked_search_messages.iter().cloned());
    result.search_messages.extend(subtree.deduped_search_messages.iter().cloned());

    // An entry is kept when it has children to show, when it matched the
    // search itself, or when it stands in for an elided subtree that did.
    let keep = !subtree.dependencies.is_empty()
        || opts.search.is_none()
        || search_match.as_ref().is_some_and(super::search::SearchMatch::is_match)
        || subtree.deduped_has_search_match;
    if !keep {
        return;
    }

    record_materialized_edge(package_info, subtree, search_match.as_ref(), opts, result);
}

fn record_materialized_edge(
    mut entry: DependencyNode,
    mut subtree: Subtree,
    search_match: Option<&super::search::SearchMatch>,
    opts: &GetTreeOptions<'_>,
    result: &mut MaterializationResult,
) {
    entry.dependencies = std::mem::take(&mut subtree.dependencies);
    if let Some(count) = subtree.deduped_count {
        entry.deduped = true;
        entry.deduped_dependencies_count = Some(count);
    }
    annotate_search(&mut entry, search_match, &subtree, result);

    if entry.is_peer && opts.exclude_peer_dependencies && entry.dependencies.is_empty() {
        return;
    }
    result.count += 1 + if entry.dependencies.is_empty() { 0 } else { subtree.count };
    result.nodes.push(entry);
}

/// What materializing one edge's target produced.
#[derive(Default)]
struct Subtree {
    dependencies: Vec<DependencyNode>,
    /// Nodes below this edge, for the parent's running total.
    count: u64,
    /// Set when the subtree was elided as a duplicate, carrying the node
    /// count of the copy that is shown instead.
    deduped_count: Option<u64>,
    /// Search state of an elided duplicate. The copy that carries the
    /// matches is elsewhere in the output, so this entry reports them.
    deduped_has_search_match: bool,
    deduped_search_messages: Vec<String>,
    /// Search state of a freshly walked subtree, already carried by the
    /// nodes in `dependencies`.
    walked_has_search_match: bool,
    walked_search_messages: Vec<String>,
}

struct SubtreeWalk<'a> {
    opts: &'a GetTreeOptions<'a>,
    cache: &'a mut MaterializationCache,
    ancestors: &'a mut HashSet<TreeNodeId>,
    target: &'a TreeNodeId,
    /// Resolved path of the edge's package, the parent directory of the
    /// subtree's own resolution.
    package_path: &'a str,
    max_depth: MaxDepth,
    guard_depth: usize,
}

fn materialize_subtree(walk: SubtreeWalk<'_>) -> Subtree {
    let SubtreeWalk { opts, cache, ancestors, target, package_path, max_depth, guard_depth } = walk;

    // A back-edge to an ancestor is truncated here; `fix_circular_refs`
    // flags it in a post-pass.
    if ancestors.contains(target) {
        return Subtree::default();
    }

    let cache_key = (target.clone(), max_depth.cache_depth());
    if let Some(CachedSubtreeOpaque(cached)) = cache.get(&cache_key) {
        return deduped_subtree(cached, opts.show_deduped_search_matches);
    }

    ancestors.insert(target.clone());
    let child_result = materialize_children(
        opts,
        cache,
        ancestors,
        target,
        max_depth,
        Some(Path::new(package_path)),
        guard_depth + 1,
    );
    ancestors.remove(target);

    cache.insert(
        cache_key,
        CachedSubtreeOpaque(CachedSubtree {
            count: child_result.count,
            has_search_match: child_result.has_search_match,
            search_messages: child_result.search_messages.clone(),
        }),
    );
    Subtree {
        dependencies: child_result.nodes,
        count: child_result.count,
        walked_has_search_match: child_result.has_search_match,
        walked_search_messages: if opts.show_deduped_search_matches {
            child_result.search_messages
        } else {
            Vec::new()
        },
        ..Subtree::default()
    }
}

/// Represent a subtree already emitted elsewhere, optionally retaining its search hits.
fn deduped_subtree(cached: &CachedSubtree, show_matches: bool) -> Subtree {
    Subtree {
        deduped_count: (cached.count > 0).then_some(cached.count),
        deduped_has_search_match: show_matches && cached.has_search_match,
        deduped_search_messages: if show_matches {
            cached.search_messages.clone()
        } else {
            Vec::new()
        },
        ..Subtree::default()
    }
}

/// Flag the entry as a search hit, from its own match or from the elided
/// duplicate it stands in for.
fn annotate_search(
    entry: &mut DependencyNode,
    search_match: Option<&super::search::SearchMatch>,
    subtree: &Subtree,
    result: &mut MaterializationResult,
) {
    if let Some(search_match) = search_match.filter(|search_match| search_match.is_match()) {
        entry.searched = true;
        result.has_search_match = true;
        if let Some(message) = search_match.message() {
            entry.search_message = Some(message.to_string());
            result.search_messages.push(message.to_string());
        }
        return;
    }
    if subtree.deduped_has_search_match {
        entry.searched = true;
        if !subtree.deduped_search_messages.is_empty() {
            entry.search_message = Some(subtree.deduped_search_messages.join("\n"));
        }
    }
}

/// Mark circular back-edges: a node whose `path` matches an ancestor's
/// gets `circular` and its dependencies (and dedup bookkeeping)
/// stripped.
fn fix_circular_refs(
    nodes: Vec<DependencyNode>,
    ancestors: &mut HashSet<String>,
) -> Vec<DependencyNode> {
    nodes
        .into_iter()
        .map(|mut node| {
            if !node.path.is_empty() && ancestors.contains(&node.path) {
                node.circular = true;
                node.dependencies = Vec::new();
                node.deduped = false;
                node.deduped_dependencies_count = None;
                return node;
            }
            if node.dependencies.is_empty() {
                return node;
            }
            ancestors.insert(node.path.clone());
            node.dependencies =
                fix_circular_refs(std::mem::take(&mut node.dependencies), ancestors);
            ancestors.remove(&node.path);
            node
        })
        .collect()
}

#[cfg(test)]
mod tests;
