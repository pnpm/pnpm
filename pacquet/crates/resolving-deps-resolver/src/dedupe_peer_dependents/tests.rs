use super::{DirectByImporter, dedupe_peer_dependents, deduplicate_dep_paths};
use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};
use pacquet_deps_path::DepPath;
use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
use pacquet_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

fn dp(raw: &str) -> DepPath {
    DepPath::from(raw.to_string())
}

fn make_node(
    pkg_id: &str,
    dep_path: &str,
    children: &[(&str, &str)],
    resolved_peers: &[&str],
) -> DependenciesGraphNode {
    DependenciesGraphNode {
        dep_path: dp(dep_path),
        resolved_package_id: pkg_id.to_string(),
        resolve_result: Arc::new(ResolveResult {
            id: PkgResolutionId::from(pkg_id.to_string()),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: None,
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "stub".to_string(),
            }),
            resolved_via: "registry".to_string(),
            normalized_bare_specifier: None,
            alias: None,
            policy_violation: None,
        }),
        children: children.iter().map(|(alias, child)| (alias.to_string(), dp(child))).collect(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::new(),
        resolved_peer_names: resolved_peers.iter().map(std::string::ToString::to_string).collect(),
        depth: 0,
        installable: true,
        is_pure: resolved_peers.is_empty(),
        optional: false,
    }
}

const SUBSET: &str = "foo@1.0.0(bar@1.0.0)";
const BAZ_VARIANT: &str = "foo@1.0.0(bar@1.0.0)(baz@1.0.0)";
const QUX_VARIANT: &str = "foo@1.0.0(bar@1.0.0)(qux@1.0.0)";

/// `foo` resolved into three peer-suffixed variants: `foo(bar)`,
/// `foo(bar)(baz)`, and `foo(bar)(qux)`. The subset `foo(bar)` is a
/// subset of both larger variants, which are incompatible with each
/// other, so the collapse target is a real choice.
fn build_graph() -> DependenciesGraph {
    let mut graph = DependenciesGraph::new();
    for (id, dep_path) in
        [("bar@1.0.0", "bar@1.0.0"), ("baz@1.0.0", "baz@1.0.0"), ("qux@1.0.0", "qux@1.0.0")]
    {
        graph.insert(dp(dep_path), make_node(id, dep_path, &[], &[]));
    }
    graph.insert(dp(SUBSET), make_node("foo@1.0.0", SUBSET, &[("bar", "bar@1.0.0")], &["bar"]));
    graph.insert(
        dp(BAZ_VARIANT),
        make_node(
            "foo@1.0.0",
            BAZ_VARIANT,
            &[("bar", "bar@1.0.0"), ("baz", "baz@1.0.0")],
            &["bar", "baz"],
        ),
    );
    graph.insert(
        dp(QUX_VARIANT),
        make_node(
            "foo@1.0.0",
            QUX_VARIANT,
            &[("bar", "bar@1.0.0"), ("qux", "qux@1.0.0")],
            &["bar", "qux"],
        ),
    );
    graph
}

/// The subset variant must collapse into the same larger variant no
/// matter the order the two equal-sized larger variants are presented in
/// — the determinism guarantee the total-order tie-break exists to
/// provide. Without the dep-path tie-break, swapping the two would flip
/// which one wins the collapse (pnpm/pnpm#12179).
#[test]
fn collapse_target_is_independent_of_variant_order() {
    let graph = build_graph();

    let (baz_first, _) =
        deduplicate_dep_paths(&[vec![dp(SUBSET), dp(BAZ_VARIANT), dp(QUX_VARIANT)]], &graph);
    let (qux_first, _) =
        deduplicate_dep_paths(&[vec![dp(SUBSET), dp(QUX_VARIANT), dp(BAZ_VARIANT)]], &graph);

    // `foo(bar)(qux)` wins because it is the lexically-greater of the two
    // equal-count variants and the sorter pops the greatest first.
    assert_eq!(baz_first.get(&dp(SUBSET)), Some(&dp(QUX_VARIANT)));
    assert_eq!(baz_first.get(&dp(SUBSET)), qux_first.get(&dp(SUBSET)));
}

