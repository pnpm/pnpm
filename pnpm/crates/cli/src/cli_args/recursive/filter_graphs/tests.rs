use super::overridden_project_dependencies;
use pnpm_catalogs_types::{
    Catalog,
    Catalogs,
};
use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::{
    GraphPkg,
    Project,
};
use pnpm_workspace_projects_graph::{
    ProjectGraph,
    ProjectGraphNode,
};
use serde_json::json;
use std::{
    fs,
    path::Path,
};

/// The dependencies the changed-catalog check reads for one project with
/// `manifest`, under `overrides` and a `local` catalog mapping `lib` to
/// `lib_version`.
fn checked_dependencies(
    manifest: &serde_json::Value,
    overrides: &[(&str, &str)],
    lib_version: &str,
) -> Vec<(String, String)> {
    let tmp = tempfile::tempdir().unwrap();
    let manifest_path = tmp.path().join("package.json");
    fs::write(&manifest_path, manifest.to_string()).unwrap();
    let project = Project {
        root_dir: tmp.path().to_path_buf(),
        manifest: PackageManifest::from_path(manifest_path).unwrap(),
        dependency_manifest: None,
    };
    let mut graph: ProjectGraph<GraphPkg<'_>> = ProjectGraph::new();
    graph.insert(
        project.root_dir.clone(),
        ProjectGraphNode { package: GraphPkg { project: &project }, dependencies: Vec::new() },
    );

    let mut config = Config::new();
    config.overrides = Some(
        overrides
            .iter()
            .map(|(selector, value)| ((*selector).to_string(), (*value).to_string()))
            .collect(),
    );
    let catalog = Catalog::from([("lib".to_string(), lib_version.to_string())]);
    config.catalogs = Some(Catalogs::from([
        ("local".to_string(), catalog.clone()),
        ("default".to_string(), catalog),
    ]));

    overridden_project_dependencies(&graph, &config, Path::new("/workspace"))
        .unwrap()
        .expect("overrides are configured")
        .remove(&project.root_dir)
        .unwrap()
}

fn has(dependencies: &[(String, String)], name: &str, spec: &str) -> bool {
    dependencies
        .iter()
        .any(|(dep, dep_spec)| dep == name && dep_spec == spec)
}

#[test]
fn keeps_a_rewritten_entry_whose_name_another_group_declares() {
    let manifest = json!({
        "name": "app",
        "dependencies": { "lib": "^1.0.0" },
        "optionalDependencies": { "lib": "^1.0.0" },
    });
    let dependencies =
        checked_dependencies(&manifest, &[("lib:optional", "catalog:local")], "^2.0.0");
    assert!(has(&dependencies, "lib", "catalog:local"), "{dependencies:?}");
}

#[test]
fn keeps_a_peer_entry_a_range_scoped_override_rewrites() {
    let manifest = json!({
        "name": "app",
        "dependencies": { "lib": "^1.0.0" },
        "peerDependencies": { "lib": "^2.0.0" },
    });
    let dependencies = checked_dependencies(&manifest, &[("lib@^2", "catalog:local")], "^2.0.0");
    assert!(has(&dependencies, "lib", "catalog:local"), "{dependencies:?}");
}

#[test]
fn keeps_a_declared_catalog_entry_an_override_rewrites() {
    let manifest = json!({ "name": "app", "dependencies": { "lib": "catalog:" } });
    let dependencies = checked_dependencies(&manifest, &[("lib@^2", "3.0.0")], "2.0.0");
    assert!(has(&dependencies, "lib", "catalog:"), "{dependencies:?}");
    assert!(has(&dependencies, "lib", "3.0.0"), "{dependencies:?}");
}

#[test]
fn keeps_a_declared_catalog_entry_whose_name_another_group_declares() {
    let manifest = json!({
        "name": "app",
        "dependencies": { "lib": "^1.0.0" },
        "peerDependencies": { "lib": "catalog:" },
    });
    let dependencies = checked_dependencies(&manifest, &[("lib@^2", "3.0.0")], "2.0.0");
    assert!(has(&dependencies, "lib", "catalog:"), "{dependencies:?}");
}
