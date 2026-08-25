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

#[test]
fn node_depending_on_a_cycle_runs_after_it() {
    let graph_map = graph(&[("a", &["b"]), ("b", &["a"]), ("c", &["a"])]);
    let nodes = included(&["a", "b", "c"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(!result.safe, "length-2 cycle must mark unsafe: {result:?}");
    assert_eq!(result.chunks.len(), 2, "cycle chunk, then its dependent: {result:?}");
    let mut cycle_chunk = result.chunks[0].clone();
    cycle_chunk.sort();
    assert_eq!(cycle_chunk, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(result.chunks[1], vec!["c".to_string()]);
    assert_eq!(result.cycles.len(), 1);
}

#[test]
fn cycle_members_appear_in_one_chunk_only() {
    // Removing `a` drops `b` to degree zero an instant before `b` is
    // removed as the same cycle's other member — `b` must not be
    // re-chunked after the cycle chunk.
    let graph_map = graph(&[("a", &["b"]), ("b", &["a"])]);
    let nodes = included(&["a", "b"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    let flat: Vec<String> = result.chunks.iter().flatten().cloned().collect();
    let mut sorted = flat.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(flat.len(), sorted.len(), "no node may repeat across chunks: {result:?}");
    assert_eq!(result.chunks.len(), 1);
}

#[test]
fn layers_resume_after_a_cycle_chunk() {
    // d is a plain leaf below the a↔b cycle; e depends on d. The order
    // must be: leaf layer, cycle chunk, then the cycle's dependent.
    let graph_map = graph(&[("a", &["b", "d"]), ("b", &["a"]), ("d", &[]), ("e", &["a"])]);
    let nodes = included(&["a", "b", "d", "e"]);
    let result = graph_sequencer(&graph_map, &nodes);
    dbg!(&result);
    assert!(!result.safe);
    assert_eq!(result.chunks[0], vec!["d".to_string()]);
    let mut cycle_chunk = result.chunks[1].clone();
    cycle_chunk.sort();
    assert_eq!(cycle_chunk, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(result.chunks[2], vec!["e".to_string()]);
}

#[test]
fn deep_chain_sorts_in_linear_time() {
    // A dense chain — every node depends on its nine predecessors — makes
    // as many chunks as nodes. Guards the O(V + E) rewrite: the quadratic
    // per-chunk full scan took seconds at workspace scale (pnpm/pnpm#14149).
    let count = 20_000;
    let names: Vec<String> = (0..count).map(|i| format!("project-{i:05}")).collect();
    let graph_map: HashMap<String, Vec<String>> =
        (0..count).map(|i| (names[i].clone(), names[i.saturating_sub(9)..i].to_vec())).collect();
    let started = std::time::Instant::now();
    let result = graph_sequencer(&graph_map, &names);
    let elapsed = started.elapsed();
    dbg!(elapsed);
    assert!(result.safe);
    assert_eq!(result.chunks.len(), count);
    assert_eq!(result.chunks[0], vec![names[0].clone()]);
    assert_eq!(result.chunks[count - 1], vec![names[count - 1].clone()]);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "a {count}-node chain must sort in linear time, took {elapsed:?}",
    );
}
