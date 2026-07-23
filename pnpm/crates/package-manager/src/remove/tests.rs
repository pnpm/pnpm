//! Up-front validation cases for the `remove` handler. Each case
//! errors before any install runs, exercising [`validate_removable`].

use super::{
    RemoveValidationError, persist_selected_manifests, prepare_selected_manifests,
    selected_project_indices, validate_removable, validate_selected_remove,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::SilentReporter;
use pacquet_workspace::Project;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{collections::HashSet, path::PathBuf};
use tempfile::TempDir;

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn manifest(value: serde_json::Value) -> (PackageManifest, TempDir) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("package.json");
    std::fs::write(&path, value.to_string()).expect("write package.json");
    (PackageManifest::from_path(path).expect("read package.json"), dir)
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(std::string::ToString::to_string).collect()
}

fn expect_missing(
    manifest: &PackageManifest,
    names: &[&str],
    save_type: Option<DependencyGroup>,
) -> (String, Option<String>) {
    match validate_removable(manifest, &strings(names), save_type) {
        Err(RemoveValidationError::CannotRemoveMissingDeps { message, hint }) => (message, hint),
        other => panic!("expected CannotRemoveMissingDeps, got {other:?}"),
    }
}

#[test]
fn remove_should_fail_if_no_dependency_is_specified() {
    let (manifest, _dir) = manifest(json!({ "name": "x", "version": "1.0.0" }));
    let err = validate_removable(&manifest, &[], None).expect_err("empty removal must fail");
    assert!(matches!(err, RemoveValidationError::MustRemoveSomething), "got {err:?}");
    assert_eq!(err.to_string(), "At least one dependency name should be specified for removal");
}

#[test]
fn remove_should_fail_if_the_project_has_no_dependencies_at_all() {
    let (manifest, _dir) = manifest(json!({ "name": "x", "version": "1.0.0" }));

    assert_eq!(
        expect_missing(&manifest, &["express"], None),
        ("Cannot remove 'express': project has no dependencies of any kind".to_string(), None),
    );
    assert_eq!(
        expect_missing(&manifest, &["express"], Some(DependencyGroup::Prod)),
        ("Cannot remove 'express': project has no 'dependencies'".to_string(), None),
    );
    assert_eq!(
        expect_missing(&manifest, &["express"], Some(DependencyGroup::Dev)),
        ("Cannot remove 'express': project has no 'devDependencies'".to_string(), None),
    );
    assert_eq!(
        expect_missing(&manifest, &["express"], Some(DependencyGroup::Optional)),
        ("Cannot remove 'express': project has no 'optionalDependencies'".to_string(), None),
    );
}

#[test]
fn remove_should_fail_if_the_project_does_not_have_one_of_the_removed_dependencies() {
    let (manifest, _dir) = manifest(json!({
        "dependencies": { "prod-dep-1": "1.0.0", "prod-dep-2": "1.0.0" },
        "devDependencies": { "dev-dep-1": "1.0.0", "dev-dep-2": "1.0.0" },
        "optionalDependencies": { "optional-dep-1": "1.0.0", "optional-dep-2": "1.0.0" },
    }));
    let names = ["prod-dep-1", "dev-dep-1", "optional-dep-1"];

    assert_eq!(
        expect_missing(&manifest, &names, Some(DependencyGroup::Prod)),
        (
            "Cannot remove 'dev-dep-1', 'optional-dep-1': no such dependencies found in 'dependencies'".to_string(),
            Some("Available dependencies: prod-dep-1, prod-dep-2".to_string()),
        ),
    );
    assert_eq!(
        expect_missing(&manifest, &names, Some(DependencyGroup::Dev)),
        (
            "Cannot remove 'prod-dep-1', 'optional-dep-1': no such dependencies found in 'devDependencies'".to_string(),
            Some("Available dependencies: dev-dep-1, dev-dep-2".to_string()),
        ),
    );
    assert_eq!(
        expect_missing(&manifest, &names, Some(DependencyGroup::Optional)),
        (
            "Cannot remove 'prod-dep-1', 'dev-dep-1': no such dependencies found in 'optionalDependencies'".to_string(),
            Some("Available dependencies: optional-dep-1, optional-dep-2".to_string()),
        ),
    );

    assert_eq!(
        expect_missing(&manifest, &["express", "prod-dep-1", "dev-dep-1", "optional-dep-1"], None),
        (
            "Cannot remove 'express': no such dependency found".to_string(),
            Some(
                "Available dependencies: dev-dep-1, dev-dep-2, prod-dep-1, prod-dep-2, optional-dep-1, optional-dep-2"
                    .to_string(),
            ),
        ),
    );
}

