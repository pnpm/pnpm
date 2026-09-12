//! Dependency resolution and topological-order tests for recursive commands.
//!
//! A selected subset keeps its original `dependencies`, order is resolved
//! through the full graph so two selected projects connected only through an
//! unselected one still run in dependency order, and a `--filter-prod`
//! selection resolves through the prod-pruned graph so the dev edges it
//! dropped are not pulled back in.

use super::{filtered_projects_dependencies, graph_sequencer, sequence_graph};
use pnpm_workspace_projects_graph::{ProjectGraph, ProjectGraphNode};
use pretty_assertions::assert_eq;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

fn make_graph(adjacency: &[(&str, &[&str])]) -> ProjectGraph<()> {
    adjacency
        .iter()
        .map(|(dir, deps)| {
            let node = ProjectGraphNode { package: (), dependencies: dirs(deps) };
            (PathBuf::from(dir), node)
        })
        .collect()
}

/// A subset of `graph`'s nodes that keep their original `dependencies` arrays
/// — still referencing the unselected projects — exactly as the real selected
/// graph is built.
fn select(graph: &ProjectGraph<()>, names: &[&str]) -> ProjectGraph<()> {
    names
        .iter()
        .map(|name| {
            let dir = PathBuf::from(name);
            let node = graph[&dir].clone();
            (dir, node)
        })
        .collect()
}

/// Concatenate selected subsets in order, so a project a `--filter-prod`
/// selector contributed keeps the prod-pruned edges it was selected with.
fn merge(graphs: [ProjectGraph<()>; 2]) -> ProjectGraph<()> {
    graphs.into_iter().flatten().collect()
}

fn dirs(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

fn prod_only(names: &[&str]) -> HashSet<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

fn filtered_order(
    selected: &ProjectGraph<()>,
    all: &ProjectGraph<()>,
    prod_all: Option<&ProjectGraph<()>>,
    prod_only_selected: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let dependencies = filtered_projects_dependencies(selected, all, prod_all, prod_only_selected);
    let edges: HashMap<PathBuf, Vec<PathBuf>> = dependencies
        .iter()
        .map(|(project, dependencies)| (project.clone(), dependencies.clone()))
        .collect();
    graph_sequencer(&edges, &dependencies.keys().cloned().collect::<Vec<_>>()).order
}

#[test]
fn sorts_every_project_when_only_one_graph_is_given() {
    let graph = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    assert_eq!(sequence_graph(&graph, &graph).order, dirs(&["c", "b", "a"]));
}

#[test]
fn orders_selected_projects_connected_only_through_an_unselected_project() {
    let graph = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    assert_eq!(sequence_graph(&select(&graph, &["a", "c"]), &graph).order, dirs(&["c", "a"]));
}

#[test]
fn keeps_independent_selected_projects_in_selection_order() {
    let graph = make_graph(&[("a", &["b"]), ("b", &[]), ("c", &[])]);
    assert_eq!(sequence_graph(&select(&graph, &["a", "c"]), &graph).order, dirs(&["a", "c"]));
}

#[test]
fn resolves_transitive_edges_across_a_diamond_of_unselected_projects() {
    let graph = make_graph(&[("a", &["x", "y"]), ("x", &["c"]), ("y", &["c"]), ("c", &[])]);
    assert_eq!(sequence_graph(&select(&graph, &["a", "c"]), &graph).order, dirs(&["c", "a"]));
}

#[test]
fn without_a_full_graph_resolution_is_limited_to_edges_among_the_sorted_projects() {
    let graph = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let selected = select(&graph, &["a", "c"]);
    assert_eq!(sequence_graph(&selected, &selected).order, dirs(&["a", "c"]));
}

#[test]
fn does_not_reintroduce_edges_that_the_selected_graph_pruned() {
    let full_graph = make_graph(&[("a", &["b"]), ("b", &[])]);
    // The selection dropped a's edge to b, as a prod-only filter drops dev edges.
    let selected = make_graph(&[("a", &[]), ("b", &[])]);
    assert_eq!(sequence_graph(&selected, &full_graph).order, dirs(&["a", "b"]));
}

#[test]
fn filtered_projects_dependencies_resolves_transitive_order_for_regular_filters() {
    let full = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let selected = select(&full, &["a", "c"]);
    assert_eq!(filtered_order(&selected, &full, None, &prod_only(&[])), dirs(&["c", "a"]));
}

#[test]
fn filtered_projects_dependencies_resolves_a_prod_only_selection_through_the_prod_graph() {
    // b's dev edge to c is gone in the prod graph, so c is not pulled ahead of a.
    let prod = make_graph(&[("a", &["b"]), ("b", &[]), ("c", &[])]);
    let full = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let selected = select(&prod, &["a", "c"]);
    assert_eq!(
        filtered_order(&selected, &full, Some(&prod), &prod_only(&["a", "c"])),
        dirs(&["a", "c"]),
    );
}

#[test]
fn filtered_projects_dependencies_orders_a_prod_only_selection_by_transitive_prod_deps() {
    // Every edge is a prod edge, so a transitively depends on c through the
    // unselected b and must run after it.
    let prod = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
    let selected = select(&prod, &["a", "c"]);
    assert_eq!(
        filtered_order(&selected, &selected, Some(&prod), &prod_only(&["a", "c"])),
        dirs(&["c", "a"]),
    );
}

#[test]
fn filtered_projects_dependencies_keeps_prod_only_roots_on_the_prod_graph_in_mixed_selections() {
    let full =
        make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["x"]), ("x", &["a"]), ("d", &[])]);
    let prod = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["x"]), ("x", &[]), ("d", &[])]);
    let selected = merge([select(&prod, &["a", "c"]), select(&full, &["d"])]);
    assert_eq!(
        filtered_order(&selected, &full, Some(&prod), &prod_only(&["a", "c"])),
        dirs(&["c", "d", "a"]),
    );
}

