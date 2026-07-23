use super::{
    is_workspace_local_path_specifier, npm_alias_target, parse_update_param,
    persist_selected_manifests, prepare_selected_manifests, selected_project_indices,
};
use pacquet_config::{CatalogMode, Config};
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::SilentReporter;
use pacquet_workspace::Project;
use serde_json::json;
use std::collections::HashSet;
use tempfile::tempdir;

#[test]
fn parses_bare_name_without_version() {
    let parsed = parse_update_param("foo");
    assert_eq!(parsed.pattern, "foo");
    assert_eq!(parsed.version, None);
}

#[test]
fn parses_name_with_version() {
    let parsed = parse_update_param("foo@2");
    assert_eq!(parsed.pattern, "foo");
    assert_eq!(parsed.version.as_deref(), Some("2"));
}

#[test]
fn leading_scope_at_is_not_a_version_separator() {
    let parsed = parse_update_param("@scope/foo");
    assert_eq!(parsed.pattern, "@scope/foo");
    assert_eq!(parsed.version, None);
}

#[test]
fn scoped_name_with_version_splits_on_last_at() {
    let parsed = parse_update_param("@scope/foo@^1.2.3");
    assert_eq!(parsed.pattern, "@scope/foo");
    assert_eq!(parsed.version.as_deref(), Some("^1.2.3"));
}

#[test]
fn wildcard_pattern_without_version() {
    let parsed = parse_update_param("@pnpm.e2e/peer-*");
    assert_eq!(parsed.pattern, "@pnpm.e2e/peer-*");
    assert_eq!(parsed.version, None);
}

#[test]
fn negated_scoped_pattern_is_not_split_on_scope_at() {
    let parsed = parse_update_param("!@pnpm.e2e/peer-*");
    assert_eq!(parsed.pattern, "!@pnpm.e2e/peer-*");
    assert_eq!(parsed.version, None);
}

#[test]
fn negated_unscoped_pattern_without_version() {
    let parsed = parse_update_param("!foo");
    assert_eq!(parsed.pattern, "!foo");
    assert_eq!(parsed.version, None);
}

#[test]
fn npm_alias_specifiers_yield_their_real_package_name() {
    for (spec, alias, target) in [
        ("npm:bar@^4.0.0", "foo", Some("bar")),
        ("npm:bar", "foo", Some("bar")),
        ("npm:@types/table@6.3.2", "@types/zkochan__table", Some("@types/table")),
        ("npm:@types/table", "@types/zkochan__table", Some("@types/table")),
    ] {
        assert_eq!(npm_alias_target(spec, alias), target, "target of {alias}@{spec}");
    }
}

#[test]
fn non_alias_specifiers_have_no_npm_alias_target() {
    for (spec, alias) in [
        ("^1.0.0", "foo"),
        ("catalog:", "foo"),
        ("workspace:*", "foo"),
        ("npm:^1.0.0", "foo"),
        ("npm:foo@^1.0.0", "foo"),
    ] {
        assert_eq!(npm_alias_target(spec, alias), None, "target of {alias}@{spec}");
    }
}

#[test]
fn workspace_local_path_specifiers_are_detected() {
    for spec in [
        "workspace:.",
        "workspace:./packages/foo",
        "workspace:../packages/foo/dist",
        "workspace:/abs/path",
        "workspace:~/home/path",
        r"workspace:C:\packages\foo",
    ] {
        assert!(is_workspace_local_path_specifier(spec), "expected {spec} to be a local path");
    }
}

#[test]
fn workspace_range_specifiers_are_not_local_paths() {
    for spec in [
        "workspace:*",
        "workspace:^",
        "workspace:~",
        "workspace:^1.0.0",
        "workspace:~1.2.3",
        "workspace:1.0.0",
        "workspace:alias@*",
        "^1.0.0",
        "link:../foo",
    ] {
        assert!(!is_workspace_local_path_specifier(spec), "expected {spec} not to be a local path");
    }
}