#[test]
fn validate_removable_accepts_present_dependencies() {
    let (manifest, _dir) = manifest(json!({
        "dependencies": { "foo": "1.0.0" },
        "devDependencies": { "bar": "1.0.0" },
    }));
    validate_removable(&manifest, &strings(&["foo", "bar"]), None).expect("both names present");
    validate_removable(&manifest, &strings(&["bar"]), Some(DependencyGroup::Dev))
        .expect("bar present in devDependencies");
}

#[test]
fn selected_remove_prepares_and_persists_only_selected_projects() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mut projects = ["a", "b", "c"]
        .into_iter()
        .map(|name| project_with_dependencies(dir.path(), name, &["foo", "keep"]))
        .collect::<Vec<_>>();
    let ordered_dirs = [projects[1].root_dir.clone(), projects[0].root_dir.clone()];
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);

    validate_selected_remove(&strings(&["foo"])).expect("every selected manifest contains foo");
    prepare_selected_manifests::<SilentReporter>(&mut projects, &indices, &strings(&["foo"]), None);
    persist_selected_manifests::<SilentReporter>(&mut projects, &indices)
        .expect("persist selected manifests");

    assert_eq!(dependency_names(&projects[0].manifest), ["keep"]);
    assert_eq!(dependency_names(&projects[1].manifest), ["keep"]);
    assert_eq!(dependency_names(&projects[2].manifest), ["foo", "keep"]);
    assert_eq!(saved_dependency_names(&projects[0].manifest), ["keep"]);
    assert_eq!(saved_dependency_names(&projects[1].manifest), ["keep"]);
    assert_eq!(saved_dependency_names(&projects[2].manifest), ["foo", "keep"]);
}

#[test]
fn selected_remove_ignores_projects_without_the_requested_dependency() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let mut projects = vec![
        project_with_dependencies(dir.path(), "a", &["foo"]),
        project_with_dependencies(dir.path(), "b", &["bar"]),
    ];
    let ordered_dirs = projects.iter().map(|project| project.root_dir.clone()).collect::<Vec<_>>();
    let selected_dirs = ordered_dirs.iter().cloned().collect::<HashSet<_>>();
    let indices = selected_project_indices(&projects, &ordered_dirs, &selected_dirs);

    validate_selected_remove(&strings(&["foo"]))
        .expect("missing dependencies are ignored in recursive remove");
    prepare_selected_manifests::<SilentReporter>(&mut projects, &indices, &strings(&["foo"]), None);

    assert_eq!(dependency_names(&projects[0].manifest), Vec::<String>::new());
    assert_eq!(dependency_names(&projects[1].manifest), ["bar"]);
}

#[test]
fn selected_remove_still_requires_a_dependency_name() {
    let error = validate_selected_remove(&[]).expect_err("empty recursive removal must fail");
    assert!(matches!(error, RemoveValidationError::MustRemoveSomething));
}

fn project_with_dependencies(root: &std::path::Path, name: &str, dependencies: &[&str]) -> Project {
    let root_dir = root.join(name);
    std::fs::create_dir_all(&root_dir).expect("create project directory");
    let package_json = root_dir.join("package.json");
    let dependencies = dependencies
        .iter()
        .map(|dependency| ((*dependency).to_string(), json!("1.0.0")))
        .collect::<serde_json::Map<_, _>>();
    std::fs::write(
        &package_json,
        json!({ "name": name, "dependencies": dependencies }).to_string(),
    )
    .expect("write package.json");
    Project {
        root_dir,
        manifest: PackageManifest::from_path(package_json).expect("read package.json"),
    }
}

fn dependency_names(manifest: &PackageManifest) -> Vec<String> {
    manifest.available_dependency_names(Some(DependencyGroup::Prod))
}

fn saved_dependency_names(manifest: &PackageManifest) -> Vec<String> {
    dependency_names(&PackageManifest::from_path(PathBuf::from(manifest.path())).expect("reread"))
}
