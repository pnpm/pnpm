use super::graph_sequencer;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn graph(edges: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
    edges
        .iter()
        .map(|(k, vs)| ((*k).to_string(), vs.iter().map(|s| (*s).to_string()).collect()))
        .collect()
}

fn included(nodes: &[&str]) -> Vec<String> {
    nodes.iter().map(|s| (*s).to_string()).collect()
}

fn is_safe(result: &super::GraphSequencerResult<String>) -> bool {
    result.cycles.iter().all(|cycle| cycle.len() == 1)
}

#[test]
fn empty_graph() {
    let graph_map: HashMap<String, Vec<String>> = HashMap::new();
    let result = graph_sequencer(&graph_map, &[]);
    dbg!(&result);
    assert!(is_safe(&result), "empty graph is trivially safe");
    assert!(result.order.is_empty(), "no included nodes ⇒ no order");
    assert!(result.cycles.is_empty(), "no nodes ⇒ no cycles");
}

#[test]
fn linear_chain_runs_leaf_first() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let nodes = included(&["a", "b", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(is_safe(&result), "DAG must sort safely: {result:?}");
    assert_eq!(result.order, vec!["c".to_string(), "b".to_string(), "a".to_string()]);
}

#[test]
fn parallel_siblings_keep_included_order() {
    let graph_map = graph(&[("root", &["a", "b", "c"]), ("a", &[]), ("b", &[]), ("c", &[])]);
    let nodes = included(&["root", "a", "b", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(is_safe(&result), "DAG must sort safely: {result:?}");
    let mut first = result.order[..3].to_vec();
    first.sort();
    assert_eq!(first, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    assert_eq!(result.order[3], "root");
}

#[test]
fn diamond_dag() {
    let graph_map = graph(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
    let nodes = included(&["a", "b", "c", "d"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(is_safe(&result), "DAG must sort safely: {result:?}");
    assert_eq!(result.order.len(), 4);
    assert_eq!(result.order[0], "d");
    let mut middle = result.order[1..3].to_vec();
    middle.sort();
    assert_eq!(middle, vec!["b".to_string(), "c".to_string()]);
    assert_eq!(result.order[3], "a");
}

#[test]
fn excluded_nodes_are_ignored() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let nodes = included(&["a", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(is_safe(&result), "excluded-edge subgraph must sort safely: {result:?}");
    let mut only = result.order;
    only.sort();
    assert_eq!(only, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn cycle_reports_every_member() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["a"])]);
    let nodes = included(&["a", "b"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(!is_safe(&result), "length-2 cycle must mark unsafe: {result:?}");
    assert!(!result.cycles.is_empty(), "cycle list must record the cycle: {result:?}");
    let mut sorted = result.order;
    sorted.sort();
    assert_eq!(sorted, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn self_loop_is_reported_without_making_the_graph_unorderable() {
    let graph_map = graph(&[("a", &["a"])]);
    let nodes = included(&["a"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(is_safe(&result), "length-1 self-loop must not mark unsafe: {result:?}");
    assert_eq!(result.order, vec!["a".to_string()]);
}

#[test]
fn deterministic_order_follows_included() {
    let graph_map = graph(&[("x", &[]), ("y", &[]), ("z", &[])]);
    let r1 = graph_sequencer(&graph_map, &included(&["x", "y", "z"]));
    let r2 = graph_sequencer(&graph_map, &included(&["z", "y", "x"]));
    assert_eq!(r1.order, vec!["x".to_string(), "y".to_string(), "z".to_string()]);
    assert_eq!(r2.order, vec!["z".to_string(), "y".to_string(), "x".to_string()]);
}

#[test]
fn node_depending_on_a_cycle_runs_after_it() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["a"]), ("c", &["a"])]);
    let nodes = included(&["a", "b", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(!is_safe(&result), "length-2 cycle must mark unsafe: {result:?}");
    assert_eq!(result.order.len(), 3, "cycle, then its dependent: {result:?}");
    let mut cycle_nodes = result.order[..2].to_vec();
    cycle_nodes.sort();
    assert_eq!(cycle_nodes, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(result.order[2], "c");
    assert_eq!(result.cycles.len(), 1);
}

#[test]
fn cycle_members_appear_once() {
    // Removing `a` drops `b` to degree zero an instant before `b` is
    // removed as the same cycle's other member — `b` must not be
    // emitted again after the cycle.
    let graph_map = graph(&[("a", &["b"]), ("b", &["a"])]);
    let nodes = included(&["a", "b"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    let mut sorted = result.order.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(result.order.len(), sorted.len(), "no node may repeat: {result:?}");
    assert_eq!(result.order.len(), 2);
}

#[test]
fn ordering_resumes_after_a_cycle() {
    // d is a plain leaf below the a↔b cycle; e depends on the cycle
    // through a. The order must be: leaf, cycle, then the
    // cycle's dependent.
    let graph_map = graph(&[("a", &["b", "d"]), ("b", &["a"]), ("d", &[]), ("e", &["a"])]);
    let nodes = included(&["a", "b", "d", "e"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(!is_safe(&result));
    assert_eq!(result.order[0], "d");
    let mut cycle_nodes = result.order[1..3].to_vec();
    cycle_nodes.sort();
    assert_eq!(cycle_nodes, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(result.order[3], "e");
}

#[test]
fn deep_chain_sorts_in_linear_time() {
    // A dense chain where every node depends on its nine predecessors guards
    // the O(V + E) rewrite: the quadratic full scan took seconds at
    // workspace scale (pnpm/pnpm#14149).
    let count = 20_000;
    let names: Vec<String> = (0..count).map(|i| format!("project-{i:05}")).collect();
    let graph_map: HashMap<String, Vec<String>> =
        (0..count).map(|i| (names[i].clone(), names[i.saturating_sub(9)..i].to_vec())).collect();
    let started = std::time::Instant::now();
    let result = graph_sequencer(&graph_map, &names);
    let elapsed = started.elapsed();
    dbg!(elapsed);
    assert!(is_safe(&result));
    assert_eq!(result.order.len(), count);
    assert_eq!(result.order[0], names[0]);
    assert_eq!(result.order[count - 1], names[count - 1]);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "a {count}-node chain must sort in linear time, took {elapsed:?}",
    );
}

#[test]
fn dependents_of_a_cycle_sort_in_linear_time() {
    // Many nodes lead into one large ring and are listed before it. The
    // component filter must keep the cycle pass from paying a full ring
    // walk per dependent, and the ring stays before every dependent.
    let ring_len = 3_000;
    let dependent_count = 30_000;
    let dependents: Vec<String> = (0..dependent_count).map(|i| format!("dep-{i:05}")).collect();
    let ring: Vec<String> = (0..ring_len).map(|i| format!("ring-{i:04}")).collect();
    let mut graph_map: HashMap<String, Vec<String>> = HashMap::new();
    for (i, name) in dependents.iter().enumerate() {
        graph_map.insert(name.clone(), vec![ring[i % ring_len].clone()]);
    }
    for (i, name) in ring.iter().enumerate() {
        graph_map.insert(name.clone(), vec![ring[(i + 1) % ring_len].clone()]);
    }
    let included: Vec<String> = dependents.iter().chain(ring.iter()).cloned().collect();
    let started = std::time::Instant::now();
    let result = graph_sequencer(&graph_map, &included);
    let elapsed = started.elapsed();
    dbg!(elapsed);
    assert!(!is_safe(&result));
    assert_eq!(result.cycles.len(), 1, "one ring, one reported cycle: {:?}", result.cycles.len());
    assert_eq!(result.order.len(), ring_len + dependent_count);
    assert_eq!(
        result.order[..ring_len].iter().cloned().collect::<std::collections::HashSet<_>>(),
        ring.iter().cloned().collect(),
    );
    assert_eq!(
        result.order[ring_len..].iter().cloned().collect::<std::collections::HashSet<_>>(),
        dependents.iter().cloned().collect(),
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "dependents of a cycle must not each pay a ring walk, took {elapsed:?}",
    );
}

#[test]
fn chained_components_sort_in_linear_time() {
    // Thousands of two-node rings, each pointing into the next. Confining
    // the cycle search to a ring's own strongly connected component keeps
    // one pass from walking every downstream ring per cycle.
    let ring_count = 5_000;
    let names: Vec<(String, String)> =
        (0..ring_count).map(|i| (format!("a-{i:04}"), format!("b-{i:04}"))).collect();
    let mut graph_map: HashMap<String, Vec<String>> = HashMap::new();
    for (i, (a, b)) in names.iter().enumerate() {
        let mut a_edges = vec![b.clone()];
        if let Some((next_a, _)) = names.get(i + 1) {
            a_edges.push(next_a.clone());
        }
        graph_map.insert(a.clone(), a_edges);
        graph_map.insert(b.clone(), vec![a.clone()]);
    }
    let included: Vec<String> = names.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect();
    let started = std::time::Instant::now();
    let result = graph_sequencer(&graph_map, &included);
    let elapsed = started.elapsed();
    dbg!(elapsed);
    assert!(!is_safe(&result));
    assert_eq!(result.cycles.len(), ring_count);
    assert_eq!(result.order.len(), ring_count * 2);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "chained components must sort in linear time, took {elapsed:?}",
    );
}
