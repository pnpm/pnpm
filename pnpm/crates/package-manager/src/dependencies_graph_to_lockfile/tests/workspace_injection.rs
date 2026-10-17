use super::{
    super::ImporterLockfileInput, dependencies_graph_to_lockfile, make_node, single_importer_opts,
    write_manifest,
};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::PkgName;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::DependenciesGraph;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::json;
use std::collections::BTreeMap;
use tempfile::TempDir;

/// A multi-importer workspace built through the parallel importer
/// fan-out must record every importer with the entries the serial loop
/// recorded.
#[test]
fn every_importer_of_a_workspace_is_recorded() {
    let node = make_node(
        "dep",
        "1.0.0",
        json!({ "name": "dep", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let dep_path = node.dep_path.clone();
    let mut graph: DependenciesGraph = HashMap::default();
    graph.insert(dep_path.clone(), node);

    let manifests: Vec<(TempDir, PackageManifest)> = ["root", "a", "b"]
        .into_iter()
        .map(|name| {
            write_manifest(json!({
                "name": name, "version": "1.0.0", "dependencies": { "dep": "^1.0.0" },
            }))
        })
        .collect();
    let direct: BTreeMap<String, DepPath> = BTreeMap::from([("dep".to_string(), dep_path)]);
    let mut opts =
        single_importer_opts(&manifests[0].1, &graph, direct.clone(), false, false, None, None);
    opts.importers = [".", "packages/a", "packages/b"]
        .into_iter()
        .zip(&manifests)
        .map(|(id, (_, manifest))| {
            (
                id.to_string(),
                ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct.clone() },
            )
        })
        .collect();

    let lockfile = dependencies_graph_to_lockfile(opts);
    for id in [".", "packages/a", "packages/b"] {
        let importer = lockfile.importers.get(id).expect("every importer must be recorded");
        let recorded = importer.dependencies
            .as_ref()
            .and_then(|deps| deps.get(&PkgName::parse("dep").expect("parse alias")))
            .expect("the direct dependency must be recorded");
        assert_eq!(recorded.specifier, "^1.0.0", "importer {id}");
    }
}
