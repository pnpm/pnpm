use crate::{
    dependencies::Reach,
    diff,
    graph::{self, CrateGraph, DependencyKind, Edge},
    layers::Layers,
    problem::{Metric, Mismatch, Problem},
    ratchet,
    tree::Tree,
};
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, BTreeSet};

mod leaks;
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

fn chain() -> CrateGraph {
    let mut graph = workspace("[package]\nname = \"top\"\n");
    graph.crates.insert("mid".to_owned());
    graph.edges.extend([
        edge("top", "mid", DependencyKind::Normal),
        edge("top", "mid", DependencyKind::Build),
        edge("mid", "base", DependencyKind::Normal),
        edge("base", "top", DependencyKind::Dev),
    ]);
    graph
}

#[test]
fn counts_linked_dependencies_directly_and_transitively() {
    let reach = Reach::new(&chain());
    dbg!(&reach);
    fn counts(sets: &BTreeMap<String, BTreeSet<String>>) -> Vec<(&str, usize)> {
        sets.iter()
            .map(|(name, set)| (name.as_str(), set.len()))
            .collect()
    }
    assert_eq!(counts(&reach.direct), vec![("base", 0), ("mid", 1), ("top", 1)]);
    assert_eq!(counts(&reach.transitive), vec![("base", 0), ("mid", 1), ("top", 2)]);
}

#[test]
fn the_direct_dependency_baseline_only_goes_down() {
    let counts = Reach::new(&chain()).direct_counts();
    dbg!(&counts);
    let check = |entries: &[(&str, usize)]| {
        let baseline = entries
            .iter()
            .map(|(name, count)| ((*name).to_owned(), *count))
            .collect();
        ratchet::check(Metric::DirectDependencies, &counts, &baseline)
    };
    assert_eq!(check(&[("mid", 1), ("top", 1)]), vec![]);
    let mismatch = |crate_name: &str, measured, baseline| {
        let crate_name = crate_name.to_owned();
        let metric = Metric::DirectDependencies;
        Problem::Ratchet(Mismatch { metric, crate_name, measured, baseline })
    };
    assert_eq!(check(&[("mid", 1)]), vec![mismatch("top", 1, 0)]);
    assert_eq!(check(&[("mid", 2), ("top", 1)]), vec![mismatch("mid", 1, 2)]);
}

#[test]
fn a_layer_caps_its_crates_transitive_dependencies() {
    let layers = |budget: usize| {
        let text = LAYERS.replace(
            r#"crates = ["top"]"#,
            &format!("crates = [\"top\", \"mid\"]\nmax_transitive_dependencies = {budget}"),
        );
        Layers::parse(&text).unwrap()
    };
    let reach = Reach::new(&chain());
    dbg!(&reach.transitive);
    assert_eq!(reach.check_budgets(&layers(2)), vec![]);
    assert_eq!(
        reach.check_budgets(&layers(1)),
        vec![Problem::OverTransitiveBudget {
            crate_name: "top".to_owned(),
            layer: "high".to_owned(),
            count: 2,
            budget: 1,
        }],
    );
}
