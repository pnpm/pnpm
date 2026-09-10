use super::{
    BTreeMap, BTreeSet, DepPath, DependenciesGraph, DependenciesGraphNode, HashMap, HashSet,
    MissingPeerInfo, NodeId, ResolveResult, ResolvedPackage, peer_segment_names, pkg_name_version,
};

/// Merge one record's graph node into the depPath-keyed graph. Records that
/// share a depPath collapse onto one entry, the shallowest — and among equals
/// the earliest walked — one supplying the node's own fields while the rest
/// still contribute their peers, optional children and child edges.
pub(super) fn insert_graph_node(
    graph: &mut DependenciesGraph,
    graph_order: &mut HashMap<DepPath, u64>,
    mut candidate: DependenciesGraphNode,
    order: u64,
    transitive_by_dep_path: &HashMap<DepPath, HashSet<String>>,
) {
    let dep_path = candidate.dep_path.clone();
    match graph.entry(dep_path.clone()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            graph_order.insert(dep_path, order);
            entry.insert(candidate);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            let existing = entry.get();
            let existing_order = graph_order.get(&dep_path).copied().unwrap_or(order);
            let replace = candidate.depth < existing.depth
                || (candidate.depth == existing.depth && order < existing_order);
            if !replace {
                let existing = entry.get_mut();
                existing
                    .transitive_peer_dependencies
                    .extend(candidate.transitive_peer_dependencies);
                existing.optional_children.extend(candidate.optional_children);
                merge_preferred_child_edges(existing, candidate.children, transitive_by_dep_path);
                return;
            }
            candidate
                .transitive_peer_dependencies
                .extend(existing.transitive_peer_dependencies.iter().cloned());
            candidate.optional_children.extend(existing.optional_children.iter().cloned());
            merge_preferred_child_edges(
                &mut candidate,
                existing.children.clone(),
                transitive_by_dep_path,
            );
            graph_order.insert(dep_path, order);
            entry.insert(candidate);
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
        .filter(|peer_alias| !pkg.peer_dependencies.contains_key(peer_alias.as_str()))
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
        match target.children.entry(alias) {
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
    let mut names: HashSet<String> =
        peer_segment_names(dep_path).unwrap_or_default().into_iter().collect();
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
    Some(names.into_iter().filter(|name| available_peer_names.contains(name)).count())
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
                    && transitive_peer_dependencies
                        .is_none_or(|transitive| !transitive.contains(name))
            })
            .collect(),
    )
}

pub(super) struct PeerNameTarjan<'a> {
    pub(super) graph: &'a BTreeMap<String, BTreeSet<&'a str>>,
    pub(super) index_of: HashMap<&'a str, u32>,
    pub(super) low_of: HashMap<&'a str, u32>,
    pub(super) on_stack: HashSet<&'a str>,
    pub(super) tarjan_stack: Vec<&'a str>,
    pub(super) cyclic: HashSet<String>,
    pub(super) next_index: u32,
}

impl<'a> PeerNameTarjan<'a> {
    pub(super) fn strongconnect(&mut self, name: &'a str) {
        self.index_of.insert(name, self.next_index);
        self.low_of.insert(name, self.next_index);
        self.next_index += 1;
        self.on_stack.insert(name);
        self.tarjan_stack.push(name);
        self.visit_neighbors(name);
        self.close_component(name);
    }

    pub(super) fn visit_neighbors(&mut self, name: &'a str) {
        let Some(neighbors) = self.graph.get(name) else { return };
        for child in neighbors {
            if !self.index_of.contains_key(child) {
                self.strongconnect(child);
                let name_low = self.low_of[name];
                let child_low = self.low_of[child];
                self.low_of.insert(name, name_low.min(child_low));
            } else if self.on_stack.contains(child) {
                let name_low = self.low_of[name];
                let child_index = self.index_of[child];
                self.low_of.insert(name, name_low.min(child_index));
            }
        }
    }

    /// A component is cyclic when more than one name takes part in
    /// it, or when its single name depends on itself.
    pub(super) fn close_component(&mut self, name: &'a str) {
        if self.low_of[name] != self.index_of[name] {
            return;
        }
        let mut component = Vec::new();
        while let Some(member) = self.tarjan_stack.pop() {
            self.on_stack.remove(&member);
            let is_root = member == name;
            component.push(member);
            if is_root {
                break;
            }
        }
        let self_loop = component.first().is_some_and(|member| {
            self.graph.get(*member).is_some_and(|edges| edges.contains(member))
        });
        if component.len() > 1 || self_loop {
            self.cyclic.extend(component.into_iter().map(str::to_owned));
        }
    }
}
