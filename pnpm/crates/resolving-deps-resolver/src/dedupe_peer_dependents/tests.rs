use super::{DirectByImporter, dedupe_peer_dependents, deduplicate_dep_paths};
use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::FxHashSet as HashSet;
use std::{collections::BTreeMap, sync::Arc};

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
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "stub".to_string(),
            }),
            resolved_via: "registry".to_string(),
            normalized_bare_specifier: None,
            alias: None,
            policy_violation: None,
            package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: None,
            },
        }),
        depth: 0,
        installable: true,
        is_pure: resolved_peers.is_empty(),
        optional: false,
        edges: crate::ResolvedDependencyEdges {
            children: children
                .iter()
                .map(|(alias, child)| (alias.to_string(), dp(child)))
                .collect(),
            optional_children: HashSet::default(),
            peer_dependencies: BTreeMap::new(),
            transitive_peer_dependencies: HashSet::default(),
            resolved_peer_names: resolved_peers
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        },
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
    let mut graph = DependenciesGraph::default();
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

#[test]
fn rewrites_importer_direct_dep_and_prunes_orphan() {
    let mut graph = build_graph();
    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-subset".to_string(), BTreeMap::from([("foo".to_string(), dp(SUBSET))]));
    direct.insert(
        "project-baz".to_string(),
        BTreeMap::from([("foo".to_string(), dp(BAZ_VARIANT))]),
    );
    direct.insert(
        "project-qux".to_string(),
        BTreeMap::from([("foo".to_string(), dp(QUX_VARIANT))]),
    );

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project-subset"]["foo"], dp(QUX_VARIANT));
    assert_eq!(direct["project-baz"]["foo"], dp(BAZ_VARIANT));
    assert_eq!(direct["project-qux"]["foo"], dp(QUX_VARIANT));
    assert!(!graph.contains_key(&dp(SUBSET)), "collapsed variant should be pruned");
    assert!(graph.contains_key(&dp(BAZ_VARIANT)));
    assert!(graph.contains_key(&dp(QUX_VARIANT)));
}

