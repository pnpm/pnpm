//! Unit tests for the peer-resolution context helpers.

use super::{
    ChainSuffixMemo, SharedChain, importer_relative_link_dep_path, peer_segment_names,
    remap_link_node_id, satisfies_with_prereleases,
};
use crate::{
    node_id::NodeId,
    resolve_peers::{ResolvePeersOptions, test_support::linked_package},
};
use pnpm_deps_path::DepPath;
use std::path::{Path, PathBuf};

const PATCHED_WORKFLOWS_SDK: &str = concat!(
    "@medusajs/workflows-sdk@2.13.3",
    "(patch_hash=248195172cff27c28650c005b6aa0aa3b2f2976f9739544b360b81668f2d8b59)",
    "(@types/node@20.19.17)",
    "(better-sqlite3@12.8.0)",
    "(express@4.21.2)",
);

#[test]
fn importer_relative_self_link_keeps_an_empty_target() {
    let workspace = Path::new("workspace");
    assert_eq!(
        importer_relative_link_dep_path(
            &DepPath::from("link:."),
            &crate::link_target::ImporterAnchor::new(workspace, workspace),
            Some(workspace),
            Some(workspace),
        ),
        DepPath::from("link:"),
    );
}

/// The link target is relative to the importer, so a project dir that
/// still carries `.` / `..` segments must be normalized before it is used
/// as the base — otherwise those segments are counted as real directories
/// and the target gains extra `..` hops.
#[test]
fn importer_relative_link_normalizes_the_project_dir() {
    let expected = DepPath::from("link:../lib");
    for project_dir in
        ["workspace/packages/app", "workspace/packages/./app", "workspace/packages/nested/../app"]
    {
        assert_eq!(
            importer_relative_link_dep_path(
                &DepPath::from("link:packages/lib"),
                &crate::link_target::ImporterAnchor::new(
                    Path::new(project_dir),
                    Path::new("workspace"),
                ),
                Some(Path::new("workspace")),
                Some(Path::new(project_dir)),
            ),
            expected,
            "unexpected link target for project dir {project_dir:?}",
        );
    }
}

/// A workspace link records its `directory` relative to the importer,
/// so the containment check has to resolve it against `project_dir`
/// first — comparing the relative form with `lockfile_dir` never
/// matches and remaps links that are already stable across machines.
#[test]
fn workspace_internal_link_is_not_remapped() {
    for directory in ["../lib", "/ws/packages/lib"] {
        let dep = linked_package("lib", "link:packages/lib", directory);
        assert_eq!(
            remap_link_node_id(&exclude_links_opts(), "lib", &dep.result),
            None,
            "unexpected remap of internal link target {directory:?}",
        );
    }
}

#[test]
fn external_link_is_remapped_to_the_importers_modules_dir() {
    for directory in ["../../../outside/lib", "/outside/lib"] {
        let dep = linked_package("lib", "link:../../../outside/lib", directory);
        assert_eq!(
            remap_link_node_id(&exclude_links_opts(), "lib", &dep.result),
            Some(NodeId::leaf("link:packages/app/node_modules/lib")),
            "unexpected remap of external link target {directory:?}",
        );
    }
}

/// An injected workspace dependency is a real package in the graph
/// rather than a link, and records its `directory` relative to
/// `lockfile_dir`.
#[test]
fn injected_workspace_dep_is_not_remapped() {
    let dep = linked_package("lib", "file:../outside/lib", "../outside/lib");
    assert_eq!(remap_link_node_id(&exclude_links_opts(), "lib", &dep.result), None);
}

fn exclude_links_opts() -> ResolvePeersOptions {
    ResolvePeersOptions {
        exclude_links_from_lockfile: true,
        lockfile_dir: Some(PathBuf::from("/ws")),
        project_dir: Some(PathBuf::from("/ws/packages/app")),
        modules_dir: Some(PathBuf::from("/ws/packages/app/node_modules")),
        ..ResolvePeersOptions::default()
    }
}

#[test]
fn parses_peer_suffix_after_patch_hash() {
    let dep_path = DepPath::from(PATCHED_WORKFLOWS_SDK);
    assert_eq!(
        peer_segment_names(&dep_path),
        Some(vec!["@types/node".to_string(), "better-sqlite3".to_string(), "express".to_string(),]),
    );
}

