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
    let g: HashMap<String, Vec<String>> = HashMap::new();
    let r = graph_sequencer(&g, &[]);
    dbg!(&r);
    assert!(r.safe, "empty graph is trivially safe");
    assert!(r.chunks.is_empty(), "no included nodes ⇒ no chunks");
    assert!(r.cycles.is_empty(), "no nodes ⇒ no cycles");
}

#[test]
fn linear_chain_runs_leaf_first() {
    // a -> b -> c. c must build first, then b, then a.
    let g = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let nodes = included(&["a", "b", "c"]);
    let r = graph_sequencer(&g, &nodes);
    dbg!(&r);
    assert!(r.safe, "DAG must sort safely: {r:?}");
    assert_eq!(r.chunks, vec![vec!["c".to_string()], vec!["b".to_string()], vec!["a".to_string()]]);
}

#[test]
fn parallel_siblings_share_chunk() {
    // root -> {a, b, c}; siblings have no edges among themselves.
    let g = graph(&[("root", &["a", "b", "c"]), ("a", &[]), ("b", &[]), ("c", &[])]);
    let nodes = included(&["root", "a", "b", "c"]);
    let r = graph_sequencer(&g, &nodes);
    dbg!(&r);
    assert!(r.safe, "DAG must sort safely: {r:?}");
    assert_eq!(r.chunks.len(), 2);
    let mut first = r.chunks[0].clone();
    first.sort();
    assert_eq!(first, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    assert_eq!(r.chunks[1], vec!["root".to_string()]);
}

#[test]
fn diamond_dag() {
    // a -> {b, c}; both b and c -> d.
    let g = graph(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
    let nodes = included(&["a", "b", "c", "d"]);
    let r = graph_sequencer(&g, &nodes);
    dbg!(&r);
    assert!(r.safe, "DAG must sort safely: {r:?}");
    assert_eq!(r.chunks.len(), 3);
    assert_eq!(r.chunks[0], vec!["d".to_string()]);
    let mut middle = r.chunks[1].clone();
    middle.sort();
    assert_eq!(middle, vec!["b".to_string(), "c".to_string()]);
    assert_eq!(r.chunks[2], vec!["a".to_string()]);
}

#[test]
fn excluded_nodes_are_ignored() {
    // a -> b -> c, but only sequence {a, c}; b is not in `included`.
    let g = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let nodes = included(&["a", "c"]);
    let r = graph_sequencer(&g, &nodes);
    dbg!(&r);
    assert!(r.safe, "excluded-edge subgraph must sort safely: {r:?}");
    // a's only outgoing edge is to b which is excluded, so a has degree 0.
    // c also has degree 0.
    assert_eq!(r.chunks.len(), 1);
    let mut only = r.chunks[0].clone();
    only.sort();
    assert_eq!(only, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn cycle_marks_unsafe_and_groups_cycle_nodes() {
    // a -> b -> a. Cycle of length 2: not safe.
    let g = graph(&[("a", &["b"]), ("b", &["a"])]);
    let nodes = included(&["a", "b"]);
    let r = graph_sequencer(&g, &nodes);
    dbg!(&r);
    assert!(!r.safe, "length-2 cycle must mark unsafe: {r:?}");
    assert!(!r.cycles.is_empty(), "cycle list must record the cycle: {r:?}");
    // Both nodes still appear in some chunk.
    let flat: Vec<String> = r.chunks.into_iter().flatten().collect();
    let mut sorted = flat.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn self_loop_not_safe_flag() {
    // a -> a. A self-loop has cycle length 1; pnpm does not mark length-1 as unsafe.
    let g = graph(&[("a", &["a"])]);
    let nodes = included(&["a"]);
    let r = graph_sequencer(&g, &nodes);
    dbg!(&r);
    assert!(r.safe, "length-1 self-loop must not mark unsafe: {r:?}");
    assert_eq!(r.chunks.len(), 1);
    assert_eq!(r.chunks[0], vec!["a".to_string()]);
}

#[test]
fn deterministic_order_follows_included() {
    // Three independent leaves; chunk order should follow the `included` slice.
    let g = graph(&[("x", &[]), ("y", &[]), ("z", &[])]);
    let r1 = graph_sequencer(&g, &included(&["x", "y", "z"]));
    let r2 = graph_sequencer(&g, &included(&["z", "y", "x"]));
    assert_eq!(r1.chunks[0], vec!["x".to_string(), "y".to_string(), "z".to_string()]);
    assert_eq!(r2.chunks[0], vec!["z".to_string(), "y".to_string(), "x".to_string()]);
}