#[test]
fn does_not_collapse_across_incompatible_peer_versions() {
    let bar1 = "foo@1.0.0(bar@1.0.0)";
    let bar1_baz = "foo@1.0.0(bar@1.0.0)(baz@1.0.0)";
    let bar2 = "foo@1.0.0(bar@2.0.0)";
    let bar2_baz = "foo@1.0.0(bar@2.0.0)(baz@2.0.0)";

    let mut graph = DependenciesGraph::default();
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

/// Every reference to a collapsed variant is rewritten, including a
/// consumer's child edge whose own peer suffix names the collapsed
/// variant — pnpm's `deduplicateAll` rewrites `node.children`
/// unconditionally, and leaving one edge behind keeps the collapsed
/// variant alive in the lockfile.
#[test]
fn a_consumers_child_edge_follows_the_collapse() {
    let subset = "foo@1.0.0(bar@1.0.0)";
    let larger = "foo@1.0.0(bar@1.0.0)(baz@1.0.0)";
    let baz = "baz@1.0.0(qux@1.0.0)";
    let consumer = "consumer@1.0.0(foo@1.0.0(bar@1.0.0))(bar@1.0.0)";

    let mut graph = DependenciesGraph::default();
    for (id, dep_path) in [("bar@1.0.0", "bar@1.0.0"), ("qux@1.0.0", "qux@1.0.0")] {
        graph.insert(dp(dep_path), make_node(id, dep_path, &[], &[]));
    }
    graph.insert(dp(baz), make_node("baz@1.0.0", baz, &[("qux", "qux@1.0.0")], &["qux"]));
    graph.insert(dp(subset), make_node("foo@1.0.0", subset, &[("bar", "bar@1.0.0")], &["bar"]));
    graph.insert(
        dp(larger),
        make_node("foo@1.0.0", larger, &[("bar", "bar@1.0.0"), ("baz", baz)], &["bar", "baz"]),
    );
    graph.insert(
        dp(consumer),
        make_node(
            "consumer@1.0.0",
            consumer,
            &[("foo", subset), ("bar", "bar@1.0.0")],
            &["foo", "bar"],
        ),
    );

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-subset".to_string(), BTreeMap::from([("foo".to_string(), dp(subset))]));
    direct.insert("project-larger".to_string(), BTreeMap::from([("foo".to_string(), dp(larger))]));
    direct.insert(
        "project-consumer".to_string(),
        BTreeMap::from([("consumer".to_string(), dp(consumer))]),
    );

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project-subset"]["foo"], dp(larger));
    assert_eq!(direct["project-larger"]["foo"], dp(larger));
    assert_eq!(graph[&dp(consumer)].edges.children["foo"], dp(larger));
    assert!(!graph.contains_key(&dp(subset)), "the collapsed variant has no reference left");
    assert!(graph.contains_key(&dp(larger)));
}

#[test]
fn incompatible_variants_do_not_collapse() {
    let mut graph = build_graph();
    graph.remove(&dp(SUBSET));

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert(
        "project-baz".to_string(),
        BTreeMap::from([("foo".to_string(), dp(BAZ_VARIANT))]),
    );
    direct.insert(
        "project-qux".to_string(),
        BTreeMap::from([("foo".to_string(), dp(QUX_VARIANT))]),
    );

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project-baz"]["foo"], dp(BAZ_VARIANT));
    assert_eq!(direct["project-qux"]["foo"], dp(QUX_VARIANT));
    assert!(graph.contains_key(&dp(BAZ_VARIANT)));
    assert!(graph.contains_key(&dp(QUX_VARIANT)));
}

/// The child variants cannot all collapse in one round: `child(other)`
/// absorbs neither `child(opt_peer)` nor the other way round, so the
/// child group still holds a leftover when the round ends and the graph's
/// child edges are never rewritten. The parents must therefore collapse
/// on the strength of their children being compatible variants of one
/// package, not on their child depPaths being equal
/// ([pnpm/pnpm#14800](https://github.com/pnpm/pnpm/issues/14800)).
#[test]
fn parent_collapses_when_its_child_carries_a_peer_suffix_the_other_lacks() {
    const CHILD_WITH_OPT_PEER: &str = "child@1.0.0(opt_peer@1.0.0)";
    const CHILD_WITH_OTHER: &str = "child@1.0.0(other@1.0.0)";
    const CHILD_BARE: &str = "child@1.0.0";
    const PARENT_WITH_OPT_PEER: &str = "parent@1.0.0(opt_peer@1.0.0)";
    const PARENT_WITH_OTHER: &str = "parent@1.0.0(other@1.0.0)";
    const PARENT_BARE: &str = "parent@1.0.0";

    let mut graph = DependenciesGraph::default();
    for peer in ["opt_peer@1.0.0", "other@1.0.0"] {
        graph.insert(dp(peer), make_node(peer, peer, &[], &[]));
    }
    for (child, peers) in [
        (CHILD_WITH_OPT_PEER, &["opt_peer"][..]),
        (CHILD_WITH_OTHER, &["other"][..]),
        (CHILD_BARE, &[][..]),
    ] {
        graph.insert(dp(child), make_node("child@1.0.0", child, &[], peers));
    }
    for (parent, child, peers) in [
        (PARENT_WITH_OPT_PEER, CHILD_WITH_OPT_PEER, &["opt_peer"][..]),
        (PARENT_WITH_OTHER, CHILD_WITH_OTHER, &["other"][..]),
        (PARENT_BARE, CHILD_BARE, &[][..]),
    ] {
        graph.insert(dp(parent), make_node("parent@1.0.0", parent, &[("child", child)], peers));
    }

    let mut direct: DirectByImporter = BTreeMap::new();
    for (importer, parent) in [
        ("project-opt-peer", PARENT_WITH_OPT_PEER),
        ("project-other", PARENT_WITH_OTHER),
        ("project-bare", PARENT_BARE),
    ] {
        direct.insert(importer.to_string(), BTreeMap::from([("parent".to_string(), dp(parent))]));
    }

    dedupe_peer_dependents(&mut graph, &mut direct);

    assert_eq!(direct["project-opt-peer"]["parent"], dp(PARENT_WITH_OPT_PEER));
    assert_eq!(direct["project-other"]["parent"], dp(PARENT_WITH_OTHER));
    assert_eq!(direct["project-bare"]["parent"], dp(PARENT_WITH_OTHER));
    assert!(!graph.contains_key(&dp(PARENT_BARE)));
}

/// Chain longer than any thread's stack budget, diverging at every level
/// so the compatibility walk has to reach the bottom. Compatibility stays
/// answerable at a depth the native call stack cannot hold.
#[test]
fn deep_divergent_chain_does_not_overflow_the_stack() {
    const DEPTH: usize = 30_000;

    let mut graph = DependenciesGraph::default();
    for level in 0..DEPTH {
        let pkg = format!("pkg{level}@1.0.0");
        let larger = format!("pkg{level}@1.0.0(peer@1.0.0)");
        let child = format!("pkg{}@1.0.0", level + 1);
        let larger_child = format!("pkg{}@1.0.0(peer@1.0.0)", level + 1);
        let last = level + 1 == DEPTH;
        let larger_children: Vec<(&str, &str)> =
            if last { vec![] } else { vec![("next", larger_child.as_str())] };
        let children: Vec<(&str, &str)> =
            if last { vec![] } else { vec![("next", child.as_str())] };
        graph.insert(dp(&larger), make_node(&pkg, &larger, &larger_children, &["peer"]));
        graph.insert(dp(&pkg), make_node(&pkg, &pkg, &children, &[]));
    }

    let larger = dp("pkg0@1.0.0(peer@1.0.0)");
    let smaller = dp("pkg0@1.0.0");
    assert!(crate::dep_path_compatibility::is_compatible_and_has_more_deps(
        &graph, &larger, &smaller
    ));
}
