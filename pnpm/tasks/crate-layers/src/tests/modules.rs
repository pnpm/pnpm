use super::MemoryTree;
use crate::{
    cycles::{self, CycleReport},
    lexer::{self, Token},
    modules::CrateModules,
    problem::{Metric, Mismatch, Problem},
    ratchet,
    use_tree::{self, UsePath},
};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

fn idents(source: &str) -> Vec<String> {
    let tokens = lexer::lex(source).tokens;
    let names = tokens
        .into_iter()
        .filter_map(|token| match token {
            Token::Ident(name) => Some(name),
            _ => None,
        });
    names.collect()
}

#[test]
fn lexer_skips_comments_strings_and_char_literals() {
    let source = concat!(
        "// crate::in_line_comment\n",
        "/* crate::in /* nested */ block */\n",
        "let quoted = \"crate::in_string \\\" still\";\n",
        "let raw = r#\"crate::in_raw \"string\"\"#;\n",
        "let quote = '\"';\n",
        "fn keep<'a>(value: &'a str) {}\n",
    );
    assert_eq!(
        idents(source),
        ["let", "quoted", "let", "raw", "let", "quote", "fn", "keep", "a", "value", "a", "str"],
    );
    assert_eq!(lexer::lex("a\n\n// note\nb c\n").code_lines, 2);
}

#[test]
fn use_trees_expand_groups_aliases_self_and_globs() {
    let tokens = lexer::lex("use crate::{first::{self, Item as Alias}, second::*};").tokens;
    let (paths, next) = use_tree::parse(&tokens, 1);
    let path = |segments: &[&str], binding: Option<&str>| UsePath {
        segments: segments
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect(),
        binding: binding.map(str::to_owned),
    };
    assert_eq!(
        paths,
        vec![
            path(&["crate", "first"], Some("first")),
            path(&["crate", "first", "Item"], Some("Alias")),
            path(&["crate", "second"], None),
        ],
    );
    assert_eq!(next, tokens.len());
}

fn crate_tree() -> MemoryTree {
    MemoryTree(BTreeMap::from([
        (
            "lib/src/lib.rs",
            "mod alpha;\npub(crate) mod beta;\nmod gamma;\n#[cfg(test)]\nmod tests;\npub use beta::Shared;\n",
        ),
        ("lib/src/alpha.rs", "use crate::Shared;\npub fn helper() {}\n"),
        ("lib/src/beta.rs", "mod inner;\npub struct Shared;\n"),
        ("lib/src/beta/inner.rs", "fn call() { super::super::alpha::helper(); }\n"),
        ("lib/src/gamma.rs", "// crate::alpha::helper\nfn standalone() {}\n"),
        ("lib/src/tests.rs", "use crate::gamma;\nuse crate::alpha;\n"),
    ]))
}

#[test]
fn module_graph_follows_reexports_and_nested_supers_and_skips_tests() {
    let graph = CrateModules::read(&crate_tree(), "lib").unwrap().top_level_graph();
    let edges = graph.edges.into_iter().collect::<Vec<_>>();
    let edge = |from: &str, to: &str| ((from.to_owned(), to.to_owned()), 1);
    assert_eq!(edges, vec![edge("alpha", "beta"), edge("beta", "alpha")]);
    let modules = graph.code_lines
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(modules, ["alpha", "beta", "gamma"]);
}

#[test]
fn reports_the_largest_cycle() {
    let graph = CrateModules::read(&crate_tree(), "lib").unwrap().top_level_graph();
    let report = CycleReport::new("lib", &graph);
    dbg!(&report);
    assert_eq!(report.size(), 2);
    assert_eq!(report.module_count, 3);
    assert!(
        report.largest.contains(&"alpha".to_owned()) && report.largest.contains(&"beta".to_owned()),
    );
}

#[test]
fn the_baseline_only_goes_down() {
    let graph = CrateModules::read(&crate_tree(), "lib").unwrap().top_level_graph();
    let reports = [CycleReport::new("lib", &graph)];
    let check = |entries: &[(&str, usize)]| {
        let baseline = entries
            .iter()
            .map(|(name, size)| ((*name).to_owned(), *size))
            .collect();
        ratchet::check(Metric::ModuleCycle, &cycles::sizes(&reports), &baseline)
    };
    let problem = |crate_name: &str, measured, baseline| {
        let crate_name = crate_name.to_owned();
        Problem::Ratchet(Mismatch { metric: Metric::ModuleCycle, crate_name, measured, baseline })
    };
    assert_eq!(check(&[("lib", 2)]), vec![]);
    assert_eq!(check(&[("lib", 1)]), vec![problem("lib", 2, 1)]);
    assert_eq!(check(&[("lib", 3)]), vec![problem("lib", 2, 3)]);
    let removed = problem("gone", 0, 2);
    assert_eq!(check(&[("lib", 2), ("gone", 2)]), vec![removed]);
}

#[test]
fn only_test_configurations_drop_a_module() {
    let lib = concat!(
        "#[cfg(test)] mod plain_test;\n",
        "#[cfg(all(test, unix))] mod unix_test;\n",
        "#[cfg(all(test, not(feature = \"slow\")))] mod fast_test;\n",
        "#[cfg(not(any(not(test), unix)))] mod double_negation_test;\n",
        "#[cfg(not(test))] mod production;\n",
        "#[cfg(any(test, unix))] mod unix_or_test;\n",
        "#[cfg(any(test, feature = \"testing\"))] pub mod shared;\n",
        "#[cfg(not(target_os = \"macos\"))] mod not_macos;\n",
        "#[cfg(unix)] mod platform;\n",
        "#[cfg(windows)] mod platform;\n",
        "mod r#raw_named;\n",
    );
    let tree = MemoryTree(BTreeMap::from([
        ("lib/src/lib.rs", lib),
        ("lib/src/production.rs", "fn run() -> impl Sized + use<> { crate::shared::helper() }\n"),
        ("lib/src/shared.rs", "pub fn helper() { crate::production::run(); }\n"),
        ("lib/src/unix_or_test.rs", "fn unix() {}\n"),
        ("lib/src/not_macos.rs", "fn other() {}\n"),
        ("lib/src/platform.rs", "fn os() {}\n"),
        ("lib/src/raw_named.rs", "fn raw() {}\n"),
    ]));
    let graph = CrateModules::read(&tree, "lib").unwrap().top_level_graph();
    let modules = graph.code_lines
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        modules,
        ["not_macos", "platform", "production", "raw_named", "shared", "unix_or_test"],
    );
    let edges = graph.edges.into_keys().collect::<Vec<_>>();
    let edge = |from: &str, to: &str| (from.to_owned(), to.to_owned());
    assert_eq!(edges, vec![edge("production", "shared"), edge("shared", "production")]);
}

#[test]
fn a_declared_module_without_a_file_is_an_error() {
    let tree = MemoryTree(BTreeMap::from([("lib/src/lib.rs", "mod missing;\n")]));
    let error = CrateModules::read(&tree, "lib").unwrap_err();
    assert_eq!(
        error,
        "lib/src/lib.rs declares `mod missing;`, but neither lib/src/missing.rs nor lib/src/missing/mod.rs exists",
    );
}
