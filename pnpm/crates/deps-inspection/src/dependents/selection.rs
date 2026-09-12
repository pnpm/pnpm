use super::{
    BuildDependentsOptions, DependencyGraph, EdgeContext, HashMap, Lockfile, ManifestSource, Path,
    PkgInfoEnv, PkgNameVerPeer, ReverseEdge, SearchMatch, TreeNodeId, WalkCtx, get_pkg_info,
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
    let dependents = reverse_map.get(node_id).into_iter().flatten();
    let mut attempts = dependents
        .filter(|edge| edge.alias != name)
        .map(|edge| opts.search.matches(&edge.alias, name, version, Some(node_id)));
    attempts.find(SearchMatch::is_match).unwrap_or(matched)
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

pub(super) fn has_snapshot(ctx: &WalkCtx<'_>, dep_path: &PkgNameVerPeer) -> bool {
    ctx.lockfile.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(dep_path))
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