/// End to end over [`dedupe_peer_dependents`]: an importer that resolved
/// `foo` to the subset variant has its direct dep rewritten to the
/// collapse target, and the now-orphaned subset snapshot is pruned from
/// the graph while both larger variants remain.
#[test]
fn rewrites_importer_direct_dep_and_prunes_orphan() {
    let mut graph = build_graph();
    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-subset".to_string(), BTreeMap::from([("foo".to_string(), dp(SUBSET))]));
    direct
        .insert("project-baz".to_string(), BTreeMap::from([("foo".to_string(), dp(BAZ_VARIANT))]));
    direct
        .insert("project-qux".to_string(), BTreeMap::from([("foo".to_string(), dp(QUX_VARIANT))]));

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project-subset"]["foo"], dp(QUX_VARIANT));
    assert_eq!(direct["project-baz"]["foo"], dp(BAZ_VARIANT));
    assert_eq!(direct["project-qux"]["foo"], dp(QUX_VARIANT));
    assert!(!graph.contains_key(&dp(SUBSET)), "collapsed variant should be pruned");
    assert!(graph.contains_key(&dp(BAZ_VARIANT)));
    assert!(graph.contains_key(&dp(QUX_VARIANT)));
}

/// Port of pnpm's `packages are not deduplicated when versions do not
/// match`
/// ([dedupeDepPaths.test.ts](https://github.com/pnpm/pnpm/blob/7f91ba4045/installing/deps-resolver/test/dedupeDepPaths.test.ts#L8)).
/// `foo` has an optional `baz` peer and a required `bar` peer pinned to
/// two different majors. The variant without `baz` collapses into the
/// one with `baz` *of the same `bar` major*, but never across `bar`
/// majors — so the two majors stay distinct.
#[test]
fn does_not_collapse_across_incompatible_peer_versions() {
    let bar1 = "foo@1.0.0(bar@1.0.0)";
    let bar1_baz = "foo@1.0.0(bar@1.0.0)(baz@1.0.0)";
    let bar2 = "foo@1.0.0(bar@2.0.0)";
    let bar2_baz = "foo@1.0.0(bar@2.0.0)(baz@2.0.0)";

    let mut graph = DependenciesGraph::new();
    for (id, dep_path) in [
        ("bar@1.0.0", "bar@1.0.0"),
        ("bar@2.0.0", "bar@2.0.0"),
        ("baz@1.0.0", "baz@1.0.0"),
        ("baz@2.0.0", "baz@2.0.0"),
    ] {
        graph.insert(dp(dep_path), make_node(id, dep_path, &[], &[]));
    }
    graph.insert(dp(bar1), make_node("foo@1.0.0", bar1, &[("bar", "bar@1.0.0")], &["bar"]));
    graph.insert(dp(bar2), make_node("foo@1.0.0", bar2, &[("bar", "bar@2.0.0")], &["bar"]));
    graph.insert(
        dp(bar1_baz),
        make_node(
            "foo@1.0.0",
            bar1_baz,
            &[("bar", "bar@1.0.0"), ("baz", "baz@1.0.0")],
            &["bar", "baz"],
        ),
    );
    graph.insert(
        dp(bar2_baz),
        make_node(
            "foo@1.0.0",
            bar2_baz,
            &[("bar", "bar@2.0.0"), ("baz", "baz@2.0.0")],
            &["bar", "baz"],
        ),
    );

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project1".to_string(), BTreeMap::from([("foo".to_string(), dp(bar1))]));
    direct.insert("project2".to_string(), BTreeMap::from([("foo".to_string(), dp(bar1_baz))]));
    direct.insert("project3".to_string(), BTreeMap::from([("foo".to_string(), dp(bar2))]));
    direct.insert("project4".to_string(), BTreeMap::from([("foo".to_string(), dp(bar2_baz))]));

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project1"]["foo"], direct["project2"]["foo"]);
    assert_ne!(direct["project1"]["foo"], direct["project3"]["foo"]);
    assert_eq!(direct["project3"]["foo"], direct["project4"]["foo"]);
}

/// A package whose two variants are mutually incompatible (neither's
/// children/peers subset the other) must not collapse — both survive and
/// no remap happens.
#[test]
fn incompatible_variants_do_not_collapse() {
    let mut graph = build_graph();
    // Drop the subset so only the two incompatible variants remain.
    graph.remove(&dp(SUBSET));

    let mut direct: DirectByImporter = BTreeMap::new();
    direct
        .insert("project-baz".to_string(), BTreeMap::from([("foo".to_string(), dp(BAZ_VARIANT))]));
    direct
        .insert("project-qux".to_string(), BTreeMap::from([("foo".to_string(), dp(QUX_VARIANT))]));

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project-baz"]["foo"], dp(BAZ_VARIANT));
    assert_eq!(direct["project-qux"]["foo"], dp(QUX_VARIANT));
    assert!(graph.contains_key(&dp(BAZ_VARIANT)));
    assert!(graph.contains_key(&dp(QUX_VARIANT)));
}
