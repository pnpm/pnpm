//! Lockfile-backed dependency graph shared by the forward (`list`) and
//! reverse (`why`) tree builders.

use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    BoundProviders,
    Lockfile,
    PeerEdgeOptions,
    PeerSatisfactionEdges,
    PkgNameVerPeer,
    ProjectSnapshot,
    SnapshotEntry,
};
use pnpm_modules_yaml::IncludedDependencies;

use super::TreeNodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DepField {
    #[serde(rename = "dependencies")]
    Dependencies,
    #[serde(rename = "devDependencies")]
    DevDependencies,
    #[serde(rename = "optionalDependencies")]
    OptionalDependencies,
}

impl DepField {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DepField::Dependencies => "dependencies",
            DepField::DevDependencies => "devDependencies",
            DepField::OptionalDependencies => "optionalDependencies",
        }
    }
}

/// One outgoing dependency edge of a graph node.
#[derive(Debug, Clone)]
pub struct GraphEdge {
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
    /// The selected importer declaration, retained after group filtering.
    pub importer_dependency: Option<ImporterDependency>,
}

#[derive(Debug, Clone)]
pub struct ImporterDependency {
    pub field: DepField,
    pub specifier: String,
}

impl GraphEdge {
    /// Whether the edge may lead to a project. Besides the importers of the
    /// lockfile, this includes a project's `link:` dependency on a directory
    /// outside the lockfile: with a dedicated lockfile per project, every
    /// other workspace project is such a directory. `pnpm list` keeps those
    /// only when the directory is a workspace project.
    #[must_use]
    pub fn leads_to_project(&self, parent: &TreeNodeId) -> bool {
        match &self.target {
            Some(target) => matches!(target, TreeNodeId::Importer(_)),
            None => {
                matches!(parent, TreeNodeId::Importer(_)) && self.link_target.is_some()
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct GraphNode {
    pub edges: Vec<GraphEdge>,
    /// Names declared in this package's `peerDependencies`, a child
    /// edge whose alias is in this set is a peer dependency.
    pub peers: HashSet<String>,
}

#[derive(Debug, Default)]
pub struct DependencyGraph {
    pub nodes: HashMap<TreeNodeId, GraphNode>,
}

pub struct BuildGraphOptions<'a> {
    pub lockfile: &'a Lockfile,
    pub include: IncludedDependencies,
    pub only_projects: bool,
    /// How the graph classifies the peer-satisfaction edges it leaves out
    /// while `include` excludes a group.
    pub peer_edges: PeerEdgeOptions,
    pub bound_peer_edges: BoundPeerEdges,
}

/// What the graph does with the edges through which a package receives the
/// peers its key binds. A dependency that is declared as a peer but not bound
/// by the key is the package's own and is always followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundPeerEdges {
    Follow,
    /// Leave them out, so packages reachable only as a peer are not in the
    /// graph.
    Skip,
}

/// Breadth-first walk from `root_ids`, recording every reachable node
/// and its outgoing edges. Mirrors the TypeScript `buildDependencyGraph`.
#[must_use]
pub fn build_dependency_graph(
    root_ids: Vec<TreeNodeId>,
    opts: &BuildGraphOptions<'_>,
) -> DependencyGraph {
    let mut graph = DependencyGraph::default();
    let mut queue = root_ids;
    let mut queue_idx = 0;
    let mut visited: HashSet<TreeNodeId> = HashSet::new();
    let skipped_peer_edges = if opts.include.excludes_a_group() {
        PeerSatisfactionEdges::of_lockfile(opts.lockfile, opts.peer_edges)
    } else {
        PeerSatisfactionEdges::default()
    };
    let publish_dirs = publish_directory_importers(opts.lockfile);

    while queue_idx < queue.len() {
        let node_id = queue[queue_idx].clone();
        queue_idx += 1;
        if !visited.insert(node_id.clone()) {
            continue;
        }

        let node = graph_node(&node_id, opts, &skipped_peer_edges, &publish_dirs);
        queue.extend(
            node.edges
                .iter()
                .filter_map(|edge| edge.target.as_ref())
                .filter(|target| !visited.contains(*target))
                .cloned(),
        );
        graph.nodes.insert(node_id, node);
    }

    graph
}

fn graph_node(
    node_id: &TreeNodeId,
    opts: &BuildGraphOptions<'_>,
    skipped_peer_edges: &PeerSatisfactionEdges,
    publish_dirs: &HashMap<String, String>,
) -> GraphNode {
    let mut edges = node_edges(node_id, opts, skipped_peer_edges, publish_dirs);
    let peers = match node_id {
        TreeNodeId::Package(dep_path) => peer_names(opts.lockfile, dep_path),
        TreeNodeId::Importer(_) => HashSet::new(),
    };
    match opts.bound_peer_edges {
        BoundPeerEdges::Follow => {}
        BoundPeerEdges::Skip => {
            if let TreeNodeId::Package(dep_path) = node_id {
                drop_bound_peer_edges(&mut edges, dep_path, &peers, opts.lockfile);
            }
        }
    }
    GraphNode { edges, peers }
}

/// A key without a peer suffix binds nothing, so every dependency of that
/// package is its own, even one whose name it also declares as a peer.
fn drop_bound_peer_edges(
    edges: &mut Vec<GraphEdge>,
    dep_path: &PkgNameVerPeer,
    peers: &HashSet<String>,
    lockfile: &Lockfile,
) {
    if peers.is_empty() || dep_path.suffix.peer().is_empty() {
        return;
    }
    let providers = BoundProviders::of_snapshot(lockfile.into(), dep_path);
    edges.retain(|edge| !is_bound_peer_edge(edge, peers, &providers));
}

/// An edge without a virtual-store target counts as bound when its alias is a
/// declared peer, since there is no key to check the bindings against.
fn is_bound_peer_edge(
    edge: &GraphEdge,
    peers: &HashSet<String>,
    providers: &BoundProviders,
) -> bool {
    peers.contains(&edge.alias)
        && edge.dep_path
            .as_ref()
            .is_none_or(|target| providers.binds(target))
}

/// The outgoing edges of one node. A node the lockfile does not describe has
/// none.
fn node_edges(
    node_id: &TreeNodeId,
    opts: &BuildGraphOptions<'_>,
    skipped_peer_edges: &PeerSatisfactionEdges,
    publish_dirs: &HashMap<String, String>,
) -> Vec<GraphEdge> {
    match node_id {
        TreeNodeId::Importer(importer_id) => opts.lockfile.importers
            .get(importer_id.as_str())
            .map(|importer| importer_edges(importer, importer_id, opts, publish_dirs))
            .unwrap_or_default(),
        TreeNodeId::Package(dep_path) => opts.lockfile.snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(dep_path))
            .map(|snapshot| package_edges(dep_path, snapshot, opts, skipped_peer_edges))
            .unwrap_or_default(),
    }
}

/// Names declared in `peerDependencies` of the `packages:` entry for
/// `dep_path` (looked up by its peer-stripped key).
#[must_use]
pub fn peer_names(lockfile: &Lockfile, dep_path: &PkgNameVerPeer) -> HashSet<String> {
    lockfile.packages
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
    publish_dirs: &HashMap<String, String>,
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    for (field, group) in importer_dependency_groups(importer, &opts.include) {
        for (alias, spec) in group {
            let dep_path = spec.version.resolved_key(alias);
            let link_target = spec.version.as_link_target().map(str::to_string);
            let target = edge_target(
                dep_path.as_ref(),
                link_target.as_deref(),
                Some((importer_id, publish_dirs)),
                opts.lockfile,
            );
            let edge = GraphEdge {
                alias: alias.to_string(),
                ref_display: spec.version.to_string(),
                dep_path,
                link_target,
                target,
                importer_dependency: Some(ImporterDependency {
                    field,
                    specifier: spec.specifier.clone(),
                }),
            };
            if opts.only_projects
                && !edge.leads_to_project(&TreeNodeId::Importer(importer_id.to_string()))
            {
                continue;
            }
            edges.push(edge);
        }
    }
    edges
}

fn importer_dependency_groups<'a>(
    importer: &'a ProjectSnapshot,
    include: &IncludedDependencies,
) -> impl Iterator<Item = (DepField, &'a pnpm_lockfile::ResolvedDependencyMap)> {
    [
        (include.dependencies, DepField::Dependencies, importer.dependencies.as_ref()),
        (include.dev_dependencies, DepField::DevDependencies, importer.dev_dependencies.as_ref()),
        (
            include.optional_dependencies,
            DepField::OptionalDependencies,
            importer.optional_dependencies.as_ref(),
        ),
    ]
    .into_iter()
    .filter_map(|(included, field, dependencies)| {
        if included { dependencies.map(|dependencies| (field, dependencies)) } else { None }
    })
}

