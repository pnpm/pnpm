use super::{
    super::{
        DependenciesGraphToLockfileError,
        dependencies_graph_to_lockfile as try_dependencies_graph_to_lockfile,
    },
    make_file_node, make_node, single_importer_opts, write_manifest,
};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::PackageKey;
use pnpm_resolving_deps_resolver::DependenciesGraph;
use pnpm_resolving_resolver_base::ResolveResult;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::json;
use std::collections::BTreeMap;

// `node_pkg_name` (the guard's `update <name>` scope matcher) reads the
// structured `name_ver` when the resolver produced one and falls back to
// the fetched manifest's `name` for directory resolutions, whose
// `name_ver` is unset.
#[test]
fn node_pkg_name_prefers_name_ver_and_falls_back_to_manifest() {
    let mut node = make_file_node("n", "packages/n");
    assert_eq!(
        crate::dependencies_graph_to_lockfile::importers::node_pkg_name(&node),
        Some("n".to_string()),
    );

    let resolve_result = ResolveResult {
        name_ver: Some("renamed@1.0.0".parse().expect("parse PkgNameVer")),
        ..(*node.resolve_result).clone()
    };
    node.resolve_result = std::sync::Arc::new(resolve_result);
    assert_eq!(
        crate::dependencies_graph_to_lockfile::importers::node_pkg_name(&node),
        Some("renamed".to_string()),
    );
}
/// With several unkeyable nodes, the reported failure must be the first
/// one in the graph's own iteration order — the parallel node fan-out
/// folds its results in that order, like the serial loop it replaced.
#[test]
fn the_first_unkeyable_node_in_graph_order_is_the_reported_one() {
    let mut graph: DependenciesGraph = HashMap::default();
    for name in ["alpha", "beta"] {
        let mut node = make_node(
            name,
            "1.0.0",
            json!({ "name": name, "version": "1.0.0" }),
            BTreeMap::new(),
            BTreeMap::new(),
            HashSet::default(),
        );
        node.dep_path = DepPath::from(format!("!broken-{name}!"));
        assert!(
            node.dep_path.as_str().parse::<PackageKey>().is_err(),
            "the fixture path must not key a snapshot row",
        );
        graph.insert(node.dep_path.clone(), node);
    }
    let expected = graph
        .values()
        .map(|node| node.dep_path.as_str().to_string())
        .next()
        .expect("two nodes were inserted");

    let (_tmp, manifest) = write_manifest(json!({ "name": "root", "version": "1.0.0" }));
    let result = try_dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &graph,
        BTreeMap::new(),
        false,
        false,
        None,
        None,
    ));

    let Err(DependenciesGraphToLockfileError::UnkeyedDepPath { dep_path, .. }) = result else {
        panic!("an unkeyable dep path must fail the conversion");
    };
    assert_eq!(dep_path, expected);
}
