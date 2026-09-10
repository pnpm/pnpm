use super::{
    Arc, DependenciesTreeNode, DirectDep, HashMap, HashSet, NodeId, RecordedChildren,
    ResolvedPackage, ResolvedTree, RunVersionsCache,
};

/// Fold one `name → version` pair into `versions` as a plain
/// `version` selector; the first fold of a pair wins over later ones.
pub(super) fn fold_version(
    versions: &mut pnpm_resolving_resolver_base::PreferredVersions,
    name: String,
    version: String,
) {
    versions.entry(name).or_default().entry(version).or_insert(
        pnpm_resolving_resolver_base::VersionSelectorEntry::Plain(
            pnpm_resolving_resolver_base::VersionSelectorType::Version,
        ),
    );
}

/// `false` when the recorded spec disagrees with the one already synced.
pub(super) fn merge_synced_child_spec(
    tree: &mut ResolvedTree,
    pkg_id: &str,
    spec: &Arc<Vec<crate::resolved_tree::ChildEdge>>,
) -> bool {
    use std::collections::hash_map::Entry;
    match tree.children_by_id.entry(Arc::from(pkg_id)) {
        Entry::Vacant(entry) => {
            entry.insert(Arc::clone(spec));
        }
        Entry::Occupied(mut entry) => {
            if Arc::ptr_eq(entry.get(), spec) {
                return true;
            }
            if **entry.get() != **spec {
                return false;
            }
            entry.insert(Arc::clone(spec));
        }
    }
    true
}

/// Drain `queue` through the children graph, marking each package visited and
/// queueing its not-yet-visited children. Returns the packages this pass
/// newly reached.
pub(super) fn collect_newly_visited(
    children_by_id: &HashMap<Arc<str>, RecordedChildren>,
    queue: &mut Vec<String>,
    visited: &mut HashSet<String>,
) -> Vec<String> {
    let mut newly_visited: Vec<String> = Vec::new();
    while let Some(pkg_id) = queue.pop() {
        if !visited.insert(pkg_id.clone()) {
            continue;
        }
        push_unvisited_children(children_by_id, &pkg_id, queue, visited);
        newly_visited.push(pkg_id);
    }
    newly_visited
}

pub(super) fn push_unvisited_children(
    children_by_id: &HashMap<Arc<str>, RecordedChildren>,
    pkg_id: &str,
    queue: &mut Vec<String>,
    visited: &HashSet<String>,
) {
    let Some(children) = children_by_id.get(pkg_id) else { return };
    for child in children.edges.iter() {
        if !visited.contains(&*child.pkg_id) {
            queue.push(child.pkg_id.to_string());
        }
    }
}

/// Fold each newly reached package's resolved version into the run's
/// preferred versions, deferring the ones whose identity is not known yet.
pub(super) fn fold_visited_versions(
    packages: &HashMap<Arc<str>, ResolvedPackage>,
    newly_visited: Vec<String>,
    cache: &mut RunVersionsCache,
) {
    for pkg_id in newly_visited {
        match packages.get(pkg_id.as_str()).and_then(|pkg| pkg.result.name_ver.as_ref()) {
            Some(name_ver) => fold_version(
                &mut cache.versions,
                name_ver.name.to_string(),
                name_ver.suffix.to_string(),
            ),
            None => {
                cache.awaiting_identity.insert(pkg_id);
            }
        }
    }
}

/// Every occurrence node reachable from `direct` through realized children,
/// and the package ids those nodes resolved to.
pub(super) fn walk_reachable_nodes(
    dependencies_tree: &HashMap<NodeId, DependenciesTreeNode>,
    direct: &[DirectDep],
) -> (HashSet<NodeId>, HashSet<Arc<str>>) {
    let mut reachable_node_ids = HashSet::default();
    let mut reachable_pkg_ids = HashSet::default();
    let mut pending_node_ids: Vec<NodeId> = direct.iter().map(|dep| dep.node_id.clone()).collect();
    while let Some(node_id) = pending_node_ids.pop() {
        if !reachable_node_ids.insert(node_id.clone()) {
            continue;
        }
        let Some(node) = dependencies_tree.get(&node_id) else {
            continue;
        };
        reachable_pkg_ids.insert(Arc::<str>::clone(&node.resolved_package_id));
        if let crate::resolved_tree::TreeChildren::Realized(children) = &node.children {
            pending_node_ids.extend(children.values().cloned());
        }
    }
    (reachable_node_ids, reachable_pkg_ids)
}

/// The per-package child edges reachable from `reachable_pkg_ids`, which grows
/// to the transitive closure as the walk proceeds.
pub(super) fn walk_reachable_children(
    all_children: &HashMap<Arc<str>, RecordedChildren>,
    reachable_pkg_ids: &mut HashSet<Arc<str>>,
) -> HashMap<Arc<str>, Arc<Vec<crate::resolved_tree::ChildEdge>>> {
    let mut pending_pkg_ids: Vec<Arc<str>> = reachable_pkg_ids.iter().cloned().collect();
    let mut children_by_id = HashMap::default();
    while let Some(pkg_id) = pending_pkg_ids.pop() {
        let Some(children) = all_children.get(&pkg_id) else {
            continue;
        };
        children_by_id.insert(pkg_id, Arc::clone(&children.edges));
        for child in children.edges.iter() {
            if reachable_pkg_ids.insert(Arc::<str>::clone(&child.pkg_id)) {
                pending_pkg_ids.push(Arc::<str>::clone(&child.pkg_id));
            }
        }
    }
    children_by_id
}
