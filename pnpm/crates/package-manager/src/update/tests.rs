use super::{
    KeptRangeVerdict, UpdateError, apply_bumped_manifest_specs, expand_update_selectors,
    insert_update_target, is_workspace_local_path_specifier, judge_against_kept_range,
    parse_update_param, persist_selected_manifests, prepare_selected_manifests,
    reject_versions_of_indirect_update_specs, selected_project_indices, update_target_name,
};
use pnpm_config::{CatalogMode, Config};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::SilentReporter;
use pnpm_workspace::Project;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
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
fn an_npm_alias_selector_targets_the_aliased_package_name() {
    let selectors = vec![parse_update_param("alias@npm:@scope/real@^1.0.0")];
    assert_eq!(update_target_name(&selectors, "alias"), "@scope/real");
}

#[test]
fn a_jsr_alias_selector_targets_the_npm_package_name_it_installs() {
    let selectors = vec![parse_update_param("bar-from-jsr@jsr:@pnpm-e2e/bar@^1.0.0")];
    assert_eq!(update_target_name(&selectors, "bar-from-jsr"), "@jsr/pnpm-e2e__bar");
}

#[test]
fn a_versioned_selector_without_an_alias_targets_the_name_it_names() {
    let selectors = vec![parse_update_param("foo@^1.0.0")];
    assert_eq!(update_target_name(&selectors, "foo"), "foo");
}

#[test]
fn an_npm_selector_carrying_only_a_range_keeps_the_alias() {
    let selectors = vec![parse_update_param("foo@npm:^1.0.0")];
    assert_eq!(update_target_name(&selectors, "foo"), "foo");
}

#[test]
fn a_bare_selector_targets_the_name_it_names() {
    let selectors = vec![parse_update_param("foo")];
    assert_eq!(update_target_name(&selectors, "foo"), "foo");
}

