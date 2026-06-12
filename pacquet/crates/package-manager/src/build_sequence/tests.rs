use super::build_sequence;
use crate::SkippedSnapshots;
use pacquet_lockfile::{
    PackageKey, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pacquet_patching::ExtendedPatchInfo;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

/// Build a `requires_build` map for tests from a list of (key, `requires_build`)
/// pairs. Mirrors the per-snapshot map the runtime computes from each
/// extracted package's `pkg_requires_build`.
fn requires<const LEN: usize>(entries: [(PackageKey, bool); LEN]) -> HashMap<PackageKey, bool> {
    entries.into_iter().collect()
}

fn snap(deps: &[(&str, &str)]) -> SnapshotEntry {
    let map: HashMap<PkgName, SnapshotDepRef> =
        deps.iter().map(|(n, v)| (name(n), SnapshotDepRef::Plain(ver(v)))).collect();
    SnapshotEntry {
        id: None,
        dependencies: (!map.is_empty()).then_some(map),
        optional_dependencies: None,
        transitive_peer_dependencies: None,
        patched: None,
        optional: false,
    }
}

fn importer(deps: &[(&str, &str)]) -> ProjectSnapshot {
    let map: ResolvedDependencyMap = deps
        .iter()
        .map(|(n, v)| {
            (
                name(n),
                ResolvedDependencySpec { specifier: (*v).to_string(), version: ver(v).into() },
            )
        })
        .collect();
    ProjectSnapshot {
        specifiers: None,
        dependencies: (!map.is_empty()).then_some(map),
        optional_dependencies: None,
        dev_dependencies: None,
        dependencies_meta: None,
        publish_directory: None,
    }
}

fn root_importers(deps: &[(&str, &str)]) -> HashMap<String, ProjectSnapshot> {
    HashMap::from([(".".to_string(), importer(deps))])
}

#[test]
fn empty_inputs() {
    let chunks = build_sequence(
        &HashMap::new(),
        None,
        &HashMap::new(),
        &HashMap::new(),
        &SkippedSnapshots::default(),
    );
    dbg!(&chunks);
    assert!(chunks.is_empty(), "empty inputs ⇒ no chunks: {chunks:?}");
}

#[test]
fn no_requires_build_yields_empty() {
    let snapshots = HashMap::from([
        (key("a", "1.0.0"), snap(&[("b", "1.0.0")])),
        (key("b", "1.0.0"), snap(&[])),
    ]);
    let requires_build = requires([(key("a", "1.0.0"), false), (key("b", "1.0.0"), false)]);
    let importers = root_importers(&[("a", "1.0.0")]);

    let chunks =
        build_sequence(&requires_build, None, &snapshots, &importers, &SkippedSnapshots::default());
    dbg!(&chunks);
    assert!(chunks.is_empty(), "no requires_build ⇒ no chunks: {chunks:?}");
}

#[test]
fn leaf_with_requires_build_runs_first() {
    // a depends on b; only b requires build. Both nodes are added to the
    // build sequence (a is an ancestor of a buildable node), but the order
    // must be b before a.
    let snapshots = HashMap::from([
        (key("a", "1.0.0"), snap(&[("b", "1.0.0")])),
        (key("b", "1.0.0"), snap(&[])),
    ]);
    let requires_build = requires([(key("a", "1.0.0"), false), (key("b", "1.0.0"), true)]);
    let importers = root_importers(&[("a", "1.0.0")]);

    let chunks =
        build_sequence(&requires_build, None, &snapshots, &importers, &SkippedSnapshots::default());
    assert_eq!(chunks, vec![vec![key("b", "1.0.0")], vec![key("a", "1.0.0")]]);
}

#[test]
fn deep_chain_orders_leaf_first() {
    // a -> b -> c, only c requires build. Sequence: [c], [b], [a].
    let snapshots = HashMap::from([
        (key("a", "1.0.0"), snap(&[("b", "1.0.0")])),
        (key("b", "1.0.0"), snap(&[("c", "1.0.0")])),
        (key("c", "1.0.0"), snap(&[])),
    ]);
    let requires_build = requires([
        (key("a", "1.0.0"), false),
        (key("b", "1.0.0"), false),
        (key("c", "1.0.0"), true),
    ]);
    let importers = root_importers(&[("a", "1.0.0")]);

    let chunks =
        build_sequence(&requires_build, None, &snapshots, &importers, &SkippedSnapshots::default());
    assert_eq!(
        chunks,
        vec![vec![key("c", "1.0.0")], vec![key("b", "1.0.0")], vec![key("a", "1.0.0")]],
    );
}

#[test]
fn unrelated_subgraph_excluded() {
    // a -> b (b builds), x -> y (y builds). Importer only depends on a.
    // Only the `a` subgraph should appear.
    let snapshots = HashMap::from([
        (key("a", "1.0.0"), snap(&[("b", "1.0.0")])),
        (key("b", "1.0.0"), snap(&[])),
        (key("x", "1.0.0"), snap(&[("y", "1.0.0")])),
        (key("y", "1.0.0"), snap(&[])),
    ]);
    let requires_build = requires([
        (key("a", "1.0.0"), false),
        (key("b", "1.0.0"), true),
        (key("x", "1.0.0"), false),
        (key("y", "1.0.0"), true),
    ]);
    let importers = root_importers(&[("a", "1.0.0")]);

    let chunks =
        build_sequence(&requires_build, None, &snapshots, &importers, &SkippedSnapshots::default());
    let flat: Vec<_> = chunks.into_iter().flatten().collect();
    dbg!(&flat);
    assert!(flat.contains(&key("a", "1.0.0")), "ancestor of build leaf must appear: {flat:?}");
    assert!(flat.contains(&key("b", "1.0.0")), "build leaf must appear: {flat:?}");
    assert!(!flat.contains(&key("x", "1.0.0")), "unreachable ancestor must be excluded: {flat:?}");
    assert!(
        !flat.contains(&key("y", "1.0.0")),
        "unreachable build leaf must be excluded: {flat:?}",
    );
}

#[test]
fn parallel_build_leaves_share_chunk() {
    // root depends on a and b; both a and b have requires_build but no shared
    // descendants. Both build leaves should land in the same chunk; root
    // follows in the next chunk as their ancestor.
    let snapshots = HashMap::from([
        (key("root", "1.0.0"), snap(&[("a", "1.0.0"), ("b", "1.0.0")])),
        (key("a", "1.0.0"), snap(&[])),
        (key("b", "1.0.0"), snap(&[])),
    ]);
    let requires_build = requires([
        (key("root", "1.0.0"), false),
        (key("a", "1.0.0"), true),
        (key("b", "1.0.0"), true),
    ]);
    let importers = root_importers(&[("root", "1.0.0")]);

    let chunks =
        build_sequence(&requires_build, None, &snapshots, &importers, &SkippedSnapshots::default());
    assert_eq!(chunks.len(), 2);
    let mut leaves = chunks[0].clone();
    leaves.sort_by_key(std::string::ToString::to_string);
    assert_eq!(leaves, vec![key("a", "1.0.0"), key("b", "1.0.0")]);
    assert_eq!(chunks[1], vec![key("root", "1.0.0")]);
}

/// Direct port of upstream
/// [`'buildSequence() test 2'`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/during-install/test/buildSequence.test.ts#L28-L51).
///
/// Two importers `a` and `b` both depend on a shared builder leaf
/// `c`. Only `a` requires its own build; `b` does not. The result
/// must surface `c` in the first chunk and `a` in the second — `b`
/// is *trimmed* from the build sequence because it neither needs a
/// build itself nor has a buildable descendant that's exclusive to
/// it (its descendant `c` is already scheduled via `a`).
///
/// This is the subgraph-trim case for [#397] item `#16`. Pacquet's
/// existing `unrelated_subgraph_excluded` covers a stronger
/// scenario (an entirely unreachable subgraph); this one pins the
/// upstream-equivalent behavior where an importer that's still in
/// the install set gets dropped from the build sequence.
///
/// [#397]: https://github.com/pnpm/pacquet/issues/397
#[test]
fn non_builder_importer_with_shared_builder_child_is_trimmed() {
    let snapshots = HashMap::from([
        (key("a", "1.0.0"), snap(&[("c", "1.0.0")])),
        (key("b", "1.0.0"), snap(&[("c", "1.0.0")])),
        (key("c", "1.0.0"), snap(&[])),
    ]);
    let requires_build = requires([
        (key("a", "1.0.0"), true),
        (key("b", "1.0.0"), false),
        (key("c", "1.0.0"), true),
    ]);
    let importers = root_importers(&[("a", "1.0.0"), ("b", "1.0.0")]);

    let chunks =
        build_sequence(&requires_build, None, &snapshots, &importers, &SkippedSnapshots::default());
    assert_eq!(chunks, vec![vec![key("c", "1.0.0")], vec![key("a", "1.0.0")]]);
}

/// A snapshot marked in the skip set must NOT enter the build queue
/// even if it carries a configured patch. Upstream's `lockfileToDepGraph`
/// excludes skipped nodes from the depGraph entirely, so the patch
/// lookup never finds them. Without this gate, `build_one_snapshot`'s
/// `pkg_dir.exists()` defensive return would still suppress the
/// actual build attempt — but the snapshot would have been queued
/// and the graph walked through it. The gate makes the exclusion
/// correct-by-construction.
#[test]
fn skipped_patched_snapshot_does_not_enter_build_queue() {
    use std::collections::HashSet;
    use std::path::PathBuf;

    let a_key = key("a", "1.0.0");
    let snapshots = HashMap::from([(a_key.clone(), snap(&[]))]);
    let requires_build = requires([(a_key.clone(), false)]);
    let importers = root_importers(&[("a", "1.0.0")]);

    // `a@1.0.0` has a patch configured AND would normally trigger
    // a build via `has_patch` alone (requires_build = false).
    let patches = HashMap::from([(
        a_key.clone(),
        ExtendedPatchInfo {
            hash: "fake-hash".to_string(),
            patch_file_path: Some(PathBuf::from("/dev/null")),
            key: "a@1.0.0".to_string(),
        },
    )]);

    let skipped = SkippedSnapshots::from_set(HashSet::from([a_key]));

    let chunks = build_sequence(&requires_build, Some(&patches), &snapshots, &importers, &skipped);

    assert!(
        chunks.is_empty(),
        "skipped+patched snapshot must not be queued for build, got {chunks:?}",
    );
}

/// A snapshot reachable *only* via a skipped optional parent must not
/// enter the build queue, even if it requires a build. Pnpm's
/// `lockfileToDepGraph` removes skipped depPaths from the graph
/// entirely, so descendants reachable only via that edge are
/// effectively orphans in the build phase.
///
/// Setup: root → S (skipped) → C (`requires_build`). Without the
/// skip-before-recurse gate, the walk would step through S into C,
/// see C as buildable, and queue both C and ancestors that look like
/// they need to be sequenced before C. With the gate, S's subtree
/// isn't visited; C never enters the queue.
#[test]
fn skipped_parent_does_not_drag_descendants_into_build_queue() {
    use std::collections::HashSet;

    let root_key = key("root", "1.0.0");
    let s_key = key("s", "1.0.0");
    let c_key = key("c", "1.0.0");
    let snapshots = HashMap::from([
        (root_key.clone(), snap(&[("s", "1.0.0")])),
        (s_key.clone(), snap(&[("c", "1.0.0")])),
        (c_key.clone(), snap(&[])),
    ]);
    let requires_build = requires([(root_key, false), (s_key.clone(), false), (c_key, true)]);
    let importers = root_importers(&[("root", "1.0.0")]);

    let skipped = SkippedSnapshots::from_set(HashSet::from([s_key]));

    let chunks = build_sequence(&requires_build, None, &snapshots, &importers, &skipped);

    assert!(
        chunks.is_empty(),
        "C (buildable) reachable only via skipped S must not be queued, got {chunks:?}",
    );
}

/// A snapshot reachable via BOTH a skipped parent and a non-skipped
/// parent must still enter the build queue if it requires building —
/// pnpm doesn't propagate "skipped" status to descendants reached by
/// any other (non-skipped) path. This pins that the
/// skip-before-recurse gate doesn't accidentally poison `walked` for
/// the alternate branch.
///
/// Setup: root → {S (skipped), B}, both S and B → C (`requires_build`).
/// Even though S is skipped, B still pulls C into the build graph.
#[test]
fn descendant_with_non_skipped_parent_still_builds() {
    use std::collections::HashSet;

    let root_key = key("root", "1.0.0");
    let s_key = key("s", "1.0.0");
    let b_key = key("b", "1.0.0");
    let c_key = key("c", "1.0.0");
    let snapshots = HashMap::from([
        (root_key.clone(), snap(&[("s", "1.0.0"), ("b", "1.0.0")])),
        (s_key.clone(), snap(&[("c", "1.0.0")])),
        (b_key.clone(), snap(&[("c", "1.0.0")])),
        (c_key.clone(), snap(&[])),
    ]);
    let requires_build = requires([
        (root_key.clone(), false),
        (s_key.clone(), false),
        (b_key.clone(), false),
        (c_key.clone(), true),
    ]);
    let importers = root_importers(&[("root", "1.0.0")]);

    let skipped = SkippedSnapshots::from_set(HashSet::from([s_key]));

    let chunks = build_sequence(&requires_build, None, &snapshots, &importers, &skipped);

    let flat: Vec<_> = chunks.into_iter().flatten().collect();
    assert!(flat.contains(&c_key), "C reached via non-skipped B must build, got {flat:?}");
    assert!(flat.contains(&b_key), "B (ancestor of buildable C) must appear, got {flat:?}");
    assert!(
        flat.contains(&root_key),
        "root (ancestor of buildable subtree) must appear, got {flat:?}",
    );
}
