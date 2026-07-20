//! Ports of the upstream `createPackagesSearcher` tests
//! (deps/inspection/tree-builder/test/createPackagesSearcher.spec.ts).

use std::collections::HashMap;

use pretty_assertions::assert_eq;

use super::{SearchMatch, Searcher};
use crate::cli_args::deps_tree::TreeNodeId;

fn searcher(queries: &[&str]) -> Searcher {
    let queries: Vec<String> = queries.iter().copied().map(str::to_string).collect();
    Searcher::from_queries(&queries).unwrap()
}

// Port of upstream's 'packages searcher' (deps/inspection/tree-builder/test/createPackagesSearcher.spec.ts).
#[test]
fn packages_searcher() {
    let search = searcher(&["rimraf@*"]);
    assert_eq!(search.matches("rimraf", "rimraf", "1.0.0", None), SearchMatch::Yes);
    assert_eq!(search.matches("express", "express", "1.0.0", None), SearchMatch::No);

    let search = searcher(&["rim*"]);
    assert_eq!(search.matches("rimraf", "rimraf", "1.0.0", None), SearchMatch::Yes);
    assert_eq!(search.matches("express", "express", "1.0.0", None), SearchMatch::No);

    let search = searcher(&["rim*@2"]);
    assert_eq!(search.matches("rimraf", "rimraf", "2.0.0", None), SearchMatch::Yes);
    assert_eq!(search.matches("rimraf", "rimraf", "1.0.0", None), SearchMatch::No);

    let search = searcher(&["minimatch", "once@1.4"]);
    assert_eq!(search.matches("minimatch", "minimatch", "2.0.0", None), SearchMatch::Yes);
    assert_eq!(search.matches("once", "once", "1.4.1", None), SearchMatch::Yes);
    assert_eq!(search.matches("rimraf", "rimraf", "1.0.0", None), SearchMatch::No);
}

// Rust counterpart of upstream's 'package searcher with 2 finders'
// (deps/inspection/tree-builder/test/createPackagesSearcher.spec.ts).
// Finders are JavaScript callbacks evaluated in the pnpmfile worker, so the
// searcher consumes pre-computed verdicts keyed by `(alias, node)`; several
// message-returning finders arrive already combined into one newline-joined
// message.
#[test]
fn finder_results_are_consulted_by_alias_and_node() {
    let once = TreeNodeId::Package("once@1.4.1".parse().unwrap());
    let rimraf = TreeNodeId::Package("rimraf@1.0.0".parse().unwrap());
    let minimatch = TreeNodeId::Package("minimatch@2.0.0".parse().unwrap());

    let mut search = Searcher::from_queries(&[]).unwrap();
    search.set_finder_results(HashMap::from([
        (("once".to_string(), Some(once.clone())), SearchMatch::Yes),
        (
            ("rimraf".to_string(), Some(rimraf.clone())),
            SearchMatch::Message("found by finder one\nfound by finder two".to_string()),
        ),
    ]));

    assert_eq!(
        search.matches("minimatch", "minimatch", "2.0.0", Some(&minimatch)),
        SearchMatch::No,
    );
    assert_eq!(search.matches("once", "once", "1.4.1", Some(&once)), SearchMatch::Yes);
    assert_eq!(
        search.matches("rimraf", "rimraf", "1.0.0", Some(&rimraf)),
        SearchMatch::Message("found by finder one\nfound by finder two".to_string()),
    );
    // A verdict recorded for one node does not apply to the same alias
    // resolved to a different node.
    assert_eq!(search.matches("once", "once", "1.4.1", Some(&minimatch)), SearchMatch::No);
}

// Positional queries are checked before finder verdicts, mirroring the
// upstream searcher, which returns a plain `true` for a query match without
// consulting the finders.
#[test]
fn queries_are_checked_before_finder_results() {
    let once = TreeNodeId::Package("once@1.4.1".parse().unwrap());
    let mut search = searcher(&["once"]);
    search.set_finder_results(HashMap::from([(
        ("once".to_string(), Some(once.clone())),
        SearchMatch::Message("finder message".to_string()),
    )]));

    assert_eq!(search.matches("once", "once", "1.4.1", Some(&once)), SearchMatch::Yes);
}
