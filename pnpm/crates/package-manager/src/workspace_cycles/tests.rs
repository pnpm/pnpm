use super::workspace_cycles;
use pnpm_workspace_projects_graph::{ProjectGraph, ProjectGraphNode};
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn dirs(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

fn make_graph(adjacency: &[(&str, &[&str])]) -> ProjectGraph<()> {
    adjacency
        .iter()
        .map(|(dir, deps)| {
            let node = ProjectGraphNode { package: (), dependencies: dirs(deps) };
            (PathBuf::from(dir), node)
        })
        .collect()
}

/// A subset of the projects keeps its original `dependencies` arrays —
/// still naming the projects outside it — exactly as a real selected
/// graph does.
fn select(graph: &ProjectGraph<()>, names: &[&str]) -> ProjectGraph<()> {
    names
        .iter()
        .map(|name| {
            let dir = PathBuf::from(name);
            let node = graph.get(&dir).expect("selected project is in the graph");
            (dir, ProjectGraphNode { package: (), dependencies: node.dependencies.clone() })
        })
        .collect()
}

#[test]
fn an_orderable_graph_has_no_cycles() {
    assert_eq!(workspace_cycles(&make_graph(&[("a", &["b"]), ("b", &[])])), None);
}

#[test]
fn names_the_projects_of_a_cycle() {
    let graph = make_graph(&[("a", &["b"]), ("b", &["a"]), ("c", &[])]);
    let cycles = workspace_cycles(&graph).expect("a <-> b is a cycle");
    assert_eq!(cycles, vec![dirs(&["a", "b"])]);
}

/// Edges leaving the graph are dropped rather than followed, so two
/// selected projects joined only through an unselected one are ordered
/// through it without being reported as a cycle — the way pnpm's check
/// sequences its selected graph.
#[test]
fn edges_leaving_the_graph_are_not_followed() {
    let graph = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"])]);
    assert_eq!(workspace_cycles(&select(&graph, &["a", "c"])), None);
}

/// A project depending on itself is not a cycle between projects, and
/// pnpm's sequencer ignores the self-edge too.
#[test]
fn a_self_dependency_is_not_a_cycle() {
    assert_eq!(workspace_cycles(&make_graph(&[("a", &["a"])])), None);
}