#[test]
fn a_wildcard_selector_does_not_shadow_an_alias_selector_that_also_matches() {
    let selectors =
        vec![parse_update_param("*"), parse_update_param("alias@npm:@scope/real@^1.0.0")];
    assert_eq!(update_target_name(&selectors, "alias"), "@scope/real");
    assert_eq!(update_target_name(&selectors, "other"), "other");
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

// The group travels with the range from the lockfile entry that was
// rewritten, so an alias declared in several groups cannot have the
// manifest move one group while the lockfile moved another.
#[test]
fn a_bumped_range_lands_in_the_group_it_was_read_from() {
    let dir = tempdir().expect("create tempdir");
    let package_json = dir.path().join("package.json");
    std::fs::write(
        &package_json,
        json!({
            "name": "a",
            "dependencies": { "foo": "1.0.0" },
            "optionalDependencies": { "foo": "^1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let mut manifest = PackageManifest::from_path(package_json).expect("read package.json");

    let bumped =
        BTreeMap::from([("foo".to_string(), (DependencyGroup::Optional, "^1.2.0".to_string()))]);
    assert!(apply_bumped_manifest_specs::<SilentReporter>(&mut manifest, &bumped, false));

    assert_eq!(dependency_specifier_in(&manifest, DependencyGroup::Prod, "foo"), Some("1.0.0"));
    assert_eq!(
        dependency_specifier_in(&manifest, DependencyGroup::Optional, "foo"),
        Some("^1.2.0"),
    );
}

/// An alias the manifest no longer declares under the group the lockfile
/// read it from is left alone rather than added back.
#[test]
fn a_bump_for_an_undeclared_group_writes_nothing() {
    let dir = tempdir().expect("create tempdir");
    let package_json = dir.path().join("package.json");
    std::fs::write(
        &package_json,
        json!({ "name": "a", "dependencies": { "foo": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    let mut manifest = PackageManifest::from_path(package_json).expect("read package.json");

    let bumped =
        BTreeMap::from([("foo".to_string(), (DependencyGroup::Dev, "^1.2.0".to_string()))]);
    assert!(!apply_bumped_manifest_specs::<SilentReporter>(&mut manifest, &bumped, false));

    assert_eq!(dependency_specifier_in(&manifest, DependencyGroup::Prod, "foo"), Some("1.0.0"));
    assert_eq!(dependency_specifier_in(&manifest, DependencyGroup::Dev, "foo"), None);
}

fn dependency_specifier_in<'a>(
    manifest: &'a PackageManifest,
    group: DependencyGroup,
    alias: &str,
) -> Option<&'a str> {
    manifest.dependencies([group]).find(|(name, _)| *name == alias).map(|(_, spec)| spec)
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
    let http_client = std::sync::Arc::new(ThrottledClient::default());

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
        None,
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
    let http_client = std::sync::Arc::new(ThrottledClient::default());

    let prepared = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &indices,
        dir.path(),
        &http_client,
        &config,
        None,
        &["foo@1.5.0".to_string()],
        false,
        false,
        false,
        &[DependencyGroup::Prod],
        0,
        None,
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
        Some("1.5.0"),
    );
}

#[tokio::test]
async fn selected_update_no_save_skips_a_selector_outside_the_kept_range() {
    let dir = tempdir().expect("create tempdir");
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "packages:\n  - '*'\n")
        .expect("write workspace manifest");
    let mut projects = vec![project_with_foo(dir.path(), "a")];
    let ordered_dirs = [projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);
    let config = Config::new();
    let http_client = std::sync::Arc::new(ThrottledClient::default());

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
        None,
        false,
        None,
    )
    .await
    .expect("prepare selected manifests");

    // 2.0.0 falls outside the kept ^1.0.0, so the selector is skipped:
    // the in-memory manifest keeps the declared range and no rewrite is
    // recorded for resolution.
    assert_eq!(dependency_specifier(&projects[0].manifest), "^1.0.0");
    assert!(prepared.persist_indices.is_empty());
    assert!(prepared.catalogs_override.is_none());
}

#[tokio::test]
async fn selected_update_depth_zero_skips_projects_without_a_matching_dependency() {
    let dir = tempdir().expect("create tempdir");
    let mut projects = [project_without_foo(dir.path(), "a"), project_with_foo(dir.path(), "b")];
    let selected_indices = [0, 1];
    let config = Config::new();
    let http_client = std::sync::Arc::new(ThrottledClient::default());

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
        None,
        false,
        None,
    )
    .await
    .expect("prepare selected manifests");

    assert_eq!(dependency_specifier(&projects[1].manifest), "2.0.0");
    assert_eq!(prepared.persist_indices, vec![1]);
}

/// A recursive `--latest` that matches no project's dependencies at
/// `--depth 0` fails, where the single-project one quietly returns: with
/// no project left to mutate there is nothing for the run to have meant.
#[tokio::test]
async fn selected_update_latest_depth_zero_errors_when_no_project_matches() {
    let dir = tempdir().expect("create tempdir");
    let mut projects = [project_without_foo(dir.path(), "a"), project_without_foo(dir.path(), "b")];
    let selected_indices = [0, 1];
    let config = Config::new();
    let http_client = std::sync::Arc::new(ThrottledClient::default());

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
        None,
        false,
        None,
    )
    .await;

    assert!(
        matches!(prepared, Err(UpdateError::NoPackageInDependencies)),
        "an unmatched depth-0 selector must fail, whatever the preparation returned",
    );
}

// No resolver in the latest-capable chain claims any of these, so none of
// them may cost a network round trip during manifest preparation — which is
// what the closed-port registry enforces. `runtime:` is covered end to end
// by the `update_latest_keeps_runtime_dependency_on_the_runtime_resolver`
// CLI test instead: its resolver does claim it, against a mocked mirror.
#[tokio::test]
async fn latest_leaves_specifiers_no_resolver_claims() {
    for specifier in [
        "workspace:*",
        "workspace:^1.0.0",
        "workspace:../packages/foo",
        "link:../foo",
        "file:../foo.tgz",
        "github:user/repo",
        "git+ssh://git@github.com/user/repo.git#v1.0.0",
        "https://example.com/foo.tgz",
    ] {
        let dir = tempdir().expect("create tempdir");
        let mut projects = [project_with_foo_specifier(dir.path(), "a", specifier)];
        let config = unroutable_registry_config();
        let http_client = std::sync::Arc::new(ThrottledClient::default());

        let prepared = prepare_selected_manifests::<SilentReporter>(
            &mut projects,
            &[0],
            dir.path(),
            &http_client,
            &config,
            None,
            &[],
            true,
            false,
            true,
            &[DependencyGroup::Prod],
            0,
            None,
            false,
            None,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("update --latest reached the registry for {specifier}: {error}")
        });

        assert_eq!(dependency_specifier(&projects[0].manifest), specifier);
        assert!(
            prepared.persist_indices.is_empty(),
            "{specifier} was queued for a manifest rewrite",
        );
    }
}

#[tokio::test]
async fn latest_rewrites_a_specifier_the_npm_resolver_claims() {
    let dir = tempdir().expect("create tempdir");
    let mut projects = [project_with_foo(dir.path(), "a")];
    let config = unroutable_registry_config();
    let http_client = std::sync::Arc::new(ThrottledClient::default());

    let result = prepare_selected_manifests::<SilentReporter>(
        &mut projects,
        &[0],
        dir.path(),
        &http_client,
        &config,
        None,
        &[],
        true,
        false,
        true,
        &[DependencyGroup::Prod],
        0,
        None,
        false,
        None,
    )
    .await;

    assert!(result.is_err(), "a range specifier is the registry's to bump, so it must be fetched");
}

fn project_with_foo(root: &std::path::Path, name: &str) -> Project {
    project_with_foo_specifier(root, name, "^1.0.0")
}

fn project_with_foo_specifier(root: &std::path::Path, name: &str, specifier: &str) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    std::fs::write(
        &package_json,
        json!({ "name": name, "dependencies": { "foo": specifier } }).to_string(),
    )
    .expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
        dependency_manifest: None,
    }
}