fn package_edges(
    key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    opts: &BuildGraphOptions<'_>,
    skipped_peer_edges: &PeerSatisfactionEdges,
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let entries =
        skipped_peer_edges.followed_entries(key, snapshot, opts.include.optional_dependencies);
    for (alias, dep_ref) in entries {
        let dep_path = dep_ref.resolve(alias);
        let link_target = dep_ref.as_link_target().map(str::to_string);
        // Links from external packages are not traversed (the
        // TypeScript `getTreeNodeChildId` returns undefined for
        // package parents), so no importer id is passed here.
        let target = edge_target(dep_path.as_ref(), link_target.as_deref(), None, opts.lockfile);
        if opts.only_projects && !matches!(target, Some(TreeNodeId::Importer(_))) {
            continue;
        }
        edges.push(GraphEdge {
            alias: alias.to_string(),
            ref_display: dep_ref.to_string(),
            dep_path,
            link_target,
            target,
            importer_dependency: None,
        });
    }
    edges
}

/// The node an edge leads to. A resolvable depPath is a package node; a
/// `link:` from an importer resolves to a sibling importer when the
/// linked path is a workspace project or its publish directory; anything
/// else is a leaf edge (`None`).
fn edge_target(
    dep_path: Option<&PkgNameVerPeer>,
    link_target: Option<&str>,
    parent_importer: Option<(&str, &HashMap<String, String>)>,
    lockfile: &Lockfile,
) -> Option<TreeNodeId> {
    if let Some(dep_path) = dep_path {
        return Some(TreeNodeId::Package(dep_path.clone()));
    }
    let link_target = link_target?;
    let (parent_importer_id, publish_dirs) = parent_importer?;
    let importer_id = normalize_importer_path(parent_importer_id, link_target)?;
    if lockfile.importers.contains_key(importer_id.as_str()) {
        return Some(TreeNodeId::Importer(importer_id));
    }
    publish_dirs
        .get(&importer_id)
        .cloned()
        .map(TreeNodeId::Importer)
}

/// The importer each publish directory belongs to, keyed like an importer
/// id. Dependents link a project with `publishConfig.directory` there unless
/// `publishConfig.linkDirectory` is false.
fn publish_directory_importers(lockfile: &Lockfile) -> HashMap<String, String> {
    lockfile.importers
        .iter()
        .filter(|(_, importer)| importer.link_directory.unwrap_or(true))
        .filter_map(|(importer_id, importer)| {
            let publish_directory = importer.publish_directory.as_deref()?;
            let linked_importer_id = normalize_importer_path(importer_id, publish_directory)?;
            Some((linked_importer_id, importer_id.clone()))
        })
        .collect()
}

/// Lexically resolve `relative` against the importer id `base`,
/// producing another importer id (`.` for the workspace root). `None`
/// when the path escapes the workspace root.
#[must_use]
pub fn normalize_importer_path(base: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = base
        .split('/')
        .filter(|segment| !segment.is_empty())
        .filter(|segment| *segment != ".")
        .collect();
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

#[cfg(test)]
mod tests;
