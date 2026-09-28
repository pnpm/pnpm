use super::{DirectByImporter, PeerSuffixes, dedupe_peer_dependents, deduplicate_dep_paths};
use crate::dependencies_graph::{DependenciesGraph, DependenciesGraphNode};
use pnpm_deps_path::{DepPath, PeerId, create_peer_dep_graph_hash};
use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, sync::Arc};

/// Peer suffixes read back from the depPaths' text.
fn text_suffixes() -> PeerSuffixes<'static> {
    static NO_PEER_IDS: std::sync::LazyLock<HashMap<DepPath, Vec<PeerId>>> =
        std::sync::LazyLock::new(HashMap::default);
    PeerSuffixes { peer_ids: &NO_PEER_IDS, max_length: 1000 }
}

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
                non_deprecated_alternative: None,
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

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

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

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

    assert_eq!(direct["project1"]["foo"], direct["project2"]["foo"]);
    assert_ne!(direct["project1"]["foo"], direct["project3"]["foo"]);
    assert_eq!(direct["project3"]["foo"], direct["project4"]["foo"]);
}

/// Every reference to a collapsed variant is rewritten, including a
/// consumer's child edge whose own peer suffix names the collapsed
/// variant — pnpm's `deduplicateAll` rewrites `node.children`
/// unconditionally, and leaving one edge behind keeps the collapsed
/// variant alive in the lockfile. The consumer is renamed after the
/// variant its peer now resolves to (pnpm/pnpm#16356).
#[test]
fn a_consumers_child_edge_follows_the_collapse() {
    let subset = "foo@1.0.0(bar@1.0.0)";
    let larger = "foo@1.0.0(bar@1.0.0)(baz@1.0.0)";
    let baz = "baz@1.0.0(qux@1.0.0)";
    let consumer = "consumer@1.0.0(foo@1.0.0(bar@1.0.0))(bar@1.0.0)";
    let renamed_consumer = "consumer@1.0.0(bar@1.0.0)(foo@1.0.0(bar@1.0.0)(baz@1.0.0))";

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

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

    assert_eq!(direct["project-subset"]["foo"], dp(larger));
    assert_eq!(direct["project-larger"]["foo"], dp(larger));
    assert_eq!(direct["project-consumer"]["consumer"], dp(renamed_consumer));
    assert!(!graph.contains_key(&dp(consumer)));
    let consumer_node = &graph[&dp(renamed_consumer)];
    assert_eq!(consumer_node.dep_path, dp(renamed_consumer));
    assert_eq!(consumer_node.edges.children["foo"], dp(larger));
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

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

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

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

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

// <https://github.com/pnpm/pnpm/issues/6200>
#[test]
fn does_not_collapse_peer_dependents_across_different_peer_versions() {
    let host1 = "host@1.0.0(peer@1.0.0)";
    let host2 = "host@1.0.0(peer@2.0.0)";
    let dep1 = "dependent@1.0.0(host@1.0.0(peer@1.0.0))";
    let dep2 = "dependent@1.0.0(host@1.0.0(peer@2.0.0))";

    let mut graph = DependenciesGraph::default();
    for (id, dep_path) in [("peer@1.0.0", "peer@1.0.0"), ("peer@2.0.0", "peer@2.0.0")] {
        graph.insert(dp(dep_path), make_node(id, dep_path, &[], &[]));
    }
    graph.insert(dp(host1), make_node("host@1.0.0", host1, &[("peer", "peer@1.0.0")], &["peer"]));
    graph.insert(dp(host2), make_node("host@1.0.0", host2, &[("peer", "peer@2.0.0")], &["peer"]));
    graph.insert(dp(dep1), make_node("dependent@1.0.0", dep1, &[("host", host1)], &["host"]));
    graph.insert(dp(dep2), make_node("dependent@1.0.0", dep2, &[("host", host2)], &["host"]));

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert(
        "project1".to_string(),
        BTreeMap::from([
            ("dependent".to_string(), dp(dep1)),
            ("host".to_string(), dp(host1)),
            ("peer".to_string(), dp("peer@1.0.0")),
        ]),
    );
    direct.insert(
        "project2".to_string(),
        BTreeMap::from([
            ("dependent".to_string(), dp(dep2)),
            ("host".to_string(), dp(host2)),
            ("peer".to_string(), dp("peer@2.0.0")),
        ]),
    );

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

    assert_eq!(direct["project1"]["host"], dp(host1));
    assert_eq!(direct["project2"]["host"], dp(host2));
    assert_eq!(direct["project1"]["dependent"], dp(dep1));
    assert_eq!(direct["project2"]["dependent"], dp(dep2));
}

/// `core` collapses into `core(sc)`, which leaves `plugin(core)` and
/// `plugin(core(sc))` with the same edges. The dep-path tie-break keeps
/// `plugin(core)`, whose name still spells the collapsed `core`, so it is
/// renamed after the `core` it now resolves (pnpm/pnpm#16356).
#[test]
fn a_twin_kept_by_the_tie_break_is_renamed_after_the_surviving_peer() {
    let core = "core@1.0.0";
    let core_sc = "core@1.0.0(sc@1.0.0)";
    let plugin_core = "plugin@1.0.0(core@1.0.0)";
    let plugin_core_sc = "plugin@1.0.0(core@1.0.0(sc@1.0.0))";

    let mut graph = DependenciesGraph::default();
    graph.insert(dp("sc@1.0.0"), make_node("sc@1.0.0", "sc@1.0.0", &[], &[]));
    graph.insert(dp(core), make_node("core@1.0.0", core, &[], &[]));
    graph.insert(dp(core_sc), make_node("core@1.0.0", core_sc, &[("sc", "sc@1.0.0")], &["sc"]));
    graph.insert(
        dp(plugin_core),
        make_node("plugin@1.0.0", plugin_core, &[("core", core)], &["core"]),
    );
    graph.insert(
        dp(plugin_core_sc),
        make_node("plugin@1.0.0", plugin_core_sc, &[("core", core_sc)], &["core"]),
    );

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert(
        "project-a".to_string(),
        BTreeMap::from([("plugin".to_string(), dp(plugin_core))]),
    );
    direct.insert(
        "project-b".to_string(),
        BTreeMap::from([("plugin".to_string(), dp(plugin_core_sc))]),
    );

    dedupe_peer_dependents(&mut graph, &mut direct, &text_suffixes());

    assert_eq!(direct["project-a"]["plugin"], dp(plugin_core_sc));
    assert_eq!(direct["project-b"]["plugin"], dp(plugin_core_sc));
    let plugin = &graph[&dp(plugin_core_sc)];
    assert_eq!(plugin.dep_path, dp(plugin_core_sc));
    assert_eq!(plugin.edges.children["core"], dp(core_sc));
    assert!(!graph.contains_key(&dp(plugin_core)));
    assert!(!graph.contains_key(&dp(core)));
}

/// A hashed suffix is rebuilt from the peer ids it was hashed from.
#[test]
fn a_hashed_suffix_is_hashed_again_from_the_renamed_peers() {
    let subset = "foo@1.0.0(bar@1.0.0)";
    let larger = "foo@1.0.0(bar@1.0.0)(baz@1.0.0)";
    let max_length = 8;
    let hashed = |peer: &str| {
        let suffix = create_peer_dep_graph_hash(&[PeerId::DepPath(dp(peer))], max_length);
        format!("consumer@1.0.0{suffix}")
    };
    let consumer = hashed(subset);
    let renamed_consumer = hashed(larger);

    let mut graph = DependenciesGraph::default();
    for id in ["bar@1.0.0", "baz@1.0.0"] {
        graph.insert(dp(id), make_node(id, id, &[], &[]));
    }
    graph.insert(dp(subset), make_node("foo@1.0.0", subset, &[("bar", "bar@1.0.0")], &["bar"]));
    graph.insert(
        dp(larger),
        make_node(
            "foo@1.0.0",
            larger,
            &[("bar", "bar@1.0.0"), ("baz", "baz@1.0.0")],
            &["bar", "baz"],
        ),
    );
    graph.insert(
        dp(&consumer),
        make_node("consumer@1.0.0", &consumer, &[("foo", subset)], &["foo"]),
    );

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-larger".to_string(), BTreeMap::from([("foo".to_string(), dp(larger))]));
    direct.insert(
        "project-consumer".to_string(),
        BTreeMap::from([("consumer".to_string(), dp(&consumer))]),
    );
    let peer_ids = HashMap::from_iter([(dp(&consumer), vec![PeerId::DepPath(dp(subset))])]);

    dedupe_peer_dependents(
        &mut graph,
        &mut direct,
        &PeerSuffixes { peer_ids: &peer_ids, max_length },
    );

    assert_ne!(consumer, renamed_consumer);
    assert_eq!(direct["project-consumer"]["consumer"], dp(&renamed_consumer));
    assert_eq!(graph[&dp(&renamed_consumer)].edges.children["foo"], dp(larger));
    assert!(!graph.contains_key(&dp(&consumer)));
}

/// A peer that closes a cycle is spelled `name@version`. That spelling
/// names no variant, so it survives the collapse of the peerless variant
/// it happens to match.
#[test]
fn a_name_at_version_peer_is_not_renamed() {
    let bare = "foo@1.0.0";
    let larger = "foo@1.0.0(bar@1.0.0)";
    let consumer = "consumer@1.0.0(foo@1.0.0)";

    let mut graph = DependenciesGraph::default();
    graph.insert(dp("bar@1.0.0"), make_node("bar@1.0.0", "bar@1.0.0", &[], &[]));
    graph.insert(dp(bare), make_node("foo@1.0.0", bare, &[], &[]));
    graph.insert(dp(larger), make_node("foo@1.0.0", larger, &[("bar", "bar@1.0.0")], &["bar"]));
    graph.insert(dp(consumer), make_node("consumer@1.0.0", consumer, &[("foo", bare)], &["foo"]));

    let mut direct: DirectByImporter = BTreeMap::new();
    direct.insert("project-larger".to_string(), BTreeMap::from([("foo".to_string(), dp(larger))]));
    direct.insert(
        "project-consumer".to_string(),
        BTreeMap::from([("consumer".to_string(), dp(consumer))]),
    );
    let peer_ids = HashMap::from_iter([(
        dp(consumer),
        vec![PeerId::Pair { name: "foo".to_string(), version: "1.0.0".to_string() }],
    )]);

    dedupe_peer_dependents(
        &mut graph,
        &mut direct,
        &PeerSuffixes { peer_ids: &peer_ids, max_length: 1000 },
    );

    assert_eq!(direct["project-consumer"]["consumer"], dp(consumer));
    assert_eq!(graph[&dp(consumer)].edges.children["foo"], dp(larger));
}
