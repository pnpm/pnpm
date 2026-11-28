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

#[test]
fn lists_members_in_pop_order_and_components_after_what_they_reach() {
    // 0 -> 1 -> 2 -> 1, 2 -> 3, and 4 apart.
    let adjacency = [vec![1], vec![2], vec![1, 3], vec![], vec![]];
    let scc = StronglyConnectedComponents::compute(&adjacency, &[false; 5]);
    assert_eq!(scc.components(), [vec![3], vec![2, 1], vec![0], vec![4]]);
}

/// A graph of up to 16 nodes from a fixed generator, with about one node in
/// five removed.
fn generated_graph(state: &mut u64) -> (Vec<Vec<usize>>, Vec<bool>) {
    let mut next = || {
        *state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(*state >> 33).expect("31 bits fit")
    };
    let node_count = 1 + next() % 16;
    let mut adjacency = vec![Vec::new(); node_count];
    for _ in 0..next() % 40 {
        let from = next() % node_count;
        adjacency[from].push(next() % node_count);
    }
    let removed = (0..node_count)
        .map(|_| next() % 5 == 0)
        .collect();
    (adjacency, removed)
}

/// Every included node is listed once, in the component `component_ids`
/// gives it, and no removed node is listed.
fn assert_members_match_ids(scc: &StronglyConnectedComponents, removed: &[bool]) {
    let mut listed = vec![0; removed.len()];
    for (component, members) in scc.components().iter().enumerate() {
        for &member in members {
            assert_eq!(scc.component_ids()[member], component);
            listed[member] += 1;
        }
    }
    let expected: Vec<usize> = removed
        .iter()
        .map(|&removed| usize::from(!removed))
        .collect();
    assert_eq!(listed, expected);
}

/// An edge between two included nodes never runs to a later component.
fn assert_edges_run_to_earlier_components(
    scc: &StronglyConnectedComponents,
    adjacency: &[Vec<usize>],
    removed: &[bool],
) {
    let ids = scc.component_ids();
    let included = |node: &usize| !removed[*node];
    for from in (0..adjacency.len()).filter(included) {
        assert!(
            adjacency[from]
                .iter()
                .filter(|to| included(to))
                .all(|&to| ids[from] >= ids[to]),
        );
    }
}

#[test]
fn components_agree_with_ids_and_follow_the_edges() {
    let mut state = 2026;
    for _ in 0..2_000 {
        let (adjacency, removed) = generated_graph(&mut state);
        let scc = StronglyConnectedComponents::compute(&adjacency, &removed);
        dbg!(&adjacency, &removed);
        assert_eq!(scc.components().len(), scc.component_count());
        assert_members_match_ids(&scc, &removed);
        assert_edges_run_to_earlier_components(&scc, &adjacency, &removed);
    }
}
