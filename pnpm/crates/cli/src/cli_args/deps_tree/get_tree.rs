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
    graph::DependencyGraph,
    pkg_info::{EdgeContext, PkgInfoEnv, get_pkg_info},
    search::Searcher,
};

/// Remaining tree depth. `Unlimited` corresponds to the TypeScript
/// `Infinity` depth, which the materialization cache keys differently
/// from any finite depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MaxDepth {
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
pub(crate) type MaterializationCache = HashMap<(TreeNodeId, Option<u64>), CachedSubtreeOpaque>;

/// Opaque wrapper so the cache type can be shared without exposing the
/// bookkeeping fields.
#[derive(Debug, Clone)]
pub(crate) struct CachedSubtreeOpaque(CachedSubtree);

pub(crate) struct GetTreeOptions<'a> {
    pub env: &'a PkgInfoEnv<'a>,
    pub graph: &'a DependencyGraph,
    pub exclude_peer_dependencies: bool,
    pub only_projects: bool,
    pub search: Option<&'a Searcher>,
    pub show_deduped_search_matches: bool,
    /// Directory `link:` versions are rewritten relative to.
    pub rewrite_link_version_dir: PathBuf,
}

struct MaterializationResult {
    nodes: Vec<DependencyNode>,
    count: u64,
    has_search_match: bool,
    search_messages: Vec<String>,
}

pub(crate) fn get_tree(
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
    let empty = || MaterializationResult {
        nodes: Vec::new(),
        count: 0,
        has_search_match: false,
        search_messages: Vec::new(),
    };
    if max_depth.is_exhausted() || guard_depth >= super::MAX_WALK_DEPTH {
        return empty();
    }
    let Some(graph_node) = opts.graph.nodes.get(parent_id) else {
        return empty();
    };

    let child_tree_max_depth = max_depth.decrement();
    let linked_path_base_dir = match parent_id {
        TreeNodeId::Importer(importer_id) => opts.env.lockfile_dir.join(importer_id),
        TreeNodeId::Package(_) => opts.env.lockfile_dir.clone(),
    };

    let mut result_dependencies: Vec<DependencyNode> = Vec::new();
    let mut result_count: u64 = 0;
    let mut result_has_search_match = false;
    let mut result_search_messages: Vec<String> = Vec::new();

    // Sort edges by alias so that deduplication is deterministic: the
    // alphabetically-first dependency always gets fully expanded.
    let mut sorted_edges: Vec<_> = graph_node.edges.iter().collect();
    sorted_edges.sort_by(|a, b| a.alias.cmp(&b.alias));

    for edge in sorted_edges {
        if opts.only_projects && !matches!(edge.target, Some(TreeNodeId::Importer(_))) {
            continue;
        }

        let edge_ctx = EdgeContext {
            peers: Some(&graph_node.peers),
            linked_path_base_dir: linked_path_base_dir.clone(),
            rewrite_link_version_dir: Some(opts.rewrite_link_version_dir.clone()),
            parent_dir: parent_dir.map(Path::to_path_buf),
        };
        let (package_info, _manifest) = get_pkg_info(opts.env, edge, &edge_ctx);

        let search_match = opts.search.map(|search| {
            search.matches(
                &edge.alias,
                &package_info.name,
                &package_info.version,
                edge.target.as_ref(),
            )
        });
        let searching = opts.search.is_some();
        let matched = search_match.as_ref().is_some_and(super::search::SearchMatch::is_match);

        let mut new_entry: DependencyNode;
        let mut child_count: u64 = 0;
        let mut deduped_has_search_match = false;
        let mut deduped_search_messages: Vec<String> = Vec::new();

        match &edge.target {
            None => {
                // External link or unresolvable — no traversal possible.
                if !searching || matched {
                    new_entry = package_info;
                } else {
                    continue;
                }
            }
            Some(target) => {
                let mut dependencies: Vec<DependencyNode>;
                let mut deduped_count: Option<u64> = None;
                let circular = ancestors.contains(target);

                if circular {
                    dependencies = Vec::new();
                } else {
                    let cache_key = (target.clone(), child_tree_max_depth.cache_depth());
                    if let Some(CachedSubtreeOpaque(cached)) = cache.get(&cache_key) {
                        // Subtree already emitted elsewhere in the
                        // output — elide it to avoid repeating nodes.
                        dependencies = Vec::new();
                        if cached.count > 0 {
                            deduped_count = Some(cached.count);
                        }
                        if opts.show_deduped_search_matches {
                            deduped_has_search_match = cached.has_search_match;
                            deduped_search_messages.clone_from(&cached.search_messages);
                        }
                    } else {
                        ancestors.insert(target.clone());
                        let child_result = materialize_children(
                            opts,
                            cache,
                            ancestors,
                            target,
                            child_tree_max_depth,
                            Some(Path::new(&package_info.path)),
                            guard_depth + 1,
                        );
                        ancestors.remove(target);

                        dependencies = child_result.nodes;
                        child_count = child_result.count;

                        cache.insert(
                            cache_key,
                            CachedSubtreeOpaque(CachedSubtree {
                                count: child_count,
                                has_search_match: child_result.has_search_match,
                                search_messages: child_result.search_messages.clone(),
                            }),
                        );
                        if child_result.has_search_match {
                            result_has_search_match = true;
                        }
                        if opts.show_deduped_search_matches {
                            result_search_messages.extend(child_result.search_messages);
                        }
                    }
                    if deduped_has_search_match {
                        result_has_search_match = true;
                        result_search_messages.extend(deduped_search_messages.iter().cloned());
                    }
                }

                if !dependencies.is_empty() {
                    new_entry = package_info;
                    new_entry.dependencies = std::mem::take(&mut dependencies);
                } else if !searching || matched || deduped_has_search_match {
                    new_entry = package_info;
                } else {
                    continue;
                }

                if let Some(count) = deduped_count {
                    new_entry.deduped = true;
                    new_entry.deduped_dependencies_count = Some(count);
                }
            }
        }

        match &search_match {
            Some(search_match) if search_match.is_match() => {
                new_entry.searched = true;
                result_has_search_match = true;
                if let Some(message) = search_match.message() {
                    new_entry.search_message = Some(message.to_string());
                    result_search_messages.push(message.to_string());
                }
            }
            _ => {
                if deduped_has_search_match {
                    new_entry.searched = true;
                    if !deduped_search_messages.is_empty() {
                        new_entry.search_message = Some(deduped_search_messages.join("\n"));
                    }
                }
            }
        }

        if !new_entry.is_peer
            || !opts.exclude_peer_dependencies
            || !new_entry.dependencies.is_empty()
        {
            let has_children = !new_entry.dependencies.is_empty();
            result_count += 1 + if has_children { child_count } else { 0 };
            result_dependencies.push(new_entry);
        }
    }

    MaterializationResult {
        nodes: result_dependencies,
        count: result_count,
        has_search_match: result_has_search_match,
        search_messages: result_search_messages,
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
