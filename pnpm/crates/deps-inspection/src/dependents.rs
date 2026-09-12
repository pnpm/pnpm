//! Reverse (dependents) tree for `pnpm why`. Rust counterpart of the
//! TypeScript tree-builder's `buildDependentsTree`.

pub use selection::{compare_versions, name_ver_from_dep_path, resolve_package_nodes};

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use pnpm_lockfile::{Lockfile, PkgNameVerPeer, ProjectSnapshot};
use pnpm_package_manifest::parse_manifest_bytes;

use super::{
    TreeNodeId,
    graph::DependencyGraph,
    peers_suffix_hash,
    pkg_info::{EdgeContext, ManifestSource, PkgInfoEnv, get_pkg_info},
    search::{SearchMatch, Searcher},
};

/// One node of the reverse tree: a package or workspace project that
/// depends (directly or transitively) on the searched package.
///
/// `Deserialize` as well as `Serialize`: an embedder that renders through
/// the `@pnpm/napi` bindings gets the tree as JSON, may annotate it (a
/// `displayName` derived from [`Self::manifest`], say), and hands it back
/// to the renderers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DependentNode {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub circular: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers_suffix_hash: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deduped: bool,
    /// For importer leaf nodes: the dependency field the searched
    /// package (or the chain leading to it) is declared in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dep_field: Option<DepField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependents: Option<Vec<DependentNode>>,
    /// The `manifest_fields` projection of this node's `package.json`,
    /// present only when [`BuildDependentsOptions::manifest_fields`] asked
    /// for fields and the manifest could be read. Workspace-project nodes
    /// carry no manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Default for DependentNode {
    fn default() -> Self {
        DependentNode::leaf(String::new(), String::new())
    }
}

impl DependentNode {
    fn leaf(name: String, version: String) -> DependentNode {
        DependentNode {
            name,
            display_name: None,
            version,
            circular: false,
            peers_suffix_hash: None,
            deduped: false,
            dep_field: None,
            dependents: None,
            manifest: None,
        }
    }
}

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

/// One matched package and everything that depends on it.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DependentsTree {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peers_suffix_hash: Option<String>,
    pub dependents: Vec<DependentNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_message: Option<String>,
    /// See [`DependentNode::manifest`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone)]
pub struct ImporterInfo {
    pub name: String,
    pub version: String,
}

pub struct BuildDependentsOptions<'a> {
    pub env: &'a PkgInfoEnv<'a>,
    pub graph: &'a DependencyGraph,
    pub search: &'a Searcher,
    pub importer_info: &'a HashMap<String, ImporterInfo>,
    /// `package.json` fields to project onto every package node of the
    /// tree, in `manifest`. Empty (the CLI's case) reads no manifests at
    /// all; the reverse walk otherwise works purely off the lockfile.
    ///
    /// This is what the TypeScript tree-builder's `nameFormatter` callback
    /// is for: an embedder that renames nodes after a manifest field —
    /// Bit rendering component ids instead of package names — asks for the
    /// field here and sets `displayName` on the returned tree, rather than
    /// handing a callback across the FFI boundary into a synchronous walk.
    pub manifest_fields: &'a [String],
}

struct ReverseEdge {
    parent: TreeNodeId,
    alias: String,
}

/// Reads the requested `package.json` fields of a resolved package node.
/// Each call re-reads the file; a `why` tree names a handful of packages,
/// and caching would keep manifests alive for the whole walk.
struct ManifestProjector<'a> {
    fields: &'a [String],
    resolved: &'a HashMap<TreeNodeId, ManifestSource>,
}

impl ManifestProjector<'_> {
    /// The projection for `node_id`, or `None` when no fields were
    /// requested, the node is a workspace project, or its manifest is
    /// unreadable — an absent manifest is never an error here, it just
    /// leaves the node un-annotated.
    fn project(&self, node_id: &TreeNodeId) -> Option<serde_json::Map<String, serde_json::Value>> {
        if self.fields.is_empty() {
            return None;
        }
        let source = self.resolved.get(node_id)?;
        let bytes = std::fs::read(source.path.join("package.json")).ok()?;
        let manifest = parse_manifest_bytes(&bytes).ok()?;
        let mut projected = serde_json::Map::new();
        for field in self.fields {
            if let Some(value) = manifest.get(field) {
                projected.insert(field.clone(), value.clone());
            }
        }
        (!projected.is_empty()).then_some(projected)
    }
}

