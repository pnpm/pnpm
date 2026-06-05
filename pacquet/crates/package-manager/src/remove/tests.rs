//! Ports the up-front validation cases of pnpm's `remove` handler
//! tests at
//! <https://github.com/pnpm/pnpm/blob/9cad8274fd/installing/commands/test/remove/remove.ts>.
//! Each case throws before any install runs, so [`validate_removable`]
//! is the faithful equivalent of pnpm calling `remove.handler` and
//! catching the error.

use super::{RemoveValidationError, validate_removable};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pretty_assertions::assert_eq;
use serde_json::json;
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

    // A single truly-missing name with no `--save-*` flag: singular noun,
    // no `in '<field>'` suffix, and the hint lists every field in
    // dev → prod → optional order.
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
