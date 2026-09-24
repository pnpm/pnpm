use super::{
    super::{
        Decision, OptimisticRepeatInstallCheck, check_optimistic_repeat_install,
        settings::current_settings,
    },
    backdate_validated_files, isolated_included, setup_fresh_install_with_config, write_state,
};
use indexmap::IndexMap;
use pnpm_config::{Config, ProjectConfig};
use pnpm_lockfile::MaybeLazyLockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};

fn sibling_decision(sibling_modules_dir: &str, configure: impl FnOnce(&mut Config)) -> Decision {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        "",
        |config| {
            config.modules_dir = config.modules_dir.with_file_name("vendor");
            config.virtual_store_dir = config.modules_dir.join(".pnpm");
            configure(config);
        },
    );
    let sibling_dir = dir.path().join("pkg-a");
    fs::create_dir_all(sibling_dir.join(sibling_modules_dir)).unwrap();
    let sibling_manifest_path = sibling_dir.join("package.json");
    fs::write(
        &sibling_manifest_path,
        r#"{"name":"pkg-a","version":"1.0.0","dependencies":{"foo":"1.0.0"}}"#,
    )
    .unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_manifest_path).unwrap();

    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    projects.insert(
        sibling_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("pkg-a".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: &[
            (dir.path().to_path_buf(), &root_manifest),
            (sibling_dir, &sibling_manifest),
        ],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifest_freshness: crate::ManifestFreshness::Mtime,
    })
}

#[test]
fn a_sibling_with_its_custom_modules_dir_is_installed() {
    assert_eq!(sibling_decision("vendor", |_| {}), Decision::UpToDate);
}

#[test]
fn a_sibling_with_only_node_modules_is_not_installed_under_a_custom_modules_dir() {
    assert!(matches!(
        sibling_decision("node_modules", |_| {}),
        Decision::Skipped { reason } if reason.contains("no node_modules directory"),
    ));
}

#[test]
fn a_sibling_with_the_modules_dir_its_package_configs_entry_names_is_installed() {
    let decision = sibling_decision("deps", |config| {
        config.shared_workspace_lockfile = false;
        config.package_configs = Some(IndexMap::from([(
            "pkg-a".to_string(),
            ProjectConfig { modules_dir: Some("deps".to_string()), ..ProjectConfig::default() },
        )]));
    });
    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn a_root_with_a_multi_component_modules_dir_is_installed() {
    let (dir, config, root_manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"1.0.0"}"#,
        |config| {
            config.modules_dir = config.modules_dir.with_file_name("custom").join("vendor");
            config.virtual_store_dir = config.modules_dir.join(".pnpm");
        },
    );
    fs::create_dir_all(&config.modules_dir).unwrap();
    let settings =
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    let decision = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: dir.path(),
        config,
        project_manifests: &[(dir.path().to_path_buf(), &root_manifest)],
        is_workspace_install: true,
        lockfile: MaybeLazyLockfile::Loaded(None),
        catalogs: &BTreeMap::default(),
        layout: crate::RepeatInstallLayout {
            node_linker: pnpm_config::NodeLinker::Isolated,
            included: isolated_included(),
            supported_architectures: None,
        },
        manifest_freshness: crate::ManifestFreshness::Mtime,
    });
    assert_eq!(decision, Decision::UpToDate);
}
