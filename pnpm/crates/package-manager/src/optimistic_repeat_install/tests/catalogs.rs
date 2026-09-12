use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        settings::current_settings_with_catalogs,
    },
    check_with_catalogs, check_workspace, isolated_included, setup_fresh_install,
    setup_fresh_install_with_config, write_empty_lockfile, write_state,
};
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::backdate_existing_files;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

/// A `catalog:` dependency whose catalog entry holds a bare local path
/// is a local file dependency after dereferencing — the catalog
/// resolver only bans the `workspace:`, `link:`, and `file:` protocols,
/// so the bare-path spelling reaches the local resolver. Same bail as
/// a direct local path.
#[test]
fn returns_skipped_when_a_catalog_dependency_resolves_to_a_local_path() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"catalog:"}"#,
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("foo".to_string(), "../foo".to_string())]),
    )]);
    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
    });
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "decision was {decision:?}",
    );
}
/// A `catalog:` dependency resolving to a registry range keeps the
/// fast path: the dereferenced specifier is not a local path.
#[test]
fn returns_up_to_date_when_a_catalog_dependency_resolves_to_a_registry_range() {
    let (dir, config, manifest) = setup_fresh_install(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"catalog:"}"#,
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("foo".to_string(), "^1.0.0".to_string())]),
    )]);
    // Record the catalogs in the state so the catalog-cache comparison
    // passes and the spec-deref path is what gets exercised, not the
    // "catalogs cache outdated" bail.
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &catalogs,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_existing_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
    });
    assert_eq!(decision, Decision::UpToDate);
}
/// A `pnpm.overrides` entry spelled `catalog:` whose catalog entry
/// holds a local path bails like a direct local file override —
/// overrides are dereferenced through `parse_config_overrides` before
/// the check.
#[test]
fn returns_skipped_when_an_override_maps_through_a_catalog_to_a_local_path() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("bar".to_string(), "catalog:".to_string())]));
        },
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("bar".to_string(), "./vendor/bar".to_string())]),
    )]);
    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        node_linker: pnpm_config::NodeLinker::Isolated,
        included: isolated_included(),
        supported_architectures: None,
        project_manifests: &[(dir.path().to_path_buf(), &manifest)],
        is_workspace_install: false,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &catalogs,
    });
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("override")),
        "decision was {decision:?}",
    );
}
#[test]
fn returns_outdated_when_workspace_catalog_cache_changes() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let recorded_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^18.0.0".to_string())]),
    )]);
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &recorded_catalogs,
    );

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let current_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^19.0.0".to_string())]),
    )]);
    let decision = check_workspace(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
        &current_catalogs,
    );
    assert_eq!(decision, Decision::Skipped { reason: "catalogs cache outdated" });
}
#[test]
fn returns_outdated_when_single_project_catalog_cache_changes() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    write_empty_lockfile(workspace_root);

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let recorded_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^18.0.0".to_string())]),
    )]);
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &recorded_catalogs,
    );

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), settings, projects);

    let current_catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^19.0.0".to_string())]),
    )]);
    let decision = check_with_catalogs(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
        false,
        &current_catalogs,
    );
    assert_eq!(decision, Decision::Skipped { reason: "catalogs cache outdated" });
}
