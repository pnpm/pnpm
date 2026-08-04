//! Lockfile-backed dependency graph shared by the forward (`list`) and
//! reverse (`why`) tree builders.

use std::collections::{HashMap, HashSet};

use pacquet_lockfile::{Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotEntry};
use pacquet_modules_yaml::IncludedDependencies;

use super::TreeNodeId;

/// One outgoing dependency edge of a graph node.
#[derive(Debug, Clone)]
pub(crate) struct GraphEdge {
    pub alias: String,
    /// The raw `version:` reference, used as the display version when
    /// the target cannot be resolved (mirrors the TypeScript
    /// `version = opts.ref` fallback).
    pub ref_display: String,
    /// The `snapshots:`/`packages:` key this edge resolves to, when the
    /// reference addresses the virtual store.
    pub dep_path: Option<PkgNameVerPeer>,
    /// The path portion of a `link:` reference, scheme stripped.
    pub link_target: Option<String>,
    /// The graph node this edge leads to. `None` for edges that cannot
    /// be traversed (links outside the workspace, links from external
    /// packages).
    pub target: Option<TreeNodeId>,
}

#[derive(Debug, Default)]
pub(crate) struct GraphNode {
    pub edges: Vec<GraphEdge>,
    /// Names declared in this package's `peerDependencies` — a child
    /// edge whose alias is in this set is a peer dependency.
    pub peers: HashSet<String>,
}

#[derive(Debug, Default)]
pub(crate) struct DependencyGraph {
    pub nodes: HashMap<TreeNodeId, GraphNode>,
}

pub(crate) struct BuildGraphOptions<'a> {
    pub lockfile: &'a Lockfile,
    pub include: IncludedDependencies,
    pub only_projects: bool,
}

/// Breadth-first walk from `root_ids`, recording every reachable node
/// and its outgoing edges. Mirrors the TypeScript `buildDependencyGraph`.
pub(crate) fn build_dependency_graph(
    root_ids: &[TreeNodeId],
    opts: &BuildGraphOptions<'_>,
) -> DependencyGraph {
    let mut graph = DependencyGraph::default();
    let mut queue: Vec<TreeNodeId> = root_ids.to_vec();
    let mut queue_idx = 0;
    let mut visited: HashSet<TreeNodeId> = HashSet::new();

    while queue_idx < queue.len() {
        let node_id = queue[queue_idx].clone();
        queue_idx += 1;
        if !visited.insert(node_id.clone()) {
            continue;
        }

        let edges = match &node_id {
            TreeNodeId::Importer(importer_id) => {
                match opts.lockfile.importers.get(importer_id.as_str()) {
                    Some(importer) => importer_edges(importer, importer_id, opts),
                    None => Vec::new(),
                }
            }
            TreeNodeId::Package(dep_path) => {
                match opts.lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(dep_path))
                {
                    Some(snapshot) => package_edges(snapshot, opts),
                    None => Vec::new(),
                }
            }
        };

        let peers = match &node_id {
            TreeNodeId::Package(dep_path) => peer_names(opts.lockfile, dep_path),
            TreeNodeId::Importer(_) => HashSet::new(),
        };

        for edge in &edges {
            if let Some(target) = &edge.target
                && !visited.contains(target)
            {
                queue.push(target.clone());
            }
        }

        graph.nodes.insert(node_id, GraphNode { edges, peers });
    }

    graph
}

/// Names declared in `peerDependencies` of the `packages:` entry for
/// `dep_path` (looked up by its peer-stripped key).
pub(crate) fn peer_names(lockfile: &Lockfile, dep_path: &PkgNameVerPeer) -> HashSet<String> {
    lockfile
        .packages
        .as_ref()
        .and_then(|packages| packages.get(&dep_path.without_peer()))
        .and_then(|metadata| metadata.peer_dependencies.as_ref())
        .map(|peers| peers.keys().cloned().collect())
        .unwrap_or_default()
}

fn importer_edges(
    importer: &ProjectSnapshot,
    importer_id: &str,
    opts: &BuildGraphOptions<'_>,
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let groups: [(bool, Option<&pacquet_lockfile::ResolvedDependencyMap>); 3] = [
        (opts.include.dependencies, importer.dependencies.as_ref()),
        (opts.include.dev_dependencies, importer.dev_dependencies.as_ref()),
        (opts.include.optional_dependencies, importer.optional_dependencies.as_ref()),
    ];
    for (included, group) in groups {
        if !included {
            continue;
        }
        for (alias, spec) in group.into_iter().flatten() {
            let dep_path = spec.version.resolved_key(alias);
            let link_target = spec.version.as_link_target().map(str::to_string);
            let target = edge_target(
                dep_path.as_ref(),
                link_target.as_deref(),
                Some(importer_id),
                opts.lockfile,
            );
            if opts.only_projects && !matches!(target, Some(TreeNodeId::Importer(_))) {
                continue;
            }
            edges.push(GraphEdge {
                alias: alias.to_string(),
                ref_display: spec.version.to_string(),
                dep_path,
                link_target,
                target,
            });
        }
    }
    edges
}

fn package_edges(snapshot: &SnapshotEntry, opts: &BuildGraphOptions<'_>) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let groups: [(bool, Option<&HashMap<PkgName, pacquet_lockfile::SnapshotDepRef>>); 2] = [
        (true, snapshot.dependencies.as_ref()),
        (opts.include.optional_dependencies, snapshot.optional_dependencies.as_ref()),
    ];
    for (included, group) in groups {
        if !included {
            continue;
        }
        for (alias, dep_ref) in group.into_iter().flatten() {
            let dep_path = dep_ref.resolve(alias);
            let link_target = dep_ref.as_link_target().map(str::to_string);
            // Links from external packages are not traversed (the
            // TypeScript `getTreeNodeChildId` returns undefined for
            // package parents), so no importer id is passed here.
            let target =
                edge_target(dep_path.as_ref(), link_target.as_deref(), None, opts.lockfile);
            if opts.only_projects && !matches!(target, Some(TreeNodeId::Importer(_))) {
                continue;
            }
            edges.push(GraphEdge {
                alias: alias.to_string(),
                ref_display: dep_ref.to_string(),
                dep_path,
                link_target,
                target,
            });
        }
    }
    edges
}

/// The node an edge leads to. A resolvable depPath is a package node; a
/// `link:` from an importer resolves to a sibling importer when the
/// linked path is itself a workspace project; anything else is a leaf
/// edge (`None`).
fn edge_target(
    dep_path: Option<&PkgNameVerPeer>,
    link_target: Option<&str>,
    parent_importer_id: Option<&str>,
    lockfile: &Lockfile,
) -> Option<TreeNodeId> {
    if let Some(dep_path) = dep_path {
        return Some(TreeNodeId::Package(dep_path.clone()));
    }
    let link_target = link_target?;
    let parent_importer_id = parent_importer_id?;
    let importer_id = normalize_importer_path(parent_importer_id, link_target)?;
    lockfile
        .importers
        .contains_key(importer_id.as_str())
        .then_some(TreeNodeId::Importer(importer_id))
}

/// Lexically resolve `relative` against the importer id `base`,
/// producing another importer id (`.` for the workspace root). `None`
/// when the path escapes the workspace root.
pub(crate) fn normalize_importer_path(base: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> =
        base.split('/').filter(|segment| !segment.is_empty() && *segment != ".").collect();
    let normalized = relative.replace('\\', "/");
    for part in normalized.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() { Some(".".to_string()) } else { Some(parts.join("/")) }
}
