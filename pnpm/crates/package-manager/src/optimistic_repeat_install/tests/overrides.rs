use super::{
    super::{Decision, settings::current_settings_with_catalogs},
    backdate_validated_files, check, check_with_catalogs, isolated_included,
    setup_fresh_install_with_config, write_state,
};
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::fs::set_mtime_ms;
use pnpm_workspace_state::{ProjectEntry, WORKSPACE_STATE_FILENAME};
use std::collections::BTreeMap;

#[test]
fn returns_up_to_date_when_a_local_file_dependency_is_replaced_by_a_registry_override() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("foo".to_string(), "^2.0.0".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// A local file dependency overridden to another local file target still
/// bails through the override reason, since the override's target is
/// what gets installed.
#[test]
fn returns_skipped_when_a_local_file_dependency_is_overridden_to_another_local_file() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
        |config| {
            config.overrides =
                Some(IndexMap::from([("foo".to_string(), "file:../bar".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("override")),
        "decision was {decision:?}",
    );
}

/// A version-constrained override selector never intersects a local file
/// spec, so the dependency is not replaced and the local-file bail
/// stands.
#[test]
fn returns_skipped_when_the_override_matching_a_local_file_dependency_has_a_version_constraint() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
        |config| {
            config.overrides =
                Some(IndexMap::from([("foo@^2.0.0".to_string(), "^2.0.0".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "decision was {decision:?}",
    );
}

/// A parent-scoped override may or may not apply depending on which
/// package declares the dependency, so it does not suppress the
/// local-file bail.
#[test]
fn returns_skipped_when_a_local_file_dependency_is_matched_only_by_a_parent_scoped_override() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
        |config| {
            config.overrides =
                Some(IndexMap::from([("bar>foo".to_string(), "^2.0.0".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "decision was {decision:?}",
    );
}

/// A convergence override (`pkg@`) rewrites an edge only when its
/// declared spec is a plain semver range, so it never replaces a
/// local file dependency and does not suppress the local-file bail.
#[test]
fn returns_skipped_when_a_local_file_dependency_is_matched_only_by_a_convergence_override() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"file:../foo"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("foo@".to_string(), "2.0.0".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "decision was {decision:?}",
    );
}

/// A catalog dependency dereferencing to a local path but replaced
/// by a generic override keeps the fast path, same as a direct local file
/// dependency replaced by one.
#[test]
fn returns_up_to_date_when_a_catalog_local_dependency_is_replaced_by_an_override() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"catalog:"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("foo".to_string(), "^2.0.0".to_string())]));
        },
    );

    let catalogs: Catalogs = BTreeMap::from([(
        "default".to_string(),
        BTreeMap::from([("foo".to_string(), "../foo".to_string())]),
    )]);
    let settings = current_settings_with_catalogs(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
        &catalogs,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path()
            .to_string_lossy()
            .into_owned(),
        ProjectEntry {
            name: Some("root".into()),
            version: Some("1.0.0".into()),
            has_modules_dir: false,
        },
    );
    const WHOLE_SECOND_MS: i64 = 1_700_000_000_000;
    set_mtime_ms(&dir.path().join(Lockfile::FILE_NAME), WHOLE_SECOND_MS);
    set_mtime_ms(manifest.path(), WHOLE_SECOND_MS);
    set_mtime_ms(&config.modules_dir.join(WORKSPACE_STATE_FILENAME), WHOLE_SECOND_MS);
    write_state(dir.path(), backdate_validated_files(dir.path()), settings, projects);

    let decision = check_with_catalogs(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
        false,
        &catalogs,
    );
    assert_eq!(decision, Decision::UpToDate);
}

/// A package extension local file dependency replaced by a generic
/// override keeps the fast path: overrides rewrite extension-injected
/// dependencies the same way they rewrite declared ones.
#[test]
fn returns_up_to_date_when_a_package_extension_local_dependency_is_replaced_by_an_override() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("bar".to_string(), "^2.0.0".to_string())]));
            config.package_extensions = Some(IndexMap::from([(
                "foo@1".to_string(),
                pnpm_config::PackageExtension {
                    dependencies: Some(BTreeMap::from([(
                        "bar".to_string(),
                        "file:../bar".to_string(),
                    )])),
                    ..Default::default()
                },
            )]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert_eq!(decision, Decision::UpToDate);
}

#[test]
fn returns_up_to_date_when_a_linked_workspace_dependency_is_overridden_and_manifest_is_edited() {
    let dir = tempfile::tempdir().unwrap();
    let workspace_root = dir.path();
    std::fs::write(
        workspace_root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"pkg-a":"^2.0.0"}}"#,
    )
    .unwrap();
    let sibling_dir = workspace_root.join("pkg-a");
    std::fs::create_dir_all(&sibling_dir).unwrap();
    std::fs::write(sibling_dir.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#)
        .unwrap();
    std::fs::write(
        workspace_root.join(Lockfile::FILE_NAME),
        "lockfileVersion: '9.0'\n\nsettings:\n  autoInstallPeers: true\n  excludeLinksFromLockfile: false\n\noverrides:\n  pkg-a: workspace:*\n\nimporters:\n\n  .:\n    dependencies:\n      pkg-a:\n        specifier: workspace:*\n        version: link:pkg-a\n\n  pkg-a: {}\n",
    )
    .unwrap();

    let mut config = pnpm_config::Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.install_state_dir = workspace_root.join("node_modules/.pnpm");
    config.link_workspace_packages = pnpm_config::LinkWorkspacePackages::DirectOnly;
    config.overrides = Some(IndexMap::from([("pkg-a".to_string(), "workspace:*".to_string())]));
    std::fs::create_dir_all(&config.modules_dir).unwrap();
    let config = config.leak();

    let sibling_manifest =
        pnpm_package_manifest::PackageManifest::from_path(sibling_dir.join("package.json"))
            .unwrap();
    let settings = super::super::settings::current_settings(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let projects = BTreeMap::from([
        (
            workspace_root.to_string_lossy().into_owned(),
            ProjectEntry {
                name: Some("root".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: false,
            },
        ),
        (
            sibling_dir.to_string_lossy().into_owned(),
            ProjectEntry {
                name: Some("pkg-a".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: false,
            },
        ),
    ]);
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);

    std::fs::write(
        workspace_root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","description":"edited","dependencies":{"pkg-a":"^2.0.0"}}"#,
    )
    .unwrap();
    let root_manifest_edited =
        pnpm_package_manifest::PackageManifest::from_path(workspace_root.join("package.json"))
            .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(workspace_root).expect("parse pnpm-lock.yaml");
    let decision = super::super::check_optimistic_repeat_install(
        &super::super::OptimisticRepeatInstallCheck {
            workspace_root,
            config,
            project_manifests: &[
                (workspace_root.to_path_buf(), &root_manifest_edited),
                (sibling_dir, &sibling_manifest),
            ],
            is_workspace_install: true,
            lockfile: pnpm_lockfile::MaybeLazyLockfile::Loaded(lockfile.as_ref()),
            catalogs: &BTreeMap::default(),
            layout: crate::RepeatInstallLayout {
                node_linker: pnpm_config::NodeLinker::Isolated,
                included: isolated_included(),
                supported_architectures: None,
            },
            manifests: crate::RepeatInstallManifests::ON_DISK,
        },
    );
    assert_eq!(decision, Decision::UpToDate);
}
