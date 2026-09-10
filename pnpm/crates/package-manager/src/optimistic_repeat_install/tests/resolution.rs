use super::{
    super::{
        Decision,
        manifest_agreement::{LinkedPackagesContext, linked_packages_are_up_to_date},
        settings::current_settings,
    },
    check, isolated_included, linked_sibling_decision, setup_fresh_install_with_config,
    write_state,
};
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_testing_utils::fs::backdate_existing_files;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

/// A `pnpm.overrides` entry mapping to a local file spec must bail the
/// same way a direct local file dependency does: the override redirects
/// every matching dependency in the graph to that directory, and
/// nothing the fast path stats covers its contents.
#[test]
fn returns_skipped_when_an_override_maps_to_a_local_file_dependency() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides =
                Some(IndexMap::from([("bar".to_string(), "file:../bar".to_string())]));
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
/// Registry and `link:` overrides keep the fast path: neither redirects
/// a dependency to contents only a refetch would pick up.
#[test]
fn returns_up_to_date_when_overrides_are_not_local_paths() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([
                ("bar".to_string(), "^2.0.0".to_string()),
                ("baz".to_string(), "link:../baz".to_string()),
            ]));
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
/// An unparsable `pnpm.overrides` (here a `catalog:` reference with no
/// matching catalog entry) bails to the full install with the
/// parse-error reason, not the local-file reason: the cause is a
/// misconfiguration, and attributing it to a local file dependency
/// would mislead troubleshooting.
#[test]
fn returns_skipped_with_parse_error_reason_when_overrides_cannot_be_parsed() {
    let (dir, config, manifest) = setup_fresh_install_with_config(
        pnpm_config::NodeLinker::Isolated,
        "root",
        "1.0.0",
        r#""dependencies":{"foo":"^1.0.0"}"#,
        |config| {
            config.overrides = Some(IndexMap::from([("bar".to_string(), "catalog:".to_string())]));
        },
    );

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("cannot be parsed")),
        "decision was {decision:?}",
    );
}
/// Drift in `overrides` invalidates the cached state.
#[test]
fn returns_skipped_when_overrides_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("foo".to_string(), "2.0.0".to_string());
    config.overrides = Some(overrides);
    let config = config.leak();

    // Cached state has `foo: "1.0.0"` for the same key.
    let mut stale_overrides_config = Config::new();
    stale_overrides_config.modules_dir = config.modules_dir.clone();
    let mut overrides = indexmap::IndexMap::new();
    overrides.insert("foo".to_string(), "1.0.0".to_string());
    stale_overrides_config.overrides = Some(overrides);
    let stale_settings = current_settings(
        &stale_overrides_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}
/// Drift in `dedupePeers` invalidates the cached state — the condition
/// the optimistic-repeat-install gate checks here.
#[test]
fn returns_skipped_when_dedupe_peers_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.dedupe_peers = true;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.dedupe_peers = false;
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}
/// Drift in `peersSuffixMaxLength` invalidates the cached state.
#[test]
fn returns_skipped_when_peers_suffix_max_length_drift() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let manifest_path = workspace_root.join("package.json");
    fs::write(&manifest_path, r#"{"name":"root","version":"1.0.0"}"#).unwrap();
    let manifest = PackageManifest::from_path(manifest_path).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    fs::create_dir_all(&config.modules_dir).unwrap();
    config.peers_suffix_max_length = 100;
    let config = config.leak();

    let mut stale_config = Config::new();
    stale_config.modules_dir = config.modules_dir.clone();
    stale_config.peers_suffix_max_length = 1000;
    let stale_settings = current_settings(
        &stale_config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_root.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state(workspace_root, backdate_existing_files(workspace_root), stale_settings, projects);

    let decision = check(
        workspace_root,
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(workspace_root.to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}
/// See `WorkspaceStateSettings::minimum_release_age_strict` for the
/// resolution rule being mirrored.
#[test]
fn records_minimum_release_age_strict_like_pnpm_resolves_it() {
    let mut config = Config::new();
    config.explicit_settings.insert("minimumReleaseAge".to_string(), serde_json::Value::from(1440));
    let settings =
        current_settings(&config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    assert_eq!(settings.minimum_release_age_strict, Some(true));

    config.minimum_release_age_strict = Some(false);
    let settings =
        current_settings(&config, pnpm_config::NodeLinker::Isolated, isolated_included(), None);
    assert_eq!(settings.minimum_release_age_strict, Some(false), "an explicit value wins");
}
#[test]
fn returns_up_to_date_when_linked_sibling_still_satisfies_range() {
    assert_eq!(linked_sibling_decision("1.5.0"), Decision::UpToDate);
}
#[test]
fn returns_skipped_when_linked_sibling_no_longer_satisfies_range() {
    let decision = linked_sibling_decision("2.0.0");
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("linked")),
        "expected Skipped(linked package out of date), got {decision:?}",
    );
}
#[test]
fn injected_self_reference_resolved_as_link_is_up_to_date() {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let sibling_dir = workspace_root.join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    fs::write(
        workspace_root.join("package.json"),
        r#"{"name":"root","version":"1.0.0","dependencies":{"pkg-a":"file:pkg-a"}}"#,
    )
    .unwrap();
    fs::write(sibling_dir.join("package.json"), r#"{"name":"pkg-a","version":"1.0.0"}"#).unwrap();
    let root_manifest = PackageManifest::from_path(workspace_root.join("package.json")).unwrap();
    let sibling_manifest = PackageManifest::from_path(sibling_dir.join("package.json")).unwrap();
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      pkg-a:
        specifier: file:pkg-a
        version: link:pkg-a

  pkg-a: {}
",
    )
    .unwrap();
    let config = Config::new();
    let project_manifests =
        [(workspace_root.to_path_buf(), &root_manifest), (sibling_dir, &sibling_manifest)];
    let context = LinkedPackagesContext::new(&config, &project_manifests);

    assert!(linked_packages_are_up_to_date(
        &context,
        workspace_root,
        &root_manifest,
        lockfile.root_project().unwrap(),
    ));
}
