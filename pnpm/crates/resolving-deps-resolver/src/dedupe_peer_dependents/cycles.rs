use super::{
    CollapseGuard, DepPath, DependenciesGraph, HashMap, HashSet, is_compatible_and_has_more_deps,
    sort_variants,
};
use crate::strongly_connected_components::SccPass;

struct Component<'a> {
    groups: Vec<&'a Vec<DepPath>>,
    dependencies: HashSet<usize>,
    cyclic: bool,
}

// Break one pair per independent cycle, deferring parents of changed components
// until their children have settled in the next ordinary round.
pub(super) fn collapse_cycles(
    graph: &DependenciesGraph,
    duplicates: &[Vec<DepPath>],
    guard: CollapseGuard<'_>,
) -> HashMap<DepPath, DepPath> {
    let components = cycle_components(graph, duplicates);
    let mut changed = vec![false; components.len()];
    let mut replacements = HashMap::default();
    for (index, component) in components.iter().enumerate() {
        if component.dependencies
            .iter()
            .any(|dependency| changed[*dependency])
        {
            changed[index] = true;
            continue;
        }
        if !component.cyclic {
            continue;
        }
        if let Some((candidate, target)) =
            component.groups.iter().find_map(|group| compatible_pair(graph, group, guard))
        {
            replacements.insert(candidate, target);
            changed[index] = true;
        }
    }
    replacements
}

fn compatible_pair(
    graph: &DependenciesGraph,
    group: &[DepPath],
    guard: CollapseGuard<'_>,
) -> Option<(DepPath, DepPath)> {
    let mut paths = group.to_vec();
    sort_variants(graph, &mut paths);
    while let Some(largest) = paths.pop() {
        let candidate = paths
            .iter()
            .rev()
            .find(|candidate| is_compatible_and_has_more_deps(graph, &largest, candidate, guard));
        if let Some(candidate) = candidate {
            return Some((candidate.clone(), largest));
        }
    }
    None
}

fn cycle_components<'a>(
    graph: &DependenciesGraph,
    duplicates: &'a [Vec<DepPath>],
) -> Vec<Component<'a>> {
    let by_id: HashMap<&str, usize> = duplicates
        .iter()
        .enumerate()
        .filter(|(_, group)| !group.is_empty())
        .map(|(index, group)| (graph[&group[0]].resolved_package_id.as_str(), index))
        .collect();
    let adjacency: Vec<_> = duplicates
        .iter()
        .map(|group| group_dependencies(graph, group, &by_id))
        .collect();
    let mut components = SccPass::default();
    for root in 0..duplicates.len() {
        if !components.index_of.contains_key(&root) {
            components.visit_root(&root, &|index| adjacency[*index].clone());
        }
    }
    components.sccs
        .iter()
        .enumerate()
        .map(|(index, nodes)| component(index, nodes, &components.scc_of, &adjacency, duplicates))
        .collect()
}

fn component<'a>(
    index: usize,
    nodes: &[usize],
    component_of: &HashMap<usize, usize>,
    adjacency: &[Vec<usize>],
    duplicates: &'a [Vec<DepPath>],
) -> Component<'a> {
    let mut nodes = nodes.to_vec();
    nodes.sort_unstable();
    let cyclic = nodes.len() > 1 || adjacency[nodes[0]].contains(&nodes[0]);
    let mut dependencies = HashSet::default();
    for node in &nodes {
        for child in &adjacency[*node] {
            let child_component = component_of[child];
            if child_component != index {
                dependencies.insert(child_component);
            }
        }
    }
    Component {
        groups: nodes
            .iter()
            .map(|node| &duplicates[*node])
            .collect(),
        dependencies,
        cyclic,
    }
}

fn group_dependencies(
    graph: &DependenciesGraph,
    group: &[DepPath],
    by_id: &HashMap<&str, usize>,
) -> Vec<usize> {
    let mut dependencies = HashSet::default();
    for variant in group {
        for child in graph[variant].edges.children.values() {
            if let Some(node) = graph.get(child)
                && let Some(index) = by_id.get(node.resolved_package_id.as_str())
            {
                dependencies.insert(*index);
            }
        }
    }
    let mut dependencies: Vec<_> = dependencies.into_iter().collect();
    dependencies.sort_unstable();
    dependencies
}