struct WalkCtx<'a> {
    reverse_map: &'a HashMap<TreeNodeId, Vec<ReverseEdge>>,
    lockfile: &'a Lockfile,
    importer_info: &'a HashMap<String, ImporterInfo>,
    /// Where each package node's manifest lives, for the
    /// `manifest_fields` projection. Empty when no fields were asked for.
    manifest_reader: ManifestProjector<'a>,
    /// Nodes on the current path, for cycle detection.
    visited: HashSet<TreeNodeId>,
    /// Nodes already fully expanded, for deduplication across branches.
    expanded: HashSet<TreeNodeId>,
}

/// Scan every package node of the graph for search matches and build
/// the reverse tree of each match. Each distinct depPath (peer-variant)
/// is a separate result.
pub fn build_dependents_tree(opts: &BuildDependentsOptions<'_>) -> Vec<DependentsTree> {
    let lockfile = opts.env.current_lockfile;
    let reverse_map = invert_graph(opts.graph);
    let resolved_nodes = resolve_package_nodes(opts.env, opts.graph);

    let mut trees: Vec<DependentsTree> = Vec::new();
    for node_id in opts.graph.nodes.keys() {
        let TreeNodeId::Package(dep_path) = node_id else {
            continue;
        };
        if !lockfile.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(dep_path)) {
            continue;
        }
        let (name, version) = name_ver_from_dep_path(lockfile, dep_path);
        let Some(resolved) = resolved_nodes.get(node_id) else {
            continue;
        };
        let matched = match_package(opts, &reverse_map, node_id, &name, &version);
        if !matched.is_match() {
            continue;
        }

        trees.push(DependentsTree {
            name,
            display_name: None,
            version,
            path: Some(resolved.path.to_string_lossy().into_owned()),
            peers_suffix_hash: peers_suffix_hash(dep_path),
            dependents: walk_dependents_of(opts, &reverse_map, &resolved_nodes, node_id),
            search_message: matched.message().map(str::to_string),
            manifest: ManifestProjector { fields: opts.manifest_fields, resolved: &resolved_nodes }
                .project(node_id),
        });
    }

    sort_trees(&mut trees);
    trees
}

fn walk_dependents_of(
    opts: &BuildDependentsOptions<'_>,
    reverse_map: &HashMap<TreeNodeId, Vec<ReverseEdge>>,
    resolved_nodes: &HashMap<TreeNodeId, ManifestSource>,
    node_id: &TreeNodeId,
) -> Vec<DependentNode> {
    let mut ctx = WalkCtx {
        reverse_map,
        lockfile: opts.env.current_lockfile,
        importer_info: opts.importer_info,
        manifest_reader: ManifestProjector {
            fields: opts.manifest_fields,
            resolved: resolved_nodes,
        },
        visited: HashSet::from([node_id.clone()]),
        expanded: HashSet::new(),
    };
    walk_reverse(&mut ctx, node_id, 0)
}

fn sort_trees(trees: &mut [DependentsTree]) {
    trees.sort_by(|a, b| {
        a.name.cmp(&b.name).then_with(|| compare_versions(&a.version, &b.version)).then_with(|| {
            a.peers_suffix_hash
                .as_deref()
                .unwrap_or("")
                .cmp(b.peers_suffix_hash.as_deref().unwrap_or(""))
        })
    });
}

fn invert_graph(graph: &DependencyGraph) -> HashMap<TreeNodeId, Vec<ReverseEdge>> {
    let mut reverse: HashMap<TreeNodeId, Vec<ReverseEdge>> = HashMap::new();
    for (parent_id, node) in &graph.nodes {
        for edge in &node.edges {
            let Some(target) = &edge.target else {
                continue;
            };
            reverse
                .entry(target.clone())
                .or_default()
                .push(ReverseEdge { parent: parent_id.clone(), alias: edge.alias.clone() });
        }
    }
    reverse
}

fn walk_reverse(ctx: &mut WalkCtx<'_>, node_id: &TreeNodeId, depth: usize) -> Vec<DependentNode> {
    if depth >= super::MAX_WALK_DEPTH {
        return Vec::new();
    }
    let Some(reverse_edges) = ctx.reverse_map.get(node_id) else {
        return Vec::new();
    };

    // Sort by parent name (serialized id as tiebreaker) so
    // deduplication is deterministic: the first parent always gets
    // fully expanded.
    let mut sorted_edges: Vec<&ReverseEdge> = reverse_edges.iter().collect();
    sorted_edges.sort_by(|a, b| {
        resolve_parent_name(ctx, &a.parent)
            .cmp(&resolve_parent_name(ctx, &b.parent))
            .then_with(|| a.parent.serialize().cmp(&b.parent.serialize()))
    });

    let mut dependents: Vec<DependentNode> = Vec::new();
    for edge in sorted_edges {
        let node = if ctx.visited.contains(&edge.parent) {
            circular_node(ctx, &edge.parent)
        } else {
            expand_parent(ctx, edge, depth)
        };
        dependents.extend(node);
    }
    dependents
}

