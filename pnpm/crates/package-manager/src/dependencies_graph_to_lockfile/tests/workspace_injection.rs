use super::{
    super::{GraphToLockfileOptions, ImporterLockfileInput},
    EMPTY_NAMED_REGISTRIES, EMPTY_REGISTRY_OPTIONS, dependencies_graph_to_lockfile,
    injected_link_fixture, make_node, previous_importers_with_link, single_importer_opts,
    write_manifest,
};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{ImporterDepVersion, PkgName};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{DependenciesGraph, UpdateReuseScope};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::json;
use std::collections::BTreeMap;
use tempfile::TempDir;

// A `pacquet update n` (UpdateReuseScope::Except containing the package
// name) targets the dependency, so its divergent `file:` resolution is
// kept rather than reverted to the prior `link:`.
#[test]
fn injected_workspace_dep_flips_to_file_when_update_targets_it() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::Except(
            std::iter::once(("n".to_string(), None)).collect(),
        ),
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    assert!(
        matches!(&entry.version, ImporterDepVersion::File(_)),
        "an update that targets n must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}
// A changed specifier (a new or edited manifest entry) targets the
// dependency too, so the divergent `file:` stands.
#[test]
fn injected_workspace_dep_flips_to_file_when_specifier_changed() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    // Previous lockfile recorded a different specifier than the manifest
    // now declares (`workspace:*`).
    let previous = previous_importers_with_link("n", "workspace:^1.0.0", "../n");

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::All,
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    assert!(
        matches!(&entry.version, ImporterDepVersion::File(_)),
        "a spec change must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}
// `pacquet update n --recursive` lowers to a `ByImporter` policy whose
// global scope is `All`, with the named package recorded per importer.
// The guard resolves the effective per-importer scope, so `n` in the
// importer that declares it is targeted and its divergent `file:` stands.
#[test]
fn injected_workspace_dep_flips_to_file_when_recursive_update_targets_it_per_importer() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");
    // Global All (recursive updates never withhold globally); the named
    // package lives in the per-importer scope for the root importer (".").
    let scopes_by_importer = BTreeMap::from([(
        ".".to_string(),
        UpdateReuseScope::Except(std::iter::once(("n".to_string(), None)).collect()),
    )]);

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: scopes_by_importer,
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    assert!(
        matches!(&entry.version, ImporterDepVersion::File(_)),
        "a recursive update naming n must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}
// The pnpm/pnpm#10433 scenario for the recursive path: `pacquet update <other>
// --recursive` records `<other>` (not `n`) in the per-importer scope, so
// `n` is untargeted and its `link:` must be preserved even though it
// re-resolved to a divergent `file:`. Without honoring the per-importer
// scope the guard would read the global `All` and (correctly, here) also
// preserve — so to prove the per-importer scope is actually consulted, the
// companion test above names `n` and asserts the opposite outcome.
#[test]
fn injected_workspace_dep_keeps_link_when_recursive_update_targets_other_pkg() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");
    let scopes_by_importer = BTreeMap::from([(
        ".".to_string(),
        UpdateReuseScope::Except(std::iter::once(("some-other-pkg".to_string(), None)).collect()),
    )]);

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: scopes_by_importer,
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    match &entry.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, "../n"),
        other => panic!("a recursive update of another package must keep n's link:, got {other:?}"),
    }
}
// Bare `pacquet update` (UpdateReuseScope::None) re-resolves the whole
// graph, so every workspace dependency is targeted and its divergent
// `file:` resolution stands — exercises the `None` arm of the guard.
#[test]
fn injected_workspace_dep_flips_to_file_on_scope_wide_update() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::None,
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    assert!(
        matches!(&entry.version, ImporterDepVersion::File(_)),
        "a scope-wide update must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}
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
        let recorded = importer
            .dependencies
            .as_ref()
            .and_then(|deps| deps.get(&PkgName::parse("dep").expect("parse alias")))
            .expect("the direct dependency must be recorded");
        assert_eq!(recorded.specifier, "^1.0.0", "importer {id}");
    }
}
