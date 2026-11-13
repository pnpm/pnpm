use super::{LAYERS, edge, workspace};
use crate::{
    cochange::{self, CoChange},
    dsm::{self, Dependency, Group, Mark},
    graph::{CrateGraph, DependencyKind},
    layers::Layers,
    modules::TopLevelGraph,
    page,
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

/// Three commits, newest first, as `cochange::read_log` asks `git` for them.
const LOG: &str = "\u{1e}2026-11-03\n\
                   \n\
                   crates/top/src/lib.rs\n\
                   crates/base/src/lib.rs\n\
                   crates/base/Cargo.toml\n\
                   README.md\n\
                   \u{1e}2026-11-02\n\
                   \n\
                   crates/base/src/lib.rs\n\
                   crates/mid/src/lib.rs\n\
                   crates/top/src/main.rs\n\
                   \u{1e}2026-11-01\n\
                   \n\
                   crates/top/src/lib.rs\n\
                   crates/base/src/lib.rs\n";

fn dirs() -> BTreeMap<String, String> {
    ["base", "mid", "top"]
        .into_iter()
        .map(|name| (name.to_owned(), format!("crates/{name}")))
        .collect()
}

fn pair(left: &str, right: &str, count: usize) -> ((String, String), usize) {
    ((left.to_owned(), right.to_owned()), count)
}

#[test]
fn counts_the_commits_that_change_each_pair_of_crates() {
    let co_change = CoChange::count(LOG, cochange::crate_of(&dirs()), 2);
    dbg!(&co_change);
    assert_eq!(co_change.commits, 3);
    assert_eq!(co_change.span, Some(("2026-11-01".to_owned(), "2026-11-03".to_owned())));
    assert_eq!(co_change.changes_of("base"), 3);
    assert_eq!(co_change.changes_of("mid"), 1);
    assert_eq!(co_change.changes_of("top"), 3);
    assert_eq!(co_change.wide_commits, 1);
    assert_eq!(co_change.pairs, BTreeMap::from([pair("base", "top", 2)]));
    assert_eq!(co_change.together("top", "base"), 2);

    let all_commits = CoChange::count(LOG, cochange::crate_of(&dirs()), 3);
    assert_eq!(
        all_commits.pairs,
        BTreeMap::from([pair("base", "mid", 1), pair("base", "top", 3), pair("mid", "top", 1)]),
    );
}

#[test]
fn maps_a_path_to_its_top_level_module() {
    let module_of = cochange::module_of("crates/top");
    let modules = [
        "crates/top/src/install.rs",
        "crates/top/src/install/state.rs",
        "crates/top/src/lib.rs",
        "crates/top/src/main.rs",
        "crates/top/src/bin/tool/lib.rs",
        "crates/top/tests/install.rs",
        "crates/top/src/README.md",
        "crates/topper/src/install.rs",
    ]
    .map(module_of);
    assert_eq!(
        modules,
        [
            Some("install".to_owned()),
            Some("install".to_owned()),
            None,
            None,
            Some("bin".to_owned()),
            None,
            None,
            None,
        ],
    );
}

/// `base` and `stray` in the bottom layer and no layer, `mid` and `top`
/// in the top layer.
fn layered_graph() -> CrateGraph {
    let mut graph = workspace("[package]\nname = \"top\"\n");
    graph.crates.extend(["mid".to_owned(), "stray".to_owned()]);
    graph.edges.extend([
        edge("top", "base", DependencyKind::Normal),
        edge("top", "base", DependencyKind::Dev),
        edge("base", "mid", DependencyKind::Normal),
        edge("base", "top", DependencyKind::Dev),
    ]);
    graph
}

fn layers(extra: &str) -> Layers {
    let text = LAYERS.replace(r#"crates = ["top"]"#, r#"crates = ["top", "mid"]"#);
    Layers::parse(&format!("{text}{extra}")).unwrap()
}

#[test]
fn orders_crates_by_layer_from_the_bottom_then_by_name() {
    let matrix = dsm::crates(&layered_graph(), &layers(""), CoChange::default());
    let group = |name: &str, members: &[&str]| Group {
        name: name.to_owned(),
        members: members
            .iter()
            .map(|member| (*member).to_owned())
            .collect(),
    };
    assert_eq!(
        matrix.groups,
        vec![
            group("low", &["base"]),
            group("high", &["mid", "top"]),
            group("no layer", &["stray"]),
        ],
    );
}

fn mark(graph: &CrateGraph, layers: &Layers, from: &str, to: &str) -> Option<Dependency> {
    let matrix = dsm::crates(graph, layers, CoChange::default());
    matrix.dependencies
        .get(&(from.to_owned(), to.to_owned()))
        .cloned()
}

#[test]
fn marks_upward_dependencies_unless_only_tests_use_them() {
    let graph = layered_graph();
    let plain = layers("");
    let dependency = |mark, detail: &str| Some(Dependency { mark, detail: detail.to_owned() });
    assert_eq!(mark(&graph, &plain, "top", "base"), dependency(Mark::Linked, "normal, dev"));
    assert_eq!(mark(&graph, &plain, "base", "mid"), dependency(Mark::Upward, "normal"));
    assert_eq!(mark(&graph, &plain, "base", "top"), dependency(Mark::TestsOnly, "dev"));
    assert_eq!(mark(&graph, &plain, "mid", "base"), None);

    let excepted = layers("\n[[exception]]\nfrom = \"base\"\nto = \"mid\"\nreason = \"pending\"\n");
    assert_eq!(mark(&graph, &excepted, "base", "mid"), dependency(Mark::Excepted, "normal"));
}

#[test]
fn lists_pairs_that_change_together_without_a_dependency() {
    let log = "\u{1e}2026-11-02\n\ncrates/mid/a.rs\ncrates/top/a.rs\ncrates/stray/a.rs\n\
               \u{1e}2026-11-01\n\ncrates/base/a.rs\ncrates/top/a.rs\ncrates/stray/a.rs\n\
               \u{1e}2026-11-01\n\ncrates/top/a.rs\ncrates/stray/a.rs\n";
    let mut dirs = dirs();
    dirs.insert("stray".to_owned(), "crates/stray".to_owned());
    let co_change = CoChange::count(log, cochange::crate_of(&dirs), 10);
    let matrix = dsm::crates(&layered_graph(), &layers(""), co_change);
    assert_eq!(
        matrix.independent_pairs(),
        vec![("stray", "top", 3), ("base", "stray", 1), ("mid", "stray", 1), ("mid", "top", 1)],
    );
}

#[test]
fn shades_grow_with_the_square_root_of_the_count() {
    let shades = [1, 4, 5, 25, 26, 64, 100].map(|count| page::shade(count, 100));
    assert_eq!(shades, [1, 1, 2, 3, 3, 4, 5]);
}

#[test]
fn the_page_highlights_upward_dependencies_and_draws_layers() {
    let co_change = CoChange::count(LOG, cochange::crate_of(&dirs()), 10);
    let html = page::render(&dsm::crates(&layered_graph(), &layers(""), co_change));
    println!("{html}");
    assert!(html.contains("<th class=g rowspan=2>high"));
    assert!(html.contains(
        "<td class=\"h3 u\" title=\"base → mid\nchanged together in 1 commit (base: 3, mid: \
         1)\ndepends: normal, upward\">▲",
    ));
    assert!(html.contains(r#"<td class=d title="top: changed in 3 commits">"#));
    assert!(html.contains("<tr><td>mid and top<td>1<td>1, 3"));
}

#[test]
fn a_module_matrix_marks_references_between_modules() {
    let graph = TopLevelGraph {
        code_lines: BTreeMap::from([("install".to_owned(), 10), ("state".to_owned(), 5)]),
        edges: BTreeMap::from([(("install".to_owned(), "state".to_owned()), 3)]),
    };
    let matrix = dsm::modules("top", &graph, CoChange::default());
    assert_eq!(
        matrix.groups,
        vec![Group {
            name: "top".to_owned(),
            members: vec!["install".to_owned(), "state".to_owned()]
        }],
    );
    assert_eq!(
        matrix.dependencies,
        BTreeMap::from([(
            ("install".to_owned(), "state".to_owned()),
            Dependency { mark: Mark::Linked, detail: "3 references".to_owned() },
        )]),
    );
}
