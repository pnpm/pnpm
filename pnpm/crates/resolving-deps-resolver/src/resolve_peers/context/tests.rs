//! Unit tests for the peer-resolution context helpers.

use super::{
    ParentRef, importer_relative_link_dep_path, peer_segment_names, satisfies_with_prereleases,
    scoped_hoisted_optional_parent_refs,
};
use crate::node_id::NodeId;
use pacquet_deps_path::DepPath;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::Path;

const PATCHED_WORKFLOWS_SDK: &str = concat!(
    "@medusajs/workflows-sdk@2.13.3",
    "(patch_hash=248195172cff27c28650c005b6aa0aa3b2f2976f9739544b360b81668f2d8b59)",
    "(@types/node@20.19.17)",
    "(better-sqlite3@12.8.0)",
    "(express@4.21.2)",
);

#[test]
fn locked_direct_dep_only_sees_its_locked_hoisted_optional_peers() {
    let debug = NodeId::next();
    let supports_color = NodeId::next();
    let regular = NodeId::next();
    let parent = |version: &str, node_id| ParentRef {
        version: version.to_string(),
        node_id: Some(node_id),
        alias: None,
        depth: 0,
        occurrence: 0,
    };
    let parent_refs = HashMap::from_iter([
        ("debug".to_string(), parent("4.4.3", debug.clone())),
        ("supports-color".to_string(), parent("8.1.1", supports_color.clone())),
        ("regular".to_string(), parent("1.0.0", regular)),
    ]);

    let scoped = scoped_hoisted_optional_parent_refs(
        &parent_refs,
        &HashSet::from_iter(["supports-color".to_string()]),
        &HashSet::from_iter([debug, supports_color]),
    );

    assert!(!scoped.contains_key("debug"));
    assert!(scoped.contains_key("supports-color"));
    assert!(scoped.contains_key("regular"));
}

#[test]
fn importer_relative_self_link_keeps_an_empty_target() {
    let workspace = Path::new("workspace");
    assert_eq!(
        importer_relative_link_dep_path(&DepPath::from("link:."), Some(workspace), Some(workspace),),
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
                Some(Path::new("workspace")),
                Some(Path::new(project_dir)),
            ),
            expected,
            "unexpected link target for project dir {project_dir:?}",
        );
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
