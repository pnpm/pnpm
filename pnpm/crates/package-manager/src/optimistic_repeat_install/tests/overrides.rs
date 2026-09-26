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
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
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
