use super::{
    Arc,
    BTreeMap,
    ChildEdge,
    HashMap,
    HashSet,
    MissingSummary,
    NodeId,
    ResolvedTree,
};
use pnpm_strongly_connected_components::StronglyConnectedComponents;

/// The resolved peers an ancestor still has to satisfy: the ones this node
/// did not itself provide a child for.
pub(super) fn external_peers_to_report(
    all_resolved_peers: &HashMap<String, NodeId>,
    children_map: &BTreeMap<String, NodeId>,
    discovery_children: Option<&Arc<Vec<ChildEdge>>>,
) -> HashMap<String, NodeId> {
    all_resolved_peers
        .iter()
        .filter(|(peer_alias, _)| {
            !children_map.contains_key(peer_alias.as_str())
                && discovery_children.is_none_or(|children| {
                    !children
                        .iter()
                        .any(|edge| edge.alias == **peer_alias)
                })
        })
        .map(|(peer_alias, peer_node_id)| (peer_alias.clone(), peer_node_id.clone()))
        .collect()
}

/// The missing-peer names reported for one package by a walk. A
/// package's occurrences can appear in several subtree summaries, whose
/// reports are read as their union.
pub(crate) enum MissingNames<'a> {
    One(&'a HashSet<String>),
    Union(Vec<&'a HashSet<String>>),
}

impl<'a> MissingNames<'a> {
    pub(super) fn add(&mut self, names: &'a HashSet<String>) {
        match self {
            MissingNames::One(first) => *self = MissingNames::Union(vec![first, names]),
            MissingNames::Union(all) => all.push(names),
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        let (one, union) = match self {
            MissingNames::One(names) => (Some(*names), None),
            MissingNames::Union(all) => (None, Some(all)),
        };
        one.into_iter()
            .chain(union.into_iter().flatten().copied())
            .map(HashSet::iter)
            .flat_map(|value| value.map(String::as_str))
    }
}

/// Strongly-connected-component ids over the recorded children graph.
pub(crate) fn children_scc_ids(tree: &ResolvedTree) -> HashMap<Arc<str>, usize> {
    let (node_ids, adjacency) = children_graph_adjacency(tree);
    let components =
        StronglyConnectedComponents::compute(&adjacency, &vec![false; adjacency.len()]);
    node_ids
        .into_iter()
        .zip(
            components
                .component_ids()
                .iter()
                .copied(),
        )
        .collect()
}

/// The children graph as a dense adjacency list, alongside the package ids
/// the indices stand for.
pub(super) fn children_graph_adjacency(tree: &ResolvedTree) -> (Vec<Arc<str>>, Vec<Vec<usize>>) {
    let mut node_ids: Vec<Arc<str>> = Vec::new();
    let mut index_by_id: HashMap<Arc<str>, usize> = HashMap::default();
    let mut intern = |id: &Arc<str>, node_ids: &mut Vec<Arc<str>>| -> usize {
        if let Some(index) = index_by_id.get(id) {
            return *index;
        }
        let index = node_ids.len();
        node_ids.push(Arc::clone(id));
        index_by_id.insert(Arc::clone(id), index);
        index
    };
    let mut adjacency: Vec<Vec<usize>> = Vec::new();
    for (pkg_id, edges) in &tree.children_by_id {
        let node = intern(pkg_id, &mut node_ids);
        if adjacency.len() <= node {
            adjacency.resize_with(node + 1, Vec::new);
        }
        let targets: Vec<usize> = edges
            .iter()
            .map(|edge| intern(&edge.pkg_id, &mut node_ids))
            .collect();
        adjacency[node] = targets;
    }
    adjacency.resize_with(node_ids.len(), Vec::new);
    (node_ids, adjacency)
}

/// Index the per-package missing-peer names `roots` reported, borrowing
/// from the summaries rather than copying every descendant's names into
/// an owned map: one of these is built per importer per hoist round, so
/// a copy would make each round cost the whole workspace.
pub(crate) fn index_missing_names(
    roots: &[Arc<MissingSummary>],
) -> HashMap<&str, MissingNames<'_>> {
    let mut index: HashMap<&str, MissingNames<'_>> = HashMap::default();
    let mut seen: HashSet<usize> = HashSet::default();
    let mut pending: Vec<&Arc<MissingSummary>> = roots.iter().collect();
    while let Some(summary) = pending.pop() {
        if !seen.insert(Arc::as_ptr(summary) as usize) {
            continue;
        }
        if let Some((pkg_id, names)) = &summary.own {
            index
                .entry(pkg_id.as_str())
                .and_modify(|entry| entry.add(names))
                .or_insert(MissingNames::One(names));
        }
        pending.extend(summary.children.iter());
    }
    index
}
