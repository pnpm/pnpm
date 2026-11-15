//! Reverse (dependents) tree for `pnpm why`. Rust counterpart of the
//! TypeScript tree-builder's `buildDependentsTree`.

pub use selection::{
    compare_versions, merge_trees, name_ver_from_dep_path, resolve_package_nodes, sort_trees,
};

use std::collections::{HashMap, HashSet};

use pnpm_lockfile::{Lockfile, PkgNameVerPeer};

use super::{
    TreeNodeId,
    graph::{DependencyGraph, ImporterDependency},
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
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the public dependent-tree JSON accepted and returned by the NAPI addon."
    )
)]
pub struct DependentNode {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    /// The raw dependency specifier the parent declared for this edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
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
    /// Workspace project identity for importer leaves, kept across
    /// [`merge_trees`] so unnamed projects do not collapse into one leaf.
    #[serde(skip)]
    pub(crate) importer_id: Option<String>,
    /// Package identity for non-semver nodes whose display `version` is empty
    /// (such as `file:` directory snapshots), kept across [`merge_trees`] so
    /// distinct targets with the same package name do not collapse into one
    /// node.
    #[serde(skip)]
    pub(crate) merge_id: Option<String>,
    /// Originating matched-package paths from merged per-project trees,
    /// used by `pnpm why --parseable --long`.
    #[serde(skip)]
    pub(crate) root_paths: Vec<String>,
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
            requires: None,
            circular: false,
            peers_suffix_hash: None,
            deduped: false,
            dep_field: None,
            dependents: None,
            manifest: None,
            importer_id: None,
            merge_id: None,
            root_paths: Vec::new(),
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
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror the public dependents-tree JSON plus an internal skip-serialized merge key."
    )
)]
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
    /// See [`DependentNode::merge_id`].
    #[serde(skip)]
    pub(crate) merge_id: Option<String>,
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
    /// tree, in `manifest`. An empty slice omits this projection;
    /// dependency specifiers are read independently.
    ///
    /// This is what the TypeScript tree-builder's `nameFormatter` callback
    /// is for: an embedder that renames nodes after a manifest field —
    /// Bit rendering component ids instead of package names — asks for the
    /// field here and sets `displayName` on the returned tree, rather than
    /// handing a callback across the FFI boundary into a synchronous walk.
    pub manifest_fields: &'a [String],
}

#[derive(Clone, Copy)]
enum DependencyKind {
    Dependency,
    Peer,
}

struct ReverseEdge {
    parent: TreeNodeId,
    alias: String,
    kind: DependencyKind,
    importer_dependency: Option<ImporterDependency>,
}

struct WalkCtx<'a> {
    lockfile_dir: &'a std::path::Path,
    reverse_map: &'a HashMap<TreeNodeId, Vec<ReverseEdge>>,
    lockfile: &'a Lockfile,
    importer_info: &'a HashMap<String, ImporterInfo>,
    manifest_reader: &'a ManifestProjector<'a>,
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
    let manifest_reader = ManifestProjector::new(opts.manifest_fields, &resolved_nodes);

    let mut trees: Vec<DependentsTree> = Vec::new();
    for node_id in opts.graph.nodes.keys() {
        let TreeNodeId::Package(dep_path) = node_id else {
            continue;
        };
        if !lockfile.snapshots
            .as_ref()
            .is_some_and(|snapshots| snapshots.contains_key(dep_path))
        {
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

        let merge_id =
            package_merge_id(&opts.env.layout.lockfile_dir, lockfile, dep_path, &version);
        trees.push(DependentsTree {
            name,
            display_name: None,
            version,
            path: Some(resolved.path.to_string_lossy().into_owned()),
            peers_suffix_hash: peers_suffix_hash(dep_path),
            dependents: walk_dependents_of(opts, &reverse_map, &manifest_reader, node_id),
            search_message: matched.message().map(str::to_string),
            manifest: manifest_reader.project(node_id),
            merge_id,
        });
    }

    sort_trees(&mut trees);
    trees
}

