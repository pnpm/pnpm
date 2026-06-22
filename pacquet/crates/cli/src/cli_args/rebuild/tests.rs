use super::rebuild_dependency_groups;
use pacquet_config::Config;
use pacquet_package_manifest::DependencyGroup;
use std::{fs, path::Path};
use tempfile::tempdir;

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
