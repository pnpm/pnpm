use std::{collections::BTreeMap, fs, path::Path};

use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::Project;
use serde_json::{Value, json};

use super::build_workspace_package_manifest_map;

const WORK: &str = "https://npm.work.example.com/";
const OTHER: &str = "https://npm.other.example.com/";

fn project(root_dir: &Path, name: &str, version: Option<&str>) -> Project {
    let mut manifest = json!({ "name": name });
    if let Some(version) = version {
        manifest["version"] = json!(version);
    }
    project_with_manifest(root_dir, manifest)
}

fn project_with_manifest(root_dir: &Path, manifest: Value) -> Project {
    Project {
        root_dir: root_dir.to_path_buf(),
        manifest: PackageManifest::from_value(root_dir.join("package.json"), manifest),
        dependency_manifest: None,
    }
}

/// The `work` prefix on the registry the `@work` scope routes to.
fn config_with_work_prefix() -> Config {
    let mut config = Config::new();
    config.registries_by_scope = BTreeMap::from([("@work".to_string(), WORK.to_string())]);
    config.registries_by_prefix = BTreeMap::from([("work".to_string(), WORK.to_string())]);
    config
}

fn registry_prefix_of_project(project: Project) -> Option<String> {
    let name = project.manifest.value()["name"]
        .as_str()
        .expect("name")
        .to_string();
    let map = build_workspace_package_manifest_map(&[project], &config_with_work_prefix())
        .expect("build the workspace map");
    map[&name][0].registry_prefix.clone()
}

fn registry_prefix_of(manifest: Value) -> Option<String> {
    registry_prefix_of_project(project_with_manifest(Path::new("/workspace/pkg"), manifest))
}

#[test]
fn every_project_of_a_name_is_kept_in_discovery_order() {
    let map = build_workspace_package_manifest_map(
        &[
            project(Path::new("/workspace/first"), "pkg-b", None),
            project(Path::new("/workspace/second"), "pkg-b", Some("2.0.0")),
            project(Path::new("/workspace/third"), "pkg-b", Some("1.0.0")),
        ],
        &Config::new(),
    )
    .expect("build the workspace map");

    let versions: Vec<&str> = map["pkg-b"]
        .iter()
        .map(|project| project.version.as_str())
        .collect();
    assert_eq!(versions, ["", "2.0.0", "1.0.0"]);
}

#[test]
fn the_scope_route_picks_the_registry_prefix() {
    assert_eq!(
        registry_prefix_of(json!({ "name": "@work/a", "version": "1.0.0" })),
        Some("work".to_string()),
    );
}

#[test]
fn publish_config_registry_picks_the_registry_prefix() {
    assert_eq!(
        registry_prefix_of(json!({
            "name": "pkg-b",
            "version": "1.0.0",
            "publishConfig": { "registry": "https://NPM.Work.Example.com" },
        })),
        Some("work".to_string()),
    );
}

#[test]
fn a_scoped_publish_config_registry_outranks_the_unscoped_one() {
    assert_eq!(
        registry_prefix_of(json!({
            "name": "@other/d",
            "version": "1.0.0",
            "publishConfig": { "registry": OTHER, "@other:registry": WORK },
        })),
        Some("work".to_string()),
    );
}

#[test]
fn publish_config_registry_outranks_the_scope_route() {
    assert_eq!(
        registry_prefix_of(json!({
            "name": "@work/a",
            "version": "1.0.0",
            "publishConfig": { "registry": OTHER },
        })),
        None,
    );
}

#[test]
fn the_published_name_picks_the_scope() {
    assert_eq!(
        registry_prefix_of(json!({
            "name": "renamed",
            "version": "1.0.0",
            "publishConfig": { "name": "@work/renamed" },
        })),
        Some("work".to_string()),
    );
}

#[test]
fn a_registry_without_a_prefix_has_none() {
    assert_eq!(registry_prefix_of(json!({ "name": "pkg-c", "version": "1.0.0" })), None);
}

/// A project on disk whose root manifest publishes from `dist`, with the
/// given `publishConfig` in each manifest.
fn project_publishing_from_dist(
    root: &Path,
    root_publish_config: Value,
    dist_publish_config: Option<Value>,
) -> Project {
    let mut root_config = root_publish_config;
    root_config["directory"] = json!("dist");
    let manifest = json!({ "name": "pkg-f", "version": "1.0.0", "publishConfig": root_config });
    if let Some(dist_config) = dist_publish_config {
        fs::create_dir_all(root.join("dist")).expect("create dist");
        fs::write(
            root.join("dist/package.json"),
            json!({ "name": "pkg-f", "version": "1.0.0", "publishConfig": dist_config }).to_string(
            ),
        )
        .expect("write the dist manifest");
    }
    project_with_manifest(root, manifest)
}

#[test]
fn the_publish_config_directory_manifest_selects_the_registry() {
    let root = tempfile::tempdir().expect("project dir");

    let project = project_publishing_from_dist(
        root.path(),
        json!({ "registry": WORK }),
        Some(json!({ "registry": OTHER })),
    );
    assert_eq!(registry_prefix_of_project(project), None);

    let project = project_publishing_from_dist(
        root.path(),
        json!({ "registry": OTHER }),
        Some(json!({ "registry": WORK })),
    );
    assert_eq!(registry_prefix_of_project(project), Some("work".to_string()));
}

#[test]
fn an_unbuilt_publish_directory_leaves_the_project_manifest_to_select_the_registry() {
    let root = tempfile::tempdir().expect("project dir");

    let project = project_publishing_from_dist(root.path(), json!({ "registry": WORK }), None);

    assert_eq!(registry_prefix_of_project(project), Some("work".to_string()));
}
