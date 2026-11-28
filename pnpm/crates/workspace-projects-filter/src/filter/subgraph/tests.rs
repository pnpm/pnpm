use super::pick_subgraph;
use indexmap::IndexSet;
use std::path::{
    Path,
    PathBuf,
};

/// A depth-first walk that recurses once per node, which the walk is
/// checked against.
fn pick_by_recursion(
    adjacency: &[Vec<usize>],
    next_node_ids: &[usize],
    walked: &mut IndexSet<usize>,
    include_root: bool,
) {
    for &next_node_id in next_node_ids {
        if walked.contains(&next_node_id) {
            continue;
        }
        if include_root {
            walked.insert(next_node_id);
        }
        pick_by_recursion(adjacency, &adjacency[next_node_id], walked, true);
    }
}

fn path(node: usize) -> PathBuf {
    PathBuf::from(format!("/p{node}"))
}

fn node(path: &Path) -> usize {
    path.to_str().expect("a generated path")[2..].parse().expect("a generated node")
}

fn pick(adjacency: &[Vec<usize>], roots: &[usize], include_root: bool) -> Vec<usize> {
    let adj = |project: &Path| -> Option<Vec<PathBuf>> {
        let children = &adjacency[node(project)];
        (!children.is_empty()).then(|| {
            children
                .iter()
                .map(|&child| path(child))
                .collect()
        })
    };
    let roots: Vec<PathBuf> = roots
        .iter()
        .map(|&root| path(root))
        .collect();
    let mut walked = IndexSet::new();
    pick_subgraph(&adj, &roots, &mut walked, include_root);
    walked
        .iter()
        .map(|project| node(project))
        .collect()
}

/// A fixed linear congruential generator, so a failure names a graph that
/// reruns.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).expect("31 bits fit a usize") % bound
    }
}

/// The walk picks the same projects in the same order as a recursive
/// walk, with or without the roots, on generated graphs with cycles,
/// repeated edges and repeated roots.
#[test]
fn picks_in_the_order_a_recursive_walk_does() {
    let mut rng = Rng(2026);
    for graph in 0..20_000 {
        let count = 1 + rng.below(7);
        let adjacency: Vec<Vec<usize>> = (0..count)
            .map(|_| {
                (0..rng.below(4))
                    .map(|_| rng.below(count))
                    .collect()
            })
            .collect();
        let roots: Vec<usize> = (0..=rng.below(3))
            .map(|_| rng.below(count))
            .collect();
        let include_root = rng.below(2) == 0;
        let mut expected = IndexSet::new();
        pick_by_recursion(&adjacency, &roots, &mut expected, include_root);
        assert_eq!(
            pick(&adjacency, &roots, include_root),
            expected.into_iter().collect::<Vec<_>>(),
            "graph {graph}: {adjacency:?} from {roots:?}, include_root {include_root}",
        );
    }
}

#[test]
fn a_long_chain_of_projects_is_walked_without_recursion() {
    const LENGTH: usize = 100_000;
    let adjacency: Vec<Vec<usize>> = (0..LENGTH)
        .map(|node| if node + 1 < LENGTH { vec![node + 1] } else { Vec::new() })
        .collect();
    assert_eq!(pick(&adjacency, &[0], true), (0..LENGTH).collect::<Vec<_>>());
}