fn walk_dependents_of(
    opts: &BuildDependentsOptions<'_>,
    reverse_map: &HashMap<TreeNodeId, Vec<ReverseEdge>>,
    manifest_reader: &ManifestProjector<'_>,
    node_id: &TreeNodeId,
) -> Vec<DependentNode> {
    let mut ctx = WalkCtx {
        lockfile_dir: &opts.env.layout.lockfile_dir,
        reverse_map,
        lockfile: opts.env.current_lockfile,
        importer_info: opts.importer_info,
        manifest_reader,
        visited: HashSet::from([node_id.clone()]),
        expanded: HashSet::new(),
    };
    walk_reverse(&mut ctx, node_id, 0)
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
                .push(ReverseEdge {
                    parent: parent_id.clone(),
                    alias: edge.alias.clone(),
                    kind: if node.peers.contains(&edge.alias) {
                        DependencyKind::Peer
                    } else {
                        DependencyKind::Dependency
                    },
                    importer_dependency: edge.importer_dependency.clone(),
                });
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
            .then_with(|| {
                a.parent
                    .serialize()
                    .cmp(&b.parent.serialize())
            })
    });

    let mut dependents: Vec<DependentNode> = Vec::new();
    for edge in sorted_edges {
        let node = if ctx.visited.contains(&edge.parent) {
            circular_node(ctx, edge)
        } else {
            expand_parent(ctx, edge, depth)
        };
        dependents.extend(node);
    }
    dependents
}

/// The parent is an ancestor of this position: report the cycle as a leaf
/// rather than descending into it again.
fn circular_node(ctx: &WalkCtx<'_>, edge: &ReverseEdge) -> Option<DependentNode> {
    match &edge.parent {
        TreeNodeId::Importer(importer_id) => {
            let info = ctx.importer_info.get(importer_id)?;
            let mut node = DependentNode::leaf(info.name.clone(), info.version.clone());
            node.requires = edge.importer_dependency
                .as_ref()
                .map(|dependency| dependency.specifier.clone());
            node.circular = true;
            node.importer_id = Some(importer_identity(ctx.lockfile_dir, importer_id));
            Some(node)
        }
        TreeNodeId::Package(dep_path) => {
            if !has_snapshot(ctx, dep_path) {
                return None;
            }
            let (name, version) = name_ver_from_dep_path(ctx.lockfile, dep_path);
            let merge_id = package_merge_id(ctx.lockfile_dir, ctx.lockfile, dep_path, &version);
            let mut node = DependentNode::leaf(name, version);
            node.requires =
                ctx.manifest_reader.dependency_specifier(&edge.parent, &edge.alias, edge.kind);
            node.circular = true;
            node.peers_suffix_hash = peers_suffix_hash(dep_path);
            node.merge_id = merge_id;
            node.manifest = ctx.manifest_reader.project(&edge.parent);
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
    let merge_id = package_merge_id(ctx.lockfile_dir, ctx.lockfile, dep_path, &version);
    let mut node = DependentNode::leaf(name, version);
    node.requires = ctx.manifest_reader.dependency_specifier(&edge.parent, &edge.alias, edge.kind);
    node.peers_suffix_hash = peers_suffix_hash(dep_path);
    node.merge_id = merge_id;
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
    if let Some(dependency) = &edge.importer_dependency {
        node.dep_field = Some(dependency.field);
        node.requires = Some(dependency.specifier.clone());
    }
    node.importer_id = Some(importer_identity(ctx.lockfile_dir, importer_id));
    node
}

fn importer_identity(lockfile_dir: &std::path::Path, importer_id: &str) -> String {
    crate::build::safe_importer_dir(lockfile_dir, importer_id)
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_else(|| importer_id.to_string())
}

fn resolve_parent_name(ctx: &WalkCtx<'_>, parent: &TreeNodeId) -> String {
    match parent {
        TreeNodeId::Importer(importer_id) => ctx.importer_info
            .get(importer_id)
            .map_or_else(|| importer_id.clone(), |info| info.name.clone()),
        TreeNodeId::Package(dep_path) => {
            if ctx.lockfile.snapshots
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

mod manifest;
mod selection;

use manifest::ManifestProjector;
use selection::{has_snapshot, match_package, package_merge_id};

#[cfg(test)]
mod tests;
