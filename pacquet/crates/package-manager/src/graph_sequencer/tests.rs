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

#[test]
fn empty_graph() {
    let graph_map: HashMap<String, Vec<String>> = HashMap::new();
    let result = graph_sequencer(&graph_map, &[]);
    dbg!(&result);
    assert!(result.safe, "empty graph is trivially safe");
    assert!(result.chunks.is_empty(), "no included nodes ⇒ no chunks");
    assert!(result.cycles.is_empty(), "no nodes ⇒ no cycles");
}

#[test]
fn linear_chain_runs_leaf_first() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let nodes = included(&["a", "b", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(result.safe, "DAG must sort safely: {result:?}");
    assert_eq!(
        result.chunks,
        vec![vec!["c".to_string()], vec!["b".to_string()], vec!["a".to_string()]],
    );
}

#[test]
fn parallel_siblings_share_chunk() {
    let graph_map = graph(&[("root", &["a", "b", "c"]), ("a", &[]), ("b", &[]), ("c", &[])]);
    let nodes = included(&["root", "a", "b", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(result.safe, "DAG must sort safely: {result:?}");
    assert_eq!(result.chunks.len(), 2);
    let mut first = result.chunks[0].clone();
    first.sort();
    assert_eq!(first, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    assert_eq!(result.chunks[1], vec!["root".to_string()]);
}

#[test]
fn diamond_dag() {
    let graph_map = graph(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
    let nodes = included(&["a", "b", "c", "d"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(result.safe, "DAG must sort safely: {result:?}");
    assert_eq!(result.chunks.len(), 3);
    assert_eq!(result.chunks[0], vec!["d".to_string()]);
    let mut middle = result.chunks[1].clone();
    middle.sort();
    assert_eq!(middle, vec!["b".to_string(), "c".to_string()]);
    assert_eq!(result.chunks[2], vec!["a".to_string()]);
}

#[test]
fn excluded_nodes_are_ignored() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let nodes = included(&["a", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(result.safe, "excluded-edge subgraph must sort safely: {result:?}");
    assert_eq!(result.chunks.len(), 1);
    let mut only = result.chunks[0].clone();
    only.sort();
    assert_eq!(only, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn cycle_marks_unsafe_and_groups_cycle_nodes() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["a"])]);
    let nodes = included(&["a", "b"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(!result.safe, "length-2 cycle must mark unsafe: {result:?}");
    assert!(!result.cycles.is_empty(), "cycle list must record the cycle: {result:?}");
    let flat: Vec<String> = result.chunks.into_iter().flatten().collect();
    let mut sorted = flat;
    sorted.sort();
    assert_eq!(sorted, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn self_loop_not_safe_flag() {
    let graph_map = graph(&[("a", &["a"])]);
    let nodes = included(&["a"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(result.safe, "length-1 self-loop must not mark unsafe: {result:?}");
    assert_eq!(result.chunks.len(), 1);
    assert_eq!(result.chunks[0], vec!["a".to_string()]);
}

#[test]
fn deterministic_order_follows_included() {
    let graph_map = graph(&[("x", &[]), ("y", &[]), ("z", &[])]);
    let r1 = graph_sequencer(&graph_map, &included(&["x", "y", "z"]));
    let r2 = graph_sequencer(&graph_map, &included(&["z", "y", "x"]));
    assert_eq!(r1.chunks[0], vec!["x".to_string(), "y".to_string(), "z".to_string()]);
    assert_eq!(r2.chunks[0], vec!["z".to_string(), "y".to_string(), "x".to_string()]);
}
