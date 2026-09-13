use super::{PackageConfigsSetting, ProjectConfig};
use crate::{Config, WorkspaceSettings};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::path::Path;

fn parse(yaml: &str) -> PackageConfigsSetting {
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    settings.package_configs.expect("packageConfigs is set")
}

fn parse_error(yaml: &str) -> String {
    let error = serde_saphyr::from_str::<WorkspaceSettings>(yaml).unwrap_err();
    let message = error.to_string();
    println!("{message}");
    message
}

#[test]
fn reads_the_map_form() {
    let record = parse(
        r"
packageConfigs:
  a:
    overrides:
      ms: 2.0.0
    saveExact: true
  b:
    hoist: false
    modulesDir: ../shared_modules
    savePrefix: '~'
",
    )
    .into_record();
    dbg!(&record);
    assert_eq!(
        record.get("a"),
        Some(&ProjectConfig {
            overrides: Some(IndexMap::from([("ms".to_string(), "2.0.0".to_string())])),
            save_exact: Some(true),
            ..ProjectConfig::default()
        }),
    );
    assert_eq!(
        record.get("b"),
        Some(&ProjectConfig {
            hoist: Some(false),
            modules_dir: Some("../shared_modules".to_string()),
            save_prefix: Some("~".to_string()),
            ..ProjectConfig::default()
        }),
    );
}

#[test]
fn reads_the_list_form_and_expands_every_match() {
    let record = parse(
        r"
packageConfigs:
  - match:
      - a
      - b
    saveExact: true
  - match:
      - b
    saveExact: false
",
    )
    .into_record();
    dbg!(&record);
    assert_eq!(record.get("a").and_then(|config| config.save_exact), Some(true));
    // A later entry wins over an earlier one naming the same project.
    assert_eq!(record.get("b").and_then(|config| config.save_exact), Some(false));
}

#[test]
fn rejects_a_misspelled_setting() {
    let message = parse_error(
        r"
packageConfigs:
  a:
    saveExactly: true
",
    );
    assert!(message.contains("saveExactly"), "{message}");
}

#[test]
fn rejects_a_setting_of_the_wrong_type() {
    let message = parse_error(
        r"
packageConfigs:
  a:
    overrides: nope
",
    );
    assert!(message.contains("overrides"), "{message}");
}

#[test]
fn rejects_a_scalar_setting_value() {
    let message = parse_error("packageConfigs: nope\n");
    assert!(message.contains("packageConfigs"), "{message}");
}

#[test]
fn rejects_a_list_entry_without_a_match() {
    let message = parse_error(
        r"
packageConfigs:
  - saveExact: true
",
    );
    assert!(message.contains("match"), "{message}");
}

fn config_with(project_dir: &Path, project_config: ProjectConfig) -> Config {
    let mut config = Config {
        package_configs: Some(IndexMap::from([("a".to_string(), project_config)])),
        ..Config::default()
    };
    config.anchor_dedicated_project(project_dir, Some("a"));
    config
}

#[test]
fn overlays_the_named_project_only() {
    let workspace = tempfile::tempdir().unwrap();
    let overrides = IndexMap::from([("ms".to_string(), "2.0.0".to_string())]);
    let project_config = ProjectConfig {
        overrides: Some(overrides.clone()),
        save_exact: Some(true),
        save_prefix: Some("~".to_string()),
        ..ProjectConfig::default()
    };
    let config = config_with(&workspace.path().join("apps/a"), project_config.clone());
    assert_eq!(config.overrides.as_ref(), Some(&overrides));
    assert!(config.save_exact);
    assert_eq!(config.save_prefix.as_deref(), Some("~"));

    let mut sibling = Config {
        package_configs: Some(IndexMap::from([("a".to_string(), project_config)])),
        ..Config::default()
    };
    sibling.anchor_dedicated_project(&workspace.path().join("apps/b"), Some("b"));
    assert_eq!(sibling.overrides, None);
    assert!(!sibling.save_exact);
    assert_eq!(sibling.save_prefix, None);
}

#[test]
fn a_nameless_project_keeps_the_workspace_settings() {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = Config {
        package_configs: Some(IndexMap::from([(
            "a".to_string(),
            ProjectConfig { save_exact: Some(true), ..ProjectConfig::default() },
        )])),
        ..Config::default()
    };
    config.anchor_dedicated_project(&workspace.path().join("a"), None);
    assert!(!config.save_exact);
}

#[test]
fn hoist_false_clears_the_hoist_pattern() {
    let workspace = tempfile::tempdir().unwrap();
    let config = config_with(
        &workspace.path().join("a"),
        ProjectConfig { hoist: Some(false), ..ProjectConfig::default() },
    );
    assert!(!config.hoist);
    assert_eq!(config.hoist_pattern, None);
}

/// pnpm 11 does the same: a project entry's `hoist: true` reaches the
/// install as a bare boolean, and the pattern the workspace's
/// `hoist: false` cleared is gone by then. Re-enabling hoisting for one
/// project of a workspace that turned it off would be a pnpm 12-only
/// behavior, so it waits for a decision rather than arriving as a side
/// effect of this parity fix.
#[test]
fn hoist_true_does_not_restore_a_cleared_hoist_pattern() {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = Config {
        hoist: false,
        hoist_pattern: None,
        package_configs: Some(IndexMap::from([(
            "a".to_string(),
            ProjectConfig { hoist: Some(true), ..ProjectConfig::default() },
        )])),
        ..Config::default()
    };
    config.anchor_dedicated_project(&workspace.path().join("a"), Some("a"));
    assert!(config.hoist);
    assert_eq!(config.hoist_pattern, None);
}

#[test]
fn modules_dir_resolves_against_the_project_and_carries_the_virtual_store() {
    let workspace = tempfile::tempdir().unwrap();
    let project_dir = workspace.path().join("a");
    let config = config_with(
        &project_dir,
        ProjectConfig { modules_dir: Some("modules".to_string()), ..ProjectConfig::default() },
    );
    assert_eq!(config.modules_dir, project_dir.join("modules"));
    assert_eq!(config.virtual_store_dir, project_dir.join("modules").join(".pnpm"));
}