// A closed port, so any dependency that reaches registry resolution fails
// loudly instead of hitting the network. Retries are off so that failure is
// immediate rather than a minute of backoff.
fn unroutable_registry_config() -> Config {
    Config { registry: "http://127.0.0.1:1/".to_string(), fetch_retries: 0, ..Config::new() }
}

fn project_without_foo(root: &std::path::Path, name: &str) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    std::fs::write(&package_json, json!({ "name": name }).to_string()).expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
        dependency_manifest: None,
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

#[test]
fn requested_version_outside_the_kept_range_is_excluded() {
    assert!(matches!(judge_against_kept_range("7.8.5", "^6.0.0"), KeptRangeVerdict::Excluded,));
    assert!(matches!(
        judge_against_kept_range("2.0.0-beta.1", "^2.0.0"),
        KeptRangeVerdict::Excluded,
    ));
}

#[test]
fn requested_version_inside_the_kept_range_is_admitted() {
    assert!(matches!(judge_against_kept_range("6.3.0", "^6.0.0"), KeptRangeVerdict::Admitted));
    // A range that admits prereleases admits this one.
    assert!(matches!(
        judge_against_kept_range("2.0.0-beta.1", "^2.0.0-0"),
        KeptRangeVerdict::Admitted,
    ));
}

#[test]
fn only_a_requested_version_gets_a_verdict() {
    // Range-against-range containment is not decided consistently across
    // semver implementations, so a request that names no version is left to
    // the specifier the manifest keeps.
    for (requested, kept) in
        [(">=6", "^6.0.0"), ("^7.0.0", "^6.0.0"), ("beta", "^6.0.0"), ("6.3.0", "workspace:*")]
    {
        assert!(
            matches!(judge_against_kept_range(requested, kept), KeptRangeVerdict::Undecided),
            "{requested:?} against {kept:?} should be undecided",
        );
    }
}

/// The update targets `selectors` produce for the lockfile name `name`,
/// rendered as `(covers 1.x, covers 2.x)` — the shape the reuse walk asks
/// [`UpdateTargets::covers`] for.
fn covers(selectors: &[&str], name: &str, versions: &[&str]) -> Vec<bool> {
    let parsed = selectors.iter().map(|selector| parse_update_param(selector)).collect::<Vec<_>>();
    let expanded = expand_update_selectors(&parsed);
    let mut targets = pnpm_resolving_deps_resolver::UpdateTargets::default();
    insert_update_target(&mut targets, &expanded, name);
    versions
        .iter()
        .map(|version| targets.covers(name, Some(&version.parse().expect("parse version"))))
        .collect()
}