/// The parent is an ancestor of this position: report the cycle as a leaf
/// rather than descending into it again.
fn circular_node(ctx: &WalkCtx<'_>, parent: &TreeNodeId) -> Option<DependentNode> {
    match parent {
        TreeNodeId::Importer(importer_id) => {
            let info = ctx.importer_info.get(importer_id)?;
            let mut node = DependentNode::leaf(info.name.clone(), info.version.clone());
            node.circular = true;
            Some(node)
        }
        TreeNodeId::Package(dep_path) => {
            if !has_snapshot(ctx, dep_path) {
                return None;
            }
            let (name, version) = name_ver_from_dep_path(ctx.lockfile, dep_path);
            let mut node = DependentNode::leaf(name, version);
            node.circular = true;
            node.manifest = ctx.manifest_reader.project(parent);
            Some(node)
        }
    }
}

/// The node for one not-yet-visited parent, with its own dependents walked
/// in unless the tree already carries them elsewhere.
fn expand_parent(ctx: &mut WalkCtx<'_>, edge: &ReverseEdge, depth: usize) -> Option<DependentNode> {
    let dep_path = match &edge.parent {
        TreeNodeId::Importer(importer_id) => return Some(importer_node(ctx, importer_id, edge)),
        TreeNodeId::Package(dep_path) => dep_path,
    };
    if !has_snapshot(ctx, dep_path) {
        return None;
    }
    let (name, version) = name_ver_from_dep_path(ctx.lockfile, dep_path);
    let mut node = DependentNode::leaf(name, version);
    node.peers_suffix_hash = peers_suffix_hash(dep_path);
    node.manifest = ctx.manifest_reader.project(&edge.parent);

    if ctx.expanded.contains(&edge.parent) {
        // Already expanded elsewhere in the tree — show as a leaf to keep
        // the output bounded.
        node.deduped = true;
        return Some(node);
    }

    ctx.visited.insert(edge.parent.clone());
    ctx.expanded.insert(edge.parent.clone());
    let child_dependents = walk_reverse(ctx, &edge.parent, depth + 1);
    ctx.visited.remove(&edge.parent);
    node.dependents = (!child_dependents.is_empty()).then_some(child_dependents);
    Some(node)
}

/// A workspace project is always a leaf: the search stops at the project
/// that declares the dependency.
fn importer_node(ctx: &WalkCtx<'_>, importer_id: &str, edge: &ReverseEdge) -> DependentNode {
    let (name, version) = match ctx.importer_info.get(importer_id) {
        Some(info) => (info.name.clone(), info.version.clone()),
        None => (importer_id.to_string(), String::new()),
    };
    let mut node = DependentNode::leaf(name, version);
    node.dep_field = ctx
        .lockfile
        .importers
        .get(importer_id)
        .and_then(|importer| dep_field_for_alias(&edge.alias, importer));
    node
}

fn resolve_parent_name(ctx: &WalkCtx<'_>, parent: &TreeNodeId) -> String {
    match parent {
        TreeNodeId::Importer(importer_id) => ctx
            .importer_info
            .get(importer_id)
            .map_or_else(|| importer_id.clone(), |info| info.name.clone()),
        TreeNodeId::Package(dep_path) => {
            if ctx
                .lockfile
                .snapshots
                .as_ref()
                .is_some_and(|snapshots| snapshots.contains_key(dep_path))
            {
                dep_path.name.to_string()
            } else {
                String::new()
            }
        }
    }
}

fn dep_field_for_alias(alias: &str, importer: &ProjectSnapshot) -> Option<DepField> {
    let has = |group: Option<&pnpm_lockfile::ResolvedDependencyMap>| {
        group.is_some_and(|deps| deps.keys().any(|key| key.to_string() == alias))
    };
    if has(importer.dev_dependencies.as_ref()) {
        return Some(DepField::DevDependencies);
    }
    if has(importer.optional_dependencies.as_ref()) {
        return Some(DepField::OptionalDependencies);
    }
    if has(importer.dependencies.as_ref()) {
        return Some(DepField::Dependencies);
    }
    None
}

mod selection;

use selection::{has_snapshot, match_package};
