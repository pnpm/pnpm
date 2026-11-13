use super::MemoryTree;
use crate::{
    boundary,
    graph::{self, CrateGraph, DependencyKind, Edge},
    layers::Layers,
    problem::{BoundaryViolation, Problem},
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

const ROOT_MANIFEST: &str = r#"
[workspace]
members = ["cli/crates/*", "cli/tasks/*", "server/crates/*"]
"#;

/// A workspace of two products, `cli` and `server`. `engine` belongs to
/// `server` by its directory alone.
fn workspace(engine_manifest: &'static str) -> CrateGraph {
    let tree = MemoryTree(BTreeMap::from([
        ("Cargo.toml", ROOT_MANIFEST),
        ("cli/crates/shared/Cargo.toml", "[package]\nname = \"cli-shared\"\n"),
        ("cli/crates/private/Cargo.toml", "[package]\nname = \"cli-private\"\n"),
        (
            "cli/tasks/bench/Cargo.toml",
            "[package]\nname = \"cli-bench\"\n[dev-dependencies]\nengine = { path = \"x\" }\n",
        ),
        ("server/crates/core/Cargo.toml", engine_manifest),
    ]));
    graph::read(&tree).unwrap()
}

const ENGINE: &str = "[package]\nname = \"engine\"\n[dependencies]\ncli-shared = \"1\"\n";

fn check(graph: &CrateGraph, shared: &[&str]) -> Vec<Problem> {
    let list = shared
        .iter()
        .map(|name| format!(r#""{name}""#))
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(
        "[[layer]]\nname = \"all\"\ndescription = \"all\"\ncrates = []\n\n\
         [product_boundary]\ncrates = [{list}]\n",
    );
    let layers = Layers::parse(&text).unwrap();
    boundary::check(graph, layers.boundary_crates())
}

#[test]
fn takes_the_product_from_the_crate_directory() {
    let graph = workspace(ENGINE);
    let products = graph.crates
        .iter()
        .map(|name| (name.as_str(), boundary::product_of(&graph, name).unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        products,
        vec![
            ("cli-bench", "cli"),
            ("cli-private", "cli"),
            ("cli-shared", "cli"),
            ("engine", "server"),
        ],
    );
}

#[test]
fn accepts_dependencies_on_listed_crates_in_either_direction() {
    let graph = workspace(ENGINE);
    assert_eq!(check(&graph, &["cli-shared", "engine"]), vec![]);
}

#[test]
fn reports_a_dependency_on_an_unlisted_crate_of_another_product() {
    let graph = workspace(
        "[package]\nname = \"engine\"\n[dependencies]\ncli-shared = \"1\"\ncli-private = \"1\"\n",
    );
    assert_eq!(
        check(&graph, &["cli-shared"]),
        vec![
            Problem::Boundary(BoundaryViolation::Unlisted {
                from: "cli-bench".to_owned(),
                from_product: "cli".to_owned(),
                to: "engine".to_owned(),
                to_product: "server".to_owned(),
            }),
            Problem::Boundary(BoundaryViolation::Unlisted {
                from: "engine".to_owned(),
                from_product: "server".to_owned(),
                to: "cli-private".to_owned(),
                to_product: "cli".to_owned(),
            }),
        ],
    );
}

#[test]
fn a_listed_crate_without_a_dependent_in_another_product_is_stale() {
    let graph = workspace("[package]\nname = \"engine\"\n");
    assert_eq!(
        check(&graph, &["cli-shared", "engine", "gone"]),
        vec![
            Problem::Boundary(BoundaryViolation::Stale("cli-shared".to_owned())),
            Problem::Boundary(BoundaryViolation::Stale("gone".to_owned())),
        ],
    );
}

#[test]
fn a_dependency_inside_one_product_needs_no_entry() {
    let mut graph = workspace("[package]\nname = \"engine\"\n");
    graph.edges.clear();
    graph.edges.insert(Edge {
        from: "cli-bench".to_owned(),
        to: "cli-private".to_owned(),
        kind: DependencyKind::Normal,
    });
    assert_eq!(check(&graph, &[]), vec![]);
}
