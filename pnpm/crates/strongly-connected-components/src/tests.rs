use super::StronglyConnectedComponents;
use pretty_assertions::assert_eq;

fn components(adjacency: &[Vec<usize>], removed: &[bool]) -> Vec<usize> {
    StronglyConnectedComponents::compute(adjacency, removed).component_ids().to_vec()
}

#[test]
fn a_cycle_is_one_component_and_a_tail_another() {
    let adjacency = [vec![1], vec![2], vec![0], vec![0]];
    let ids = components(&adjacency, &[false; 4]);
    assert_eq!((ids[0], ids[1]), (ids[2], ids[2]));
    assert_ne!(ids[3], ids[0]);
    let scc = StronglyConnectedComponents::compute(&adjacency, &[false; 4]);
    assert_eq!(scc.component_count(), 2);
    assert!(scc.may_lie_on_cycle(0, &adjacency));
    assert!(!scc.may_lie_on_cycle(3, &adjacency));
    assert!(scc.shares_component(0, 2));
}

#[test]
fn a_removed_node_has_no_component_and_cuts_its_cycle() {
    let adjacency = [vec![1], vec![2], vec![0]];
    let removed = [false, true, false];
    let scc = StronglyConnectedComponents::compute(&adjacency, &removed);
    assert_eq!(scc.component_ids()[1], usize::MAX);
    assert_eq!(scc.component_count(), 2);
    assert!(!scc.shares_component(0, 2));
}

#[test]
fn a_self_loop_may_lie_on_a_cycle_alone() {
    let adjacency = [vec![0], vec![]];
    let scc = StronglyConnectedComponents::compute(&adjacency, &[false; 2]);
    assert_eq!(scc.component_count(), 2);
    assert!(scc.may_lie_on_cycle(0, &adjacency));
    assert!(!scc.may_lie_on_cycle(1, &adjacency));
}

#[test]
fn a_deep_chain_does_not_recurse() {
    let node_count = 200_000;
    let adjacency: Vec<Vec<usize>> = (0..node_count)
        .map(|node| if node + 1 < node_count { vec![node + 1] } else { vec![0] })
        .collect();
    let scc = StronglyConnectedComponents::compute(&adjacency, &vec![false; node_count]);
    assert_eq!(scc.component_count(), 1);
}
