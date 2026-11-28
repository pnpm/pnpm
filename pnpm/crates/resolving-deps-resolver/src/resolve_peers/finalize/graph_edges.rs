use super::{
    BTreeMap,
    DepPath,
    DependenciesGraph,
    DependenciesGraphNode,
    HashMap,
    HashSet,
    MissingPeerInfo,
    NodeId,
    ResolveResult,
    ResolvedPackage,
    peer_segment_names,
    pkg_name_version,
};

/// Records arrive in walk order, with their shared minimum depth already set.
/// The first supplies the payload; later records contribute edges and peers.
pub(super) fn insert_graph_node(
    graph: &mut DependenciesGraph,
    candidate: DependenciesGraphNode,
    transitive_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) {
    match graph.entry(candidate.dep_path.clone()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            merge_additional_edges(entry.get_mut(), candidate, transitive_by_dep_path);
        }
    }
}

/// The peers visible in a node's subtree that its own manifest does not
/// declare.
pub(super) fn transitive_peer_names(
    pkg: &ResolvedPackage,
    all_resolved_peers: &HashMap<String, NodeId>,
    all_missing_peers: &HashMap<String, MissingPeerInfo>,
) -> HashSet<String> {
    all_resolved_peers
        .keys()
        .chain(all_missing_peers.keys())
        .filter(|peer_alias| !pkg.peer_set.taken().contains_key(peer_alias.as_str()))
        .cloned()
        .collect()
}

pub(super) fn merge_preferred_child_edges(
    target: &mut DependenciesGraphNode,
    children: BTreeMap<String, DepPath>,
    transitive_peer_dependencies_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) {
    let available_peer_names =
        available_peer_names_for_dep_path(&target.dep_path, &target.resolve_result);
    for (alias, candidate_dep_path) in children {
        match target.edges.children.entry(alias) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidate_dep_path);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let preferred = child_dep_path_is_preferred(
                    entry.get(),
                    &candidate_dep_path,
                    &available_peer_names,
                    transitive_peer_dependencies_by_dep_path,
                );
                if preferred {
                    entry.insert(candidate_dep_path);
                }
            }
        }
    }
}

pub(super) fn available_peer_names_for_dep_path(
    dep_path: &DepPath,
    resolve_result: &ResolveResult,
) -> HashSet<String> {
    let mut names: HashSet<String> = peer_segment_names(dep_path)
        .unwrap_or_default()
        .into_iter()
        .collect();
    names.insert(pkg_name_version(resolve_result).0);
    names
}

pub(super) fn child_dep_path_is_preferred(
    current: &DepPath,
    candidate: &DepPath,
    available_peer_names: &HashSet<String>,
    transitive_peer_dependencies_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) -> bool {
    if current == candidate {
        return false;
    }
    let current_unavailable = unavailable_non_transitive_peer_segment_names(
        current,
        available_peer_names,
        transitive_peer_dependencies_by_dep_path,
    )
    .unwrap_or_default();
    let candidate_unavailable = unavailable_non_transitive_peer_segment_names(
        candidate,
        available_peer_names,
        transitive_peer_dependencies_by_dep_path,
    )
    .unwrap_or_default();
    if candidate_unavailable.len() != current_unavailable.len() {
        return candidate_unavailable.len() < current_unavailable.len();
    }
    if !candidate_unavailable.is_empty() {
        return false;
    }
    let current_peer_count =
        available_peer_segment_count(current, available_peer_names).unwrap_or(0);
    let candidate_peer_count =
        available_peer_segment_count(candidate, available_peer_names).unwrap_or(0);
    candidate_peer_count > current_peer_count
}

pub(super) fn available_peer_segment_count(
    dep_path: &DepPath,
    available_peer_names: &HashSet<String>,
) -> Option<usize> {
    let names = peer_segment_names(dep_path)?;
    Some(
        names
            .into_iter()
            .filter(|name| available_peer_names.contains(name))
            .count(),
    )
}

pub(super) fn unavailable_non_transitive_peer_segment_names(
    dep_path: &DepPath,
    available_peer_names: &HashSet<String>,
    transitive_peer_dependencies_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) -> Option<Vec<String>> {
    let transitive_peer_dependencies = transitive_peer_dependencies_by_dep_path.get(dep_path);
    let names = peer_segment_names(dep_path)?;
    Some(
        names
            .into_iter()
            .filter(|name| {
                !available_peer_names.contains(name)
                    && transitive_peer_dependencies.is_none_or(|transitive| {
                        !transitive.contains(name)
                    })
            })
            .collect(),
    )
}

pub(crate) fn merge_additional_edges(
    existing: &mut DependenciesGraphNode,
    candidate: DependenciesGraphNode,
    transitive_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) {
    existing.edges.transitive_peer_dependencies.extend(
        candidate.edges.transitive_peer_dependencies,
    );
    existing.edges.optional_children.extend(candidate.edges.optional_children);
    merge_preferred_child_edges(existing, candidate.edges.children, transitive_by_dep_path);
}
