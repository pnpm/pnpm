use super::{
    BuildDependentsOptions, DepField, DependencyGraph, DependentNode, DependentsTree, EdgeContext,
    HashMap, Lockfile, ManifestSource, PkgInfoEnv, PkgNameVerPeer, ReverseEdge, SearchMatch,
    TreeNodeId, WalkCtx, get_pkg_info,
};

/// Match the search against the package's canonical name first, then against
/// the aliases its incoming edges give it (`npm:` protocol aliases).
pub(super) fn match_package(
    opts: &BuildDependentsOptions<'_>,
    reverse_map: &HashMap<TreeNodeId, Vec<ReverseEdge>>,
    node_id: &TreeNodeId,
    name: &str,
    version: &str,
) -> SearchMatch {
    let matched = opts.search.matches(name, name, version, Some(node_id));
    if matched.is_match() {
        return matched;
    }
    reverse_map
        .get(node_id)
        .into_iter()
        .flatten()
        .filter(|edge| edge.alias != name)
        .map(|edge| opts.search.matches(&edge.alias, name, version, Some(node_id)))
        .find(SearchMatch::is_match)
        .unwrap_or(matched)
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
        edge_ctx: &EdgeContext<'_>,
        depth: usize,
    ) {
        if depth >= super::super::MAX_WALK_DEPTH {
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
            let (_, manifest_source) = get_pkg_info(env, edge, edge_ctx);
            let child_ctx = EdgeContext {
                peers: None,
                linked_path_base_dir: edge_ctx.linked_path_base_dir.clone(),
                rewrite_link_version_dir: None,
                parent_dir: Some(manifest_source.path.clone()),
            };
            resolved.insert(target.clone(), manifest_source);
            walk(env, graph, resolved, target, &child_ctx, depth + 1);
        }
    }

    for node_id in graph.nodes.keys() {
        if let TreeNodeId::Importer(importer_id) = node_id {
            walk(env, graph, &mut resolved, node_id, &importer_edge_ctx(env, importer_id), 0);
        }
    }
    resolved
}

/// The context of an importer's direct dependencies: hoisted copies
/// are resolved from the importer's own directory.
fn importer_edge_ctx(env: &PkgInfoEnv<'_>, importer_id: &str) -> EdgeContext<'static> {
    EdgeContext {
        peers: None,
        linked_path_base_dir: crate::build::safe_importer_dir(
            &env.layout.lockfile_dir,
            importer_id,
        )
        .unwrap_or_else(|| env.layout.lockfile_dir.clone()),
        rewrite_link_version_dir: None,
        parent_dir: None,
    }
}

pub(super) fn has_snapshot(ctx: &WalkCtx<'_>, dep_path: &PkgNameVerPeer) -> bool {
    ctx.lockfile.snapshots
        .as_ref()
        .is_some_and(|snapshots| snapshots.contains_key(dep_path))
}

/// Name and display version of a depPath, preferring the `version:`
/// recorded in the `packages:` entry (git/tarball deps) over the
/// version encoded in the depPath.
#[must_use]
pub fn name_ver_from_dep_path(lockfile: &Lockfile, dep_path: &PkgNameVerPeer) -> (String, String) {
    let version = lockfile.packages
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

pub fn sort_trees(trees: &mut [DependentsTree]) {
    trees.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| compare_versions(&a.version, &b.version))
            .then_with(|| {
                a.peers_suffix_hash
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.peers_suffix_hash.as_deref().unwrap_or(""))
            })
    });
}

/// Merge reverse dependency trees collected from dedicated per-project
/// lockfiles so a package version shared across projects renders once
/// with all of its dependents.
#[must_use]
pub fn merge_trees(trees: Vec<DependentsTree>) -> Vec<DependentsTree> {
    let mut merged: Vec<DependentsTree> = Vec::new();
    for (tree_index, mut tree) in trees.into_iter().enumerate() {
        tag_tree_dependents(&mut tree.dependents, tree.path.as_deref(), tree_index);
        match merged.iter_mut().find(|existing| same_tree_key(existing, &tree)) {
            Some(existing) => merge_into_tree(existing, tree),
            None => merged.push(tree),
        }
    }
    sort_trees(&mut merged);
    merged
}

fn tag_tree_dependents(nodes: &mut [DependentNode], root_path: Option<&str>, tree_index: usize) {
    for node in nodes {
        if node.root_paths.is_empty()
            && let Some(path) = root_path
        {
            node.root_paths.push(path.to_string());
        }
        if node.dep_field.is_some() && node.importer_id.is_none() {
            node.importer_id = Some(format!("tree:{tree_index}"));
        }
        if let Some(children) = &mut node.dependents {
            tag_tree_dependents(children, root_path, tree_index);
        }
    }
}

fn same_tree_key(left: &DependentsTree, right: &DependentsTree) -> bool {
    left.name == right.name
        && left.version == right.version
        && left.peers_suffix_hash == right.peers_suffix_hash
}

fn merge_into_tree(target: &mut DependentsTree, source: DependentsTree) {
    target.display_name = target.display_name.take().or(source.display_name);
    target.path = target.path.take().or(source.path);
    target.search_message = target.search_message.take().or(source.search_message);
    target.manifest = target.manifest.take().or(source.manifest);
    merge_dependent_nodes(&mut target.dependents, source.dependents);
}

fn merge_dependent_nodes(target: &mut Vec<DependentNode>, source: Vec<DependentNode>) {
    for node in source {
        match target.iter_mut().find(|existing| same_node_key(existing, &node)) {
            Some(existing) => merge_into_node(existing, node),
            None => target.push(node),
        }
    }
    target.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| compare_versions(&a.version, &b.version))
            .then_with(|| {
                a.peers_suffix_hash
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.peers_suffix_hash.as_deref().unwrap_or(""))
            })
            .then_with(|| {
                a.dep_field
                    .map(DepField::as_str)
                    .unwrap_or("")
                    .cmp(b.dep_field.map(DepField::as_str).unwrap_or(""))
            })
            .then_with(|| {
                a.importer_id
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.importer_id.as_deref().unwrap_or(""))
            })
    });
}

fn same_node_key(left: &DependentNode, right: &DependentNode) -> bool {
    left.name == right.name
        && left.version == right.version
        && left.peers_suffix_hash == right.peers_suffix_hash
        && left.dep_field == right.dep_field
        && left.circular == right.circular
        && left.importer_id == right.importer_id
}

fn merge_into_node(target: &mut DependentNode, source: DependentNode) {
    target.display_name = target.display_name.take().or(source.display_name);
    target.manifest = target.manifest.take().or(source.manifest);
    for path in source.root_paths {
        if !target.root_paths.contains(&path) {
            target.root_paths.push(path);
        }
    }
    if let Some(node_deps) = source.dependents {
        merge_dependent_nodes(target.dependents.get_or_insert_with(Vec::new), node_deps);
        target.deduped = false;
    } else if target.dependents.is_some() {
        target.deduped = false;
    }
}
