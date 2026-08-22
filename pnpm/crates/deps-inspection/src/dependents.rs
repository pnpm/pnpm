//! Reverse (dependents) tree for `pnpm why`. Rust counterpart of the
//! TypeScript tree-builder's `buildDependentsTree`.

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
    search::Searcher,
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

        // Canonical name first, then aliases from incoming edges
        // (npm: protocol aliases).
        let mut matched = opts.search.matches(&name, &name, &version, Some(node_id));
        if !matched.is_match()
            && let Some(incoming) = reverse_map.get(node_id)
        {
            for edge in incoming {
                if edge.alias != name {
                    matched = opts.search.matches(&edge.alias, &name, &version, Some(node_id));
                    if matched.is_match() {
                        break;
                    }
                }
            }
        }
        if !matched.is_match() {
            continue;
        }

        let mut ctx = WalkCtx {
            reverse_map: &reverse_map,
            lockfile,
            importer_info: opts.importer_info,
            manifest_reader: ManifestProjector {
                fields: opts.manifest_fields,
                resolved: &resolved_nodes,
            },
            visited: HashSet::from([node_id.clone()]),
            expanded: HashSet::new(),
        };
        let dependents = walk_reverse(&mut ctx, node_id, 0);

        trees.push(DependentsTree {
            name,
            display_name: None,
            version,
            path: Some(resolved.path.to_string_lossy().into_owned()),
            peers_suffix_hash: peers_suffix_hash(dep_path),
            dependents,
            search_message: matched.message().map(str::to_string),
            manifest: ManifestProjector { fields: opts.manifest_fields, resolved: &resolved_nodes }
                .project(node_id),
        });
    }

    trees.sort_by(|a, b| {
        a.name.cmp(&b.name).then_with(|| compare_versions(&a.version, &b.version)).then_with(|| {
            a.peers_suffix_hash
                .as_deref()
                .unwrap_or("")
                .cmp(b.peers_suffix_hash.as_deref().unwrap_or(""))
        })
    });
    trees
}

/// The resolved filesystem location (and manifest source) of every
/// package node, found by walking the graph top-down from importers —
/// with a global virtual store the correct path is only reachable by
/// following symlinks through each parent's `node_modules`.
#[must_use]
pub fn resolve_package_nodes(
    env: &PkgInfoEnv<'_>,
    graph: &DependencyGraph,
) -> HashMap<TreeNodeId, ManifestSource> {
    let mut resolved: HashMap<TreeNodeId, ManifestSource> = HashMap::new();

    fn walk(
        env: &PkgInfoEnv<'_>,
        graph: &DependencyGraph,
        resolved: &mut HashMap<TreeNodeId, ManifestSource>,
        node_id: &TreeNodeId,
        parent_dir: Option<&Path>,
        depth: usize,
    ) {
        if depth >= super::MAX_WALK_DEPTH {
            return;
        }
        let Some(node) = graph.nodes.get(node_id) else {
            return;
        };
        for edge in &node.edges {
            let Some(target) = &edge.target else {
                continue;
            };
            if resolved.contains_key(target) || !matches!(target, TreeNodeId::Package(_)) {
                continue;
            }
            let edge_ctx = EdgeContext {
                peers: None,
                linked_path_base_dir: env.modules_dir.clone(),
                rewrite_link_version_dir: None,
                parent_dir: parent_dir.map(Path::to_path_buf),
            };
            let (_, manifest_source) = get_pkg_info(env, edge, &edge_ctx);
            let target_path = manifest_source.path.clone();
            resolved.insert(target.clone(), manifest_source);
            walk(env, graph, resolved, target, Some(&target_path), depth + 1);
        }
    }

    for node_id in graph.nodes.keys() {
        if matches!(node_id, TreeNodeId::Importer(_)) {
            walk(env, graph, &mut resolved, node_id, None, 0);
        }
    }
    resolved
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
        if ctx.visited.contains(&edge.parent) {
            match &edge.parent {
                TreeNodeId::Importer(importer_id) => {
                    if let Some(info) = ctx.importer_info.get(importer_id) {
                        let mut node = DependentNode::leaf(info.name.clone(), info.version.clone());
                        node.circular = true;
                        dependents.push(node);
                    }
                }
                TreeNodeId::Package(dep_path) => {
                    if ctx
                        .lockfile
                        .snapshots
                        .as_ref()
                        .is_some_and(|snapshots| snapshots.contains_key(dep_path))
                    {
                        let (name, version) = name_ver_from_dep_path(ctx.lockfile, dep_path);
                        let mut node = DependentNode::leaf(name, version);
                        node.circular = true;
                        node.manifest = ctx.manifest_reader.project(&edge.parent);
                        dependents.push(node);
                    }
                }
            }
            continue;
        }

        match &edge.parent {
            TreeNodeId::Importer(importer_id) => {
                let (name, version) = match ctx.importer_info.get(importer_id) {
                    Some(info) => (info.name.clone(), info.version.clone()),
                    None => (importer_id.clone(), String::new()),
                };
                let mut node = DependentNode::leaf(name, version);
                node.dep_field = ctx
                    .lockfile
                    .importers
                    .get(importer_id.as_str())
                    .and_then(|importer| dep_field_for_alias(&edge.alias, importer));
                dependents.push(node);
            }
            TreeNodeId::Package(dep_path) => {
                if !ctx
                    .lockfile
                    .snapshots
                    .as_ref()
                    .is_some_and(|snapshots| snapshots.contains_key(dep_path))
                {
                    continue;
                }
                let (name, version) = name_ver_from_dep_path(ctx.lockfile, dep_path);
                let hash = peers_suffix_hash(dep_path);

                if ctx.expanded.contains(&edge.parent) {
                    // Already expanded elsewhere in the tree — show as
                    // a leaf to keep the output bounded.
                    let mut node = DependentNode::leaf(name, version);
                    node.peers_suffix_hash = hash;
                    node.deduped = true;
                    node.manifest = ctx.manifest_reader.project(&edge.parent);
                    dependents.push(node);
                    continue;
                }

                ctx.visited.insert(edge.parent.clone());
                ctx.expanded.insert(edge.parent.clone());
                let child_dependents = walk_reverse(ctx, &edge.parent, depth + 1);
                ctx.visited.remove(&edge.parent);

                let mut node = DependentNode::leaf(name, version);
                node.peers_suffix_hash = hash;
                node.dependents =
                    if child_dependents.is_empty() { None } else { Some(child_dependents) };
                node.manifest = ctx.manifest_reader.project(&edge.parent);
                dependents.push(node);
            }
        }
    }

    dependents
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

/// Name and display version of a depPath, preferring the `version:`
/// recorded in the `packages:` entry (git/tarball deps) over the
/// version encoded in the depPath.
#[must_use]
pub fn name_ver_from_dep_path(lockfile: &Lockfile, dep_path: &PkgNameVerPeer) -> (String, String) {
    let version = lockfile
        .packages
        .as_ref()
        .and_then(|packages| packages.get(&dep_path.without_peer()))
        .and_then(|metadata| metadata.version.clone())
        .unwrap_or_else(|| dep_path.suffix.version().to_string());
    (dep_path.name.to_string(), version)
}

/// `semver.compare` when both versions parse as semver, lexicographic
/// otherwise — the tree ordering the TypeScript CLI uses.
#[must_use]
pub fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}
