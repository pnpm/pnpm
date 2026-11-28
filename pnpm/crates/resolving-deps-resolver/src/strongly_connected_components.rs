use pnpm_strongly_connected_components::StronglyConnectedComponents;
use rustc_hash::FxHashMap as HashMap;
use std::hash::Hash;

/// The strongly connected components of the graph `neighbors` draws over
/// `nodes`, walked from each node in turn: the components in the order the
/// walk closes them, so each comes after every component it has an edge to,
/// and the component of each node. `neighbors` names members of `nodes`
/// only.
pub(crate) fn components_of<Node>(
    nodes: &[Node],
    neighbors: impl Fn(&Node) -> Vec<Node>,
) -> (Vec<Vec<Node>>, HashMap<Node, usize>)
where
    Node: Clone + Eq + Hash,
{
    let mut index_of: HashMap<Node, usize> = HashMap::default();
    let mut distinct: Vec<&Node> = Vec::new();
    for node in nodes {
        index_of
            .entry(node.clone())
            .or_insert_with(|| {
                distinct.push(node);
                distinct.len() - 1
            });
    }
    let adjacency: Vec<Vec<usize>> = distinct
        .iter()
        .map(|node| {
            neighbors(node)
                .iter()
                .map(|next| index_of[next])
                .collect()
        })
        .collect();
    let found = StronglyConnectedComponents::compute(&adjacency, &vec![false; adjacency.len()]);
    let components = found
        .components()
        .iter()
        .map(|members| {
            members
                .iter()
                .map(|&member| distinct[member].clone())
                .collect()
        })
        .collect();
    let component_of = distinct
        .iter()
        .zip(found.component_ids())
        .map(|(node, &component)| ((*node).clone(), component))
        .collect();
    (components, component_of)
}