#[test]
fn a_pinned_selector_targets_only_its_version_line() {
    assert_eq!(
        covers(&["js-yaml@3.15.1"], "js-yaml", &["3.15.0", "3.15.1", "4.3.0"]),
        [true, true, false],
    );
}

#[test]
fn a_pinned_zero_x_selector_targets_only_its_minor_line() {
    assert_eq!(covers(&["foo@0.2.5"], "foo", &["0.2.1", "0.3.0", "1.0.0"]), [true, false, false]);
}

#[test]
fn a_selector_that_names_no_single_version_targets_every_line() {
    for selector in ["foo", "foo@^3.15.1", "foo@latest"] {
        assert_eq!(covers(&[selector], "foo", &["3.15.0", "4.3.0"]), [true, true], "{selector}");
    }
}

#[test]
fn every_selector_that_claims_a_name_widens_its_lines() {
    assert_eq!(
        covers(&["foo@1.0.0", "foo@2.0.0"], "foo", &["1.5.0", "2.5.0", "3.0.0"]),
        [true, true, false],
    );
}

#[test]
fn an_alias_selector_scopes_the_aliased_package_by_version_line() {
    // `expand_update_selectors` turns `alias@npm:foo@100.1.0` into a
    // selector for `foo` on the 100.x line, which is the name the resolver
    // resolves the edge under.
    assert_eq!(covers(&["alias@npm:foo@100.1.0"], "foo", &["100.0.0", "101.0.0"]), [true, false]);
}

#[test]
fn expands_an_alias_selector_to_the_aliased_package() {
    let parsed = [parse_update_param("alias@npm:foo@100.1.0")];
    let expanded = expand_update_selectors(&parsed);

    assert_eq!(expanded.len(), 2);
    assert_eq!(expanded[1].pattern, "foo");
    assert_eq!(expanded[1].version.as_deref(), Some("100.1.0"));
}

#[test]
fn keeps_a_negated_alias_selector_negated() {
    let parsed = [parse_update_param("!alias@npm:foo@100.1.0")];
    let expanded = expand_update_selectors(&parsed);

    assert_eq!(expanded[1].pattern, "!foo");
}

/// Run the indirect-version check over `selectors` against a single manifest
/// declaring `foo` directly.
fn reject_indirect(selectors: &[&str]) -> Result<(), super::UpdateError> {
    let dir = tempdir().expect("create temp dir");
    let package_json = dir.path().join("package.json");
    std::fs::write(
        &package_json,
        json!({ "name": "a", "dependencies": { "foo": "^1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    let manifest = PackageManifest::from_path(package_json).expect("read package.json");
    let parsed = selectors.iter().map(|input| parse_update_param(input)).collect::<Vec<_>>();
    reject_versions_of_indirect_update_specs::<SilentReporter>(
        &parsed,
        &[&manifest],
        &[DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
        "prefix",
    )
}

#[test]
fn an_exact_version_nothing_declares_directly_is_rejected() {
    let err = reject_indirect(&["bar@1.2.3"]).expect_err("bar is not a direct dependency");
    let rendered = err.to_string();
    assert!(rendered.contains(r#""bar" (requested "1.2.3")"#), "{rendered}");
}

#[test]
fn a_version_any_manifest_declares_directly_is_accepted() {
    reject_indirect(&["foo@1.2.3"]).expect("foo is a direct dependency");
}

#[test]
fn a_negated_selector_is_not_judged() {
    // `!bar` excludes a name; the version on it requests nothing. Checked with
    // no manifests too: with one, the "everything but bar" matcher happens to
    // match some other direct dependency and hides the misclassification.
    reject_indirect(&["!bar@1.2.3"]).expect("a negated selector requests no version");
    let parsed = [parse_update_param("!bar@1.2.3")];
    reject_versions_of_indirect_update_specs::<SilentReporter>(
        &parsed,
        &[],
        &[DependencyGroup::Prod],
        "prefix",
    )
    .expect("a negated selector requests no version");
}

#[test]
fn a_range_or_a_tag_is_not_rejected() {
    for selector in ["bar@^1.2.3", "bar@latest"] {
        reject_indirect(&[selector]).unwrap_or_else(|err| panic!("{selector}: {err}"));
    }
}
