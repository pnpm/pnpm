use crate::Config;
use pnpm_catalogs_types::Catalogs;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::TempDir;

fn write_manifest(dir: &Path, yaml: &str) {
    fs::create_dir_all(dir).expect("create the manifest dir");
    fs::write(dir.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

fn dedicated_workspace_config(workspace: &Path) -> Config {
    let mut config = Config::new();
    config.workspace_dir = Some(workspace.to_path_buf());
    config.shared_workspace_lockfile = false;
    config
}

fn default_catalog(entries: &[(&str, &str)]) -> Catalogs {
    Catalogs::from([(
        "default".to_string(),
        entries
            .iter()
            .map(|(name, spec)| ((*name).to_string(), (*spec).to_string()))
            .collect(),
    )])
}

#[test]
fn a_project_with_its_own_manifest_resolves_against_its_catalogs() {
    let workspace = TempDir::new().expect("create a temp dir");
    write_manifest(workspace.path(), "catalog:\n  react: ^19.0.0\n  lodash: ^4.0.0\n");
    let project_dir = workspace.path().join("apps/legacy");
    write_manifest(&project_dir, "extends: ./shared\ncatalog:\n  react: ^17.0.0\n");
    write_manifest(&project_dir.join("shared"), "catalog:\n  jquery: ^3.0.0\n");
    let mut config = dedicated_workspace_config(workspace.path());

    config.anchor_dedicated_project(&project_dir, None).expect("anchor the project");

    dbg!(&config.catalogs);
    assert_eq!(
        config.catalogs,
        Some(default_catalog(&[("jquery", "^3.0.0"), ("react", "^17.0.0")])),
    );
    assert_eq!(config.catalogs_dir(), Some(project_dir.as_path()));
}

#[test]
fn a_project_without_its_own_manifest_keeps_the_workspace_catalogs() {
    let workspace = TempDir::new().expect("create a temp dir");
    write_manifest(workspace.path(), "catalog:\n  react: ^19.0.0\n");
    let project_dir = workspace.path().join("apps/web");
    fs::create_dir_all(&project_dir).expect("create the project dir");
    let mut config = dedicated_workspace_config(workspace.path());

    config.anchor_dedicated_project(&project_dir, None).expect("anchor the project");

    assert_eq!(config.catalogs, None);
    assert_eq!(config.catalogs_dir(), Some(workspace.path()));
}

#[test]
fn the_workspace_root_keeps_the_workspace_catalogs() {
    let workspace = TempDir::new().expect("create a temp dir");
    write_manifest(workspace.path(), "catalog:\n  react: ^19.0.0\n");
    let mut config = dedicated_workspace_config(workspace.path());

    config.anchor_dedicated_project(workspace.path(), None).expect("anchor the project");

    assert_eq!(config.catalogs, None);
    assert_eq!(config.project_catalogs_dir, None);
}

#[test]
fn a_shared_lockfile_keeps_the_workspace_catalogs() {
    let workspace = TempDir::new().expect("create a temp dir");
    write_manifest(workspace.path(), "catalog:\n  react: ^19.0.0\n");
    let project_dir = workspace.path().join("apps/legacy");
    write_manifest(&project_dir, "catalog:\n  react: ^17.0.0\n");
    let mut config = dedicated_workspace_config(workspace.path());
    config.shared_workspace_lockfile = true;

    config.adopt_project_catalogs(&project_dir).expect("adopt the project catalogs");

    assert_eq!(config.catalogs, None);
    assert_eq!(config.project_catalogs_dir, None);
}

#[test]
fn an_invalid_project_manifest_is_an_error() {
    let workspace = TempDir::new().expect("create a temp dir");
    write_manifest(workspace.path(), "catalog:\n  react: ^19.0.0\n");
    let project_dir = workspace.path().join("apps/legacy");
    write_manifest(&project_dir, "extends: ./missing\n");
    let mut config = dedicated_workspace_config(workspace.path());

    let error = config.anchor_dedicated_project(&project_dir, None).unwrap_err();

    dbg!(&error);
    assert!(matches!(error, crate::ProjectCatalogsError::ReadWorkspaceManifest(_)));
}
