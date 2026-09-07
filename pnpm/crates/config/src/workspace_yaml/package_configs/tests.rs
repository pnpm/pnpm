use super::{PackageConfigsSetting, ProjectConfig};
use crate::{Config, WorkspaceSettings};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

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

fn project_config_of(project_dir: &Path, name: &str, project_config: ProjectConfig) -> Config {
    fs::create_dir_all(project_dir).unwrap();
    fs::write(project_dir.join("package.json"), format!(r#"{{"name":"{name}"}}"#)).unwrap();
    let mut config = Config {
        package_configs: Some(IndexMap::from([("a".to_string(), project_config)])),
        ..Config::default()
    };
    config.anchor_dedicated_project(project_dir);
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
    let config = project_config_of(&workspace.path().join("apps/a"), "a", project_config.clone());
    assert_eq!(config.overrides.as_ref(), Some(&overrides));
    assert!(config.save_exact);
    assert_eq!(config.save_prefix.as_deref(), Some("~"));

    let sibling = project_config_of(&workspace.path().join("apps/b"), "b", project_config);
    assert_eq!(sibling.overrides, None);
    assert!(!sibling.save_exact);
    assert_eq!(sibling.save_prefix, None);
}

#[test]
fn hoist_false_clears_the_hoist_pattern() {
    let workspace = tempfile::tempdir().unwrap();
    let config = project_config_of(
        &workspace.path().join("a"),
        "a",
        ProjectConfig { hoist: Some(false), ..ProjectConfig::default() },
    );
    assert!(!config.hoist);
    assert_eq!(config.hoist_pattern, None);
}

#[test]
fn modules_dir_resolves_against_the_project_and_carries_the_virtual_store() {
    let workspace = tempfile::tempdir().unwrap();
    let project_dir = workspace.path().join("a");
    let config = project_config_of(
        &project_dir,
        "a",
        ProjectConfig { modules_dir: Some("modules".to_string()), ..ProjectConfig::default() },
    );
    assert_eq!(config.modules_dir, project_dir.join("modules"));
    assert_eq!(config.virtual_store_dir, project_dir.join("modules").join(".pnpm"));
}

#[test]
fn a_project_without_a_manifest_keeps_the_workspace_settings() {
    let workspace = tempfile::tempdir().unwrap();
    let project_dir = workspace.path().join("a");
    fs::create_dir_all(&project_dir).unwrap();
    let mut config = Config {
        package_configs: Some(IndexMap::from([(
            "a".to_string(),
            ProjectConfig { save_exact: Some(true), ..ProjectConfig::default() },
        )])),
        ..Config::default()
    };
    config.anchor_dedicated_project(&project_dir);
    assert!(!config.save_exact);
}