#[tokio::test]
async fn selected_update_prepares_and_persists_only_selected_projects() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects = ["a", "b", "c"]
        .into_iter()
        .map(|name| project_with_foo(dir.path(), name))
        .collect::<Vec<_>>();
    let ordered_dirs = [projects[1].root_dir.clone(), projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let config = Config::new();
    let http_client = ThrottledClient::default();

    let prepared = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &indices,
        dir.path(),
        &http_client,
        &config,
        None,
        &["foo@2.0.0".to_string()],
        false,
        false,
        true,
        &[DependencyGroup::Prod],
        0,
        false,
        None,
    )
    .await
    .expect("prepare selected manifests");
    persist_selected_manifests::<SilentReporter>(&mut projects, &prepared.persist_indices)
        .expect("persist selected manifests");

    assert_eq!(dependency_specifier(&projects[0].manifest), "2.0.0");
    assert_eq!(dependency_specifier(&projects[1].manifest), "2.0.0");
    assert_eq!(dependency_specifier(&projects[2].manifest), "^1.0.0");
    assert_eq!(saved_dependency_specifier(&projects[0].manifest), "2.0.0");
    assert_eq!(saved_dependency_specifier(&projects[1].manifest), "2.0.0");
    assert_eq!(saved_dependency_specifier(&projects[2].manifest), "^1.0.0");
    assert_eq!(prepared.seed_policies.len(), 2);
}

#[tokio::test]
async fn selected_update_no_save_mutates_in_memory_without_persisting() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects =
        ["a", "b"].into_iter().map(|name| project_with_foo(dir.path(), name)).collect::<Vec<_>>();
    let ordered_dirs = [projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let mut config = Config::new();
    config.catalog_mode = CatalogMode::Prefer;
    let http_client = ThrottledClient::default();

    let prepared = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &indices,
        dir.path(),
        &http_client,
        &config,
        None,
        &["foo@2.0.0".to_string()],
        false,
        false,
        false,
        &[DependencyGroup::Prod],
        0,
        false,
        None,
    )
    .await
    .expect("prepare selected manifests");

    assert_eq!(dependency_specifier(&projects[0].manifest), "catalog:");
    assert_eq!(dependency_specifier(&projects[1].manifest), "^1.0.0");
    assert_eq!(saved_dependency_specifier(&projects[0].manifest), "^1.0.0");
    assert!(prepared.persist_indices.is_empty());
    assert_eq!(
        prepared
            .catalogs_override
            .as_ref()
            .and_then(|catalogs| catalogs.get("default"))
            .and_then(|catalog| catalog.get("foo"))
            .map(String::as_str),
        Some("2.0.0"),
    );
}

#[tokio::test]
async fn selected_update_depth_zero_skips_projects_without_a_matching_dependency() {
    let dir = tempdir().expect("create tempdir");
    let mut projects = [project_without_foo(dir.path(), "a"), project_with_foo(dir.path(), "b")];
    let selected_indices = [0, 1];
    let config = Config::new();
    let http_client = ThrottledClient::default();

    let prepared = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &selected_indices,
        dir.path(),
        &http_client,
        &config,
        None,
        &["foo@2.0.0".to_string()],
        false,
        false,
        true,
        &[DependencyGroup::Prod],
        0,
        false,
        None,
    )
    .await
    .expect("prepare selected manifests");

    assert_eq!(dependency_specifier(&projects[1].manifest), "2.0.0");
    assert_eq!(prepared.persist_indices, vec![1]);
}

#[tokio::test]
async fn selected_update_latest_depth_zero_is_noop_when_no_project_matches() {
    let dir = tempdir().expect("create tempdir");
    let mut projects = [project_without_foo(dir.path(), "a"), project_without_foo(dir.path(), "b")];
    let selected_indices = [0, 1];
    let config = Config::new();
    let http_client = ThrottledClient::default();

    let prepared = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &selected_indices,
        dir.path(),
        &http_client,
        &config,
        None,
        &["foo".to_string()],
        true,
        false,
        true,
        &[DependencyGroup::Prod],
        0,
        false,
        None,
    )
    .await
    .expect("unmatched latest update is a no-op");

    assert!(!prepared.any_work);
    assert!(prepared.persist_indices.is_empty());
}

fn project_with_foo(root: &std::path::Path, name: &str) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    std::fs::write(
        &package_json,
        json!({ "name": name, "dependencies": { "foo": "^1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
    }
}

fn project_without_foo(root: &std::path::Path, name: &str) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    std::fs::write(&package_json, json!({ "name": name }).to_string()).expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
    }
}

fn dependency_specifier(manifest: &PackageManifest) -> &str {
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(name, _)| *name == "foo")
        .map(|(_, specifier)| specifier)
        .expect("foo dependency")
}

fn saved_dependency_specifier(manifest: &PackageManifest) -> String {
    let saved =
        PackageManifest::from_path(manifest.path().to_path_buf()).expect("reread package.json");
    dependency_specifier(&saved).to_string()
}