#[test]
fn satisfies_handles_basic_ranges() {
    assert!(satisfies_with_prereleases("1.2.3", "^1.0.0"));
    assert!(!satisfies_with_prereleases("2.0.0", "^1.0.0"));
    assert!(satisfies_with_prereleases("18.0.0", "*"));
}

#[test]
fn satisfies_falls_back_to_equality_for_unparsable_ranges() {
    assert!(satisfies_with_prereleases("workspace:^1.0.0", "workspace:^1.0.0"));
    assert!(!satisfies_with_prereleases("1.0.0", "workspace:^1.0.0"));
}

#[test]
fn satisfies_accepts_prerelease_against_non_prerelease_range() {
    assert!(satisfies_with_prereleases("18.0.0-rc.1", "^18.0.0"));
    assert!(satisfies_with_prereleases("1.2.3-beta.0", "^1.2.0"));
    assert!(!satisfies_with_prereleases("19.0.0-rc.1", "^18.0.0"));
}

/// Strong count of a chain's tip link, for asserting who holds it.
fn link_strong_count(chain: &SharedChain<String>) -> usize {
    chain.0.as_ref().map_or(0, std::sync::Arc::strong_count)
}

/// Build `root -> ... -> tip` and return the chain at the tip.
fn chain_of(values: &[&str]) -> SharedChain<String> {
    values.iter().fold(SharedChain::default(), |chain, value| chain.pushed((*value).to_string()))
}

#[test]
fn memoized_any_matches_the_unmemoized_answer() {
    let shared = chain_of(&["root", "middle"]);
    let with_match = shared.pushed("needle".to_string());
    let without_match = shared.pushed("other".to_string());

    let mut memo = ChainSuffixMemo::default();
    assert!(with_match.any_memoized(&mut memo, |value| value == "needle"));
    assert!(!without_match.any_memoized(&mut memo, |value| value == "needle"));

    let mut fresh = ChainSuffixMemo::default();
    assert_eq!(
        with_match.any_memoized(&mut fresh, |value| value == "needle"),
        with_match.iter().any(|value| value == "needle"),
    );
}

#[test]
fn a_match_in_a_shared_suffix_answers_every_chain_built_on_it() {
    let shared = chain_of(&["root", "needle"]);
    let branches: Vec<_> =
        ["a", "b", "c"].iter().map(|tip| shared.pushed((*tip).to_string())).collect();

    let mut memo = ChainSuffixMemo::default();
    let mut visits = 0;
    for branch in &branches {
        assert!(branch.any_memoized(&mut memo, |value| {
            visits += 1;
            value == "needle"
        }));
    }

    assert_eq!(
        visits, 2,
        "the two shared links answer once, and a suffix that already matched \
         spares every branch's own tip",
    );
}

#[test]
fn an_unmatched_shared_suffix_is_still_evaluated_only_once() {
    let shared = chain_of(&["root", "plain"]);
    let branches: Vec<_> =
        ["a", "b", "c"].iter().map(|tip| shared.pushed((*tip).to_string())).collect();

    let mut memo = ChainSuffixMemo::default();
    let mut visits = 0;
    for branch in &branches {
        assert!(!branch.any_memoized(&mut memo, |value| {
            visits += 1;
            value == "needle"
        }));
    }

    assert_eq!(visits, 5, "two shared links once, plus each branch's own tip");
}

#[test]
fn suffixes_evaluated_under_one_predicate_are_not_reused_for_another() {
    let chain = chain_of(&["root", "needle"]);

    let mut memo = ChainSuffixMemo::default();
    assert!(chain.any_memoized(&mut memo, |value| value == "needle"));

    let mut other = ChainSuffixMemo::default();
    assert!(!chain.any_memoized(&mut other, |value| value == "absent"));
}

#[test]
fn a_memo_keeps_the_links_it_keyed_on_alive() {
    let mut memo = ChainSuffixMemo::default();
    let root = chain_of(&["root"]);

    let held_by = {
        let temporary = root.pushed("temporary".to_string());
        assert!(temporary.any_memoized(&mut memo, |value| value == "temporary"));
        link_strong_count(&temporary)
    };

    // The chain that produced the entry is gone, but the memo's own
    // reference keeps its link — and therefore its address — reserved,
    // so nothing else can be allocated there and inherit the answer.
    assert_eq!(held_by, 2, "the memo holds a reference alongside the caller's");
    assert!(!chain_of(&["root", "other"]).any_memoized(&mut memo, |value| value == "temporary"));
}
