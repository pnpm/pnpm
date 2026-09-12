use super::{
    super::{GraphToLockfileOptions, ImporterLockfileInput},
    EMPTY_NAMED_REGISTRIES, EMPTY_REGISTRY_OPTIONS, dependencies_graph_to_lockfile, make_node,
    write_manifest,
};
use pnpm_deps_path::DepPath;
use pnpm_resolving_deps_resolver::{DependenciesGraph, UpdateReuseScope};
use rustc_hash::FxHashSet as HashSet;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn aliased_catalog_dependency_records_catalog_snapshot() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "js-yaml": "catalog:" },
    }));

    let zkochan_js_yaml = make_node(
        "@zkochan/js-yaml",
        "0.0.11",
        json!({ "name": "@zkochan/js-yaml", "version": "0.0.11" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(zkochan_js_yaml.dep_path.clone(), zkochan_js_yaml);

    let mut direct = BTreeMap::new();
    direct.insert("js-yaml".to_string(), DepPath::from("@zkochan/js-yaml@0.0.11".to_string()));

    let mut catalogs: pnpm_catalogs_types::Catalogs = BTreeMap::new();
    catalogs
        .entry("default".to_string())
        .or_default()
        .insert("js-yaml".to_string(), "npm:@zkochan/js-yaml@0.0.11".to_string());

    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        ImporterLockfileInput { manifest: &manifest, direct_dependencies_by_alias: direct },
    );
    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &catalogs,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        time: BTreeMap::new(),
    });

    let snapshots = lockfile.catalogs.as_ref().expect("catalogs snapshot present");
    let entry = snapshots
        .get("default")
        .and_then(|catalog| catalog.get("js-yaml"))
        .expect("aliased catalog entry recorded");
    assert_eq!(entry.specifier, "npm:@zkochan/js-yaml@0.0.11");
    assert_eq!(entry.version, "0.0.11");
}
