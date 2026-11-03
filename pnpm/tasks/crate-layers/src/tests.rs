use crate::{
    diff,
    graph::{self, CrateGraph, DependencyKind, Edge},
    layers::{Layers, Problem},
    tree::Tree,
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

mod modules;

struct MemoryTree(BTreeMap<&'static str, &'static str>);

impl Tree for MemoryTree {
    fn read(&self, path: &str) -> Result<Option<String>, String> {
        Ok(self.0.get(path).map(|text| (*text).to_owned()))
    }

    fn subdirectories(&self, dir: &str) -> Result<Vec<String>, String> {
        let prefix = format!("{dir}/");
        let names = self.0
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix)?.split_once('/'));
        Ok(names.map(|(name, _)| name.to_owned()).collect())
    }
}

const ROOT_MANIFEST: &str = r#"
[workspace]
members = ["crates/*"]

[workspace.dependencies]
base = { path = "crates/base" }
renamed-base = { path = "crates/base", package = "base" }
serde = "1"
"#;

fn workspace(top_manifest: &'static str) -> CrateGraph {
    let tree = MemoryTree(BTreeMap::from([
        ("Cargo.toml", ROOT_MANIFEST),
        ("crates/base/Cargo.toml", "[package]\nname = \"base\"\n"),
        ("crates/top/Cargo.toml", top_manifest),
    ]));
    graph::read(&tree).unwrap()
}

fn edge(from: &str, to: &str, kind: DependencyKind) -> Edge {
    Edge { from: from.to_owned(), to: to.to_owned(), kind }
}

const LAYERS: &str = r#"
[[layer]]
name = "low"
description = "low"
crates = ["base"]

[[layer]]
name = "high"
description = "high"
crates = ["top"]
"#;

#[test]
fn reads_workspace_edges_through_renames_and_targets() {
    let graph = workspace(
        r#"
[package]
name = "top"

[dependencies]
alias = { workspace = true, package = "base" }
serde = { workspace = true }

[target.'cfg(unix)'.build-dependencies]
renamed-base = { workspace = true }

[dev-dependencies]
base = { path = "../base" }
"#,
    );
    assert_eq!(
        graph.edges.into_iter().collect::<Vec<_>>(),
        vec![
            edge("top", "base", DependencyKind::Normal),
            edge("top", "base", DependencyKind::Build),
            edge("top", "base", DependencyKind::Dev),
        ],
    );
}

#[test]
fn skips_excluded_workspace_directories_like_cargo() {
    let root = r#"
[workspace]
members = ["crates/*", "vendor/*", "crates/pinned"]
exclude = ["./crates/vendored/", "vendor", "crates/pinned"]
"#;
    let tree = MemoryTree(BTreeMap::from([
        ("Cargo.toml", root),
        ("crates/kept/Cargo.toml", "[package]\nname = \"kept\"\n"),
        ("crates/pinned/Cargo.toml", "[package]\nname = \"pinned\"\n"),
        ("crates/vendored/Cargo.toml", "[package]\nname = \"vendored\"\n"),
        ("vendor/nested/Cargo.toml", "[package]\nname = \"nested\"\n"),
    ]));
    let graph = graph::read(&tree).unwrap();
    assert_eq!(
        graph.crates.into_iter().collect::<Vec<_>>(),
        vec!["kept".to_owned(), "pinned".to_owned()],
    );
}

#[test]
fn reports_upward_edges_but_not_upward_dev_dependencies() {
    let mut graph = workspace("[package]\nname = \"top\"\n");
    graph.edges.insert(edge("base", "top", DependencyKind::Dev));
    let layers = Layers::parse(LAYERS).unwrap();
    assert_eq!(
        layers
            .check(&graph)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![],
    );

    graph.edges.insert(edge("base", "top", DependencyKind::Normal));
    let problems = layers
        .check(&graph)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        problems,
        vec![Problem::Upward {
            edge: edge("base", "top", DependencyKind::Normal),
            from_layer: "low".to_owned(),
            to_layer: "high".to_owned(),
        }],
    );
}

#[test]
fn an_exception_covers_its_edge_until_the_edge_is_gone() {
    let excepted =
        format!("{LAYERS}\n[[exception]]\nfrom = \"base\"\nto = \"top\"\nreason = \"pending\"\n");
    let layers = Layers::parse(&excepted).unwrap();
    let mut graph = workspace("[package]\nname = \"top\"\n");
    graph.edges.insert(edge("base", "top", DependencyKind::Normal));
    assert_eq!(
        layers
            .check(&graph)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![],
    );

    graph.edges.clear();
    let problems = layers
        .check(&graph)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        problems,
        vec![Problem::StaleException { from: "base".to_owned(), to: "top".to_owned() }],
    );
}

#[test]
fn every_crate_needs_exactly_one_layer() {
    let layers =
        Layers::parse(&LAYERS.replace(r#"crates = ["top"]"#, r#"crates = ["gone"]"#)).unwrap();
    let problems = layers
        .check(&workspace("[package]\nname = \"top\"\n"))
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        problems,
        vec![Problem::Unassigned("top".to_owned()), Problem::UnknownCrate("gone".to_owned())],
    );

    let error =
        Layers::parse(&LAYERS.replace(r#"crates = ["top"]"#, r#"crates = ["base"]"#)).unwrap_err();
    assert_eq!(error, "crate-layers.toml puts base in more than one layer");
}

#[test]
fn diff_marks_added_upward_edges() {
    let layers = Layers::parse(LAYERS).unwrap();
    let base = workspace("[package]\nname = \"top\"\n");
    let mut head = workspace("[package]\nname = \"top\"\n");
    head.edges.insert(edge("base", "top", DependencyKind::Normal));
    let report = diff::render(&base, &head, &layers, "main");
    assert_eq!(
        report,
        "## Crate graph changes against `main`\n\n\n\
         | Change | Edge | Kind | Layers |\n| --- | --- | --- | --- |\n\
         | added | `base` → `top` | normal | low → high (**upward**) |\n",
    );
    assert_eq!(
        diff::render(&base, &base, &layers, "main"),
        "## Crate graph changes against `main`\n\nNo workspace crate or dependency edge changed.\n",
    );
}
