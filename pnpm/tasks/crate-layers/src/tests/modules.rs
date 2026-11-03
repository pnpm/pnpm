use super::MemoryTree;
use crate::{
    cycles::{self, CycleReport},
    layers::Problem,
    lexer::{self, Token},
    modules::CrateModules,
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
        cycles::check_baseline(&reports, &baseline)
    };
    assert_eq!(check(&[("lib", 2)]), vec![]);
    let grew = Problem::ModuleCycleGrew { crate_name: "lib".to_owned(), size: 2, baseline: 1 };
    assert_eq!(check(&[("lib", 1)]), vec![grew]);
    let shrank = Problem::ModuleCycleShrank { crate_name: "lib".to_owned(), size: 2, baseline: 3 };
    assert_eq!(check(&[("lib", 3)]), vec![shrank]);
    let removed =
        Problem::ModuleCycleShrank { crate_name: "gone".to_owned(), size: 0, baseline: 2 };
    assert_eq!(check(&[("lib", 2), ("gone", 2)]), vec![removed]);
}
