use super::rebuild_dependency_groups;
use pnpm_config::Config;
use pnpm_package_manifest::DependencyGroup;
use std::{fs, path::Path};
use tempfile::tempdir;

fn state(dir: &Path, config: Config) -> crate::State {
    let manifest_path = dir.join("package.json");
    fs::write(&manifest_path, "{}").unwrap();
    let config = Box::leak(Box::new(config));
    crate::State {
        tarball_mem_cache: std::sync::Arc::new(pnpm_tarball::MemCache::default()),
        http_client: std::sync::Arc::new(pnpm_network::ThrottledClient::default()),
        config,
        manifest: pnpm_package_manifest::PackageManifest::from_path(manifest_path).unwrap(),
        lockfile: pnpm_lockfile::LazyLockfile::disabled(),
        resolved_packages: pnpm_package_manager::ResolvedPackages::new(),
        dedicated: None,
    }
}

/// A `Config` whose `modules_dir` is a `node_modules` under `dir`. When
/// `included` is `Some`, a `.modules.yaml` recording it is written.
fn config_with_included(dir: &Path, included: Option<serde_json::Value>) -> Config {
    let modules_dir = dir.join("node_modules");
    if let Some(included) = included {
        fs::create_dir_all(&modules_dir).expect("create node_modules");
        let manifest = serde_json::json!({
            "layoutVersion": 5,
            "packageManager": "pacquet@test",
            "included": included,
            "storeDir": "/store",
            "virtualStoreDir": ".pnpm",
        });
        fs::write(modules_dir.join(".modules.yaml"), manifest.to_string())
            .expect("write .modules.yaml");
    }
    let mut config = Config::new();
    config.modules_dir = modules_dir;
    config
}

#[test]
fn reuses_prod_only_when_only_dependencies_were_installed() {
    let dir = tempdir().unwrap();
    let config = config_with_included(
        dir.path(),
        Some(serde_json::json!({
            "dependencies": true,
            "devDependencies": false,
            "optionalDependencies": false,
        })),
    );
    assert_eq!(rebuild_dependency_groups(&config).unwrap(), vec![DependencyGroup::Prod]);
}

#[test]
fn omits_optional_when_installed_with_no_optional() {
    let dir = tempdir().unwrap();
    let config = config_with_included(
        dir.path(),
        Some(serde_json::json!({
            "dependencies": true,
            "devDependencies": true,
            "optionalDependencies": false,
        })),
    );
    assert_eq!(
        rebuild_dependency_groups(&config).unwrap(),
        vec![DependencyGroup::Prod, DependencyGroup::Dev],
    );
}

#[test]
fn reuses_all_groups_when_all_were_installed() {
    let dir = tempdir().unwrap();
    let config = config_with_included(
        dir.path(),
        Some(serde_json::json!({
            "dependencies": true,
            "devDependencies": true,
            "optionalDependencies": true,
        })),
    );
    assert_eq!(
        rebuild_dependency_groups(&config).unwrap(),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn defaults_to_all_groups_without_a_modules_manifest() {
    let dir = tempdir().unwrap();
    let config = config_with_included(dir.path(), None);
    assert_eq!(
        rebuild_dependency_groups(&config).unwrap(),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn falls_back_to_all_groups_when_included_is_empty() {
    // A `.modules.yaml` that records no included groups (e.g. a legacy
    // manifest) must not narrow the rebuild to the empty set.
    let dir = tempdir().unwrap();
    let config = config_with_included(
        dir.path(),
        Some(serde_json::json!({
            "dependencies": false,
            "devDependencies": false,
            "optionalDependencies": false,
        })),
    );
    assert_eq!(
        rebuild_dependency_groups(&config).unwrap(),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn pending_without_install_state_does_not_rebuild_every_dependency() {
    let dir = tempdir().unwrap();
    let config = config_with_included(dir.path(), None);
    let state = state(dir.path(), config);
    let selection = super::resolve_selection(&[], true, &state).unwrap();
    assert_eq!(selection.names, Some(Vec::new()));
    assert!(selection.projects.is_empty());
    assert!(selection.snapshots.is_none());
}

#[test]
fn pending_selects_dependencies_and_workspace_projects_separately() {
    let dir = tempdir().unwrap();
    let config = config_with_included(
        dir.path(),
        Some(serde_json::json!({
            "dependencies": true,
            "devDependencies": true,
            "optionalDependencies": true,
        })),
    );
    let project = dir.path().join("app");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.json"), r#"{"name":"app"}"#).unwrap();
    let manifest_path = config.modules_dir.join(".modules.yaml");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["pendingBuilds"] = serde_json::json!(["dependency@1.0.0", "app"]);
    fs::write(manifest_path, manifest.to_string()).unwrap();
    let state = state(dir.path(), config);
    let selection = super::resolve_selection(&[], true, &state).unwrap();
    assert_eq!(selection.names, Some(vec!["dependency".to_owned()]));
    assert_eq!(selection.projects, vec!["app".to_owned()]);
    assert!(selection.snapshots.is_none());
}
