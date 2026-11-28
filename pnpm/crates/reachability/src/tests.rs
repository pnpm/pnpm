use super::reaching;
use pretty_assertions::assert_eq;

/// Whether `node` reaches a flagged node, by a search from `node` alone.
fn reaches_by_search(adjacency: &[Vec<usize>], targets: &[bool], node: usize) -> bool {
    let mut seen = vec![false; adjacency.len()];
    let mut pending = vec![node];
    seen[node] = true;
    while let Some(current) = pending.pop() {
        if targets[current] {
            return true;
        }
        for &next in &adjacency[current] {
            if !seen[next] {
                seen[next] = true;
                pending.push(next);
            }
        }
    }
    false
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

#[test]
fn agrees_with_a_search_from_each_node() {
    let mut rng = Rng(2026);
    for graph in 0..50_000 {
        let count = 1 + rng.below(8);
        let adjacency: Vec<Vec<usize>> = (0..count)
            .map(|_| {
                (0..rng.below(4))
                    .map(|_| rng.below(count))
                    .collect()
            })
            .collect();
        let targets: Vec<bool> = (0..count)
            .map(|_| rng.below(4) == 0)
            .collect();
        let reaches = reaching(&adjacency, &targets);
        for (node, &reaches) in reaches.iter().enumerate() {
            assert_eq!(
                reaches,
                reaches_by_search(&adjacency, &targets, node),
                "graph {graph}: node {node} of {adjacency:?} with targets {targets:?}",
            );
        }
    }
}

#[test]
fn an_empty_graph_has_no_answers() {
    assert_eq!(reaching(&[], &[]), Vec::<bool>::new());
}

#[test]
fn a_long_chain_reaches_its_last_node() {
    let length = 200_000;
    let adjacency: Vec<Vec<usize>> = (0..length)
        .map(|node| if node + 1 < length { vec![node + 1] } else { Vec::new() })
        .collect();
    let mut targets = vec![false; length];
    targets[length - 1] = true;
    assert!(reaching(&adjacency, &targets).iter().all(|&reaches| reaches));
}