#[test]
fn does_not_order_a_regular_filter_across_a_dev_edge_pruned_by_a_prod_only_selection() {
    // a -> x -> c -> d with x selected prod-only. x's edge to c is a dev edge the
    // prod selection drops, so a reaches c only through it and stays concurrent
    // with c rather than after it.
    let full = make_graph(&[("a", &["x"]), ("x", &["c"]), ("c", &["d"]), ("d", &[])]);
    let prod = make_graph(&[("a", &["x"]), ("x", &[]), ("c", &["d"]), ("d", &[])]);
    let selected = merge([select(&prod, &["x"]), select(&full, &["a", "c", "d"])]);
    assert_eq!(
        filtered_order(&selected, &full, Some(&prod), &prod_only(&["x"])),
        dirs(&["x", "d", "a", "c"]),
    );
}

#[test]
fn orders_a_regular_filter_across_a_prod_edge_kept_by_a_prod_only_selection() {
    // Same shape, but x -> c is a prod edge the prod graph keeps, so the full
    // a -> x -> c -> d chain holds even though x is sorted through the prod graph.
    let full = make_graph(&[("a", &["x"]), ("x", &["c"]), ("c", &["d"]), ("d", &[])]);
    let prod = make_graph(&[("a", &["x"]), ("x", &["c"]), ("c", &["d"]), ("d", &[])]);
    let selected = merge([select(&prod, &["x"]), select(&full, &["a", "c", "d"])]);
    assert_eq!(
        filtered_order(&selected, &full, Some(&prod), &prod_only(&["x"])),
        dirs(&["d", "c", "x", "a"]),
    );
}

#[test]
fn detects_a_cycle_that_passes_through_unselected_projects() {
    let graph = make_graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"])]);
    let result = sequence_graph(&select(&graph, &["a", "c"]), &graph);
    dbg!(&result);
    assert!(
        result.cycles.iter().any(|cycle| cycle.len() > 1),
        "a -> b -> c -> a is a cycle once b is tunneled through",
    );
}
