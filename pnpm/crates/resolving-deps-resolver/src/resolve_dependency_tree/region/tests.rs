use super::{
    Inspected,
    decide,
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

/// A package's state as [`decide`] inspects it: memoised with an answer,
/// unfit itself, or fit itself with these children.
enum Package {
    Memoised(bool),
    Unfit,
    Fit(Vec<usize>),
}

fn inspect(graph: &[Package]) -> impl FnMut(&usize) -> Inspected<usize> + '_ {
    |&node| match &graph[node] {
        Package::Memoised(answer) => Inspected::Settled(*answer),
        Package::Unfit => Inspected::Settled(false),
        Package::Fit(children) => Inspected::Children(children.clone()),
    }
}

fn answers(graph: &[Package], root: usize) -> BTreeMap<usize, bool> {
    decide(&root, inspect(graph)).into_iter().collect()
}

/// `true` when no package `node` reaches, through packages that are not
/// memoised, is unfit or memoised `false`.
fn expected(graph: &[Package], node: usize) -> bool {
    let mut seen = vec![false; graph.len()];
    let mut pending = vec![node];
    seen[node] = true;
    while let Some(current) = pending.pop() {
        let children = match &graph[current] {
            Package::Memoised(true) => continue,
            Package::Memoised(false) | Package::Unfit => return false,
            Package::Fit(children) => children,
        };
        for &child in children {
            if !seen[child] {
                seen[child] = true;
                pending.push(child);
            }
        }
    }
    true
}

/// `0` depends on `1` and then on `2`, and `1` back on `0`. A walk that
/// answers `1` while `0` is still under inspection, by taking the back edge
/// as `true`, answers `1` `true` before `0`'s edge to the unfit `2` is seen.
#[test]
fn a_cycle_member_found_before_an_unfit_package_is_answered_false() {
    let graph = [Package::Fit(vec![1, 2]), Package::Fit(vec![0]), Package::Unfit];
    for root in 0..2 {
        assert_eq!(
            answers(&graph, root),
            BTreeMap::from([(0, false), (1, false), (2, false)]),
            "root {root}",
        );
    }
}

#[test]
fn a_cycle_of_fit_packages_is_answered_true() {
    let graph = [Package::Fit(vec![1]), Package::Fit(vec![0])];
    assert_eq!(answers(&graph, 0), BTreeMap::from([(0, true), (1, true)]));
}

/// A memoised package's children are not visited, so `inspect` never
/// meets `2`, and the root's answer is the memoised one's.
#[test]
fn a_memoised_package_stops_the_walk() {
    let graph = [Package::Fit(vec![1]), Package::Memoised(true), Package::Unfit];
    let mut inspected = Vec::new();
    let mut inspect_graph = inspect(&graph);
    let answers = decide(&0, |node| {
        inspected.push(*node);
        inspect_graph(node)
    });
    assert_eq!(answers, vec![(0, true), (1, true)]);
    assert_eq!(inspected, vec![0, 1]);
}

/// `decide` inspects each package once, and answers the root first.
#[test]
fn each_package_is_inspected_once() {
    let graph = [Package::Fit(vec![1, 2, 1]), Package::Fit(vec![2, 0]), Package::Fit(vec![1])];
    let mut inspected = Vec::new();
    let mut inspect_graph = inspect(&graph);
    let answers = decide(&0, |node| {
        inspected.push(*node);
        inspect_graph(node)
    });
    inspected.sort_unstable();
    assert_eq!(inspected, vec![0, 1, 2]);
    assert_eq!(answers[0], (0, true));
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

    fn package(&mut self, count: usize) -> Package {
        match self.below(10) {
            0 => Package::Unfit,
            1 => Package::Memoised(self.below(2) == 0),
            _ => Package::Fit(
                (0..self.below(4))
                    .map(|_| self.below(count))
                    .collect(),
            ),
        }
    }
}

#[test]
fn agrees_with_a_search_from_each_package() {
    let mut rng = Rng(2026);
    for graph_index in 0..20_000 {
        let count = 1 + rng.below(8);
        let graph: Vec<Package> = (0..count)
            .map(|_| rng.package(count))
            .collect();
        let root = rng.below(count);
        for (node, answer) in answers(&graph, root) {
            assert_eq!(answer, expected(&graph, node), "graph {graph_index}: {node} from {root}");
        }
    }
}

#[test]
fn a_long_chain_is_decided_without_recursion() {
    const LENGTH: usize = 200_000;
    let graph: Vec<Package> = (0..LENGTH)
        .map(|node| Package::Fit(if node + 1 < LENGTH { vec![node + 1] } else { Vec::new() }))
        .collect();
    let answers = answers(&graph, 0);
    assert_eq!(answers.len(), LENGTH);
    assert!(answers.values().all(|&answer| answer));
}
