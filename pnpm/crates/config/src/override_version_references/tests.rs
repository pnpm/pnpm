use super::resolve_version_references;
use crate::workspace_yaml::LoadWorkspaceYamlError;
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::{TempDir, tempdir};

fn root_with_manifest(manifest: &serde_json::Value) -> TempDir {
    let root = tempdir().expect("create a temporary workspace root");
    fs::write(root.path().join("package.json"), manifest.to_string())
        .expect("write the root package.json");
    root
}

fn overrides_map(entries: &[(&str, &str)]) -> IndexMap<String, String> {
    entries.iter().map(|(selector, spec)| ((*selector).to_string(), (*spec).to_string())).collect()
}

#[test]
fn a_reference_resolves_to_the_specifier_of_a_direct_dependency() {
    let root = root_with_manifest(&serde_json::json!({
        "name": "root",
        "dependencies": { "is-odd": "3.0.1" },
        "devDependencies": { "rolldown": "~1.2.0" },
        "optionalDependencies": { "fsevents": "^2.3.0" },
    }));
    let mut overrides = overrides_map(&[
        ("is-odd", "$is-odd"),
        ("rolldown", "$rolldown"),
        ("fsevents", "$fsevents"),
    ]);

    resolve_version_references(&mut overrides, root.path()).expect("resolve the references");

    assert_eq!(
        overrides,
        overrides_map(&[("is-odd", "3.0.1"), ("rolldown", "~1.2.0"), ("fsevents", "^2.3.0")]),
    );
}

/// The selector a reference is attached to is irrelevant — `$name`
/// names the dependency to copy the specifier from, not the dependency
/// being overridden.
#[test]
fn a_reference_may_name_a_dependency_other_than_the_overridden_one() {
    let root = root_with_manifest(&serde_json::json!({
        "name": "root",
        "dependencies": { "is-odd": "3.0.1" },
    }));
    let mut overrides = overrides_map(&[("is-even>is-odd", "$is-odd")]);

    resolve_version_references(&mut overrides, root.path()).expect("resolve the reference");

    assert_eq!(overrides, overrides_map(&[("is-even>is-odd", "3.0.1")]));
}

#[test]
fn values_without_a_reference_are_left_alone() {
    let root = root_with_manifest(&serde_json::json!({ "name": "root" }));
    let mut overrides =
        overrides_map(&[("is-odd", "3.0.1"), ("is-even", "catalog:"), ("foo", "-")]);
    let untouched = overrides.clone();

    resolve_version_references(&mut overrides, root.path()).expect("leave the values alone");

    assert_eq!(overrides, untouched);
}

#[test]
fn a_reference_to_a_missing_dependency_is_rejected() {
    let root = root_with_manifest(&serde_json::json!({
        "name": "root",
        "peerDependencies": { "is-odd": "3.0.1" },
    }));
    let mut overrides = overrides_map(&[("is-odd", "$is-odd")]);

    let error = resolve_version_references(&mut overrides, root.path())
        .expect_err("a peer dependency is not referenceable");

    assert!(
        matches!(
            &error,
            LoadWorkspaceYamlError::CannotResolveOverrideVersion { spec, dependency_name }
                if spec == "$is-odd" && dependency_name == "is-odd",
        ),
        "unexpected error: {error:?}",
    );
    assert_eq!(
        error.to_string(),
        r#"Cannot resolve version $is-odd in overrides. The direct dependencies don't have dependency "is-odd"."#,
    );
}

#[test]
fn a_reference_without_a_root_manifest_is_rejected() {
    let root = tempdir().expect("create a temporary workspace root");
    let mut overrides = overrides_map(&[("is-odd", "$is-odd")]);

    let error = resolve_version_references(&mut overrides, root.path())
        .expect_err("nothing can be referenced without a root manifest");

    assert!(
        matches!(&error, LoadWorkspaceYamlError::CannotResolveOverrideVersion { .. }),
        "unexpected error: {error:?}",
    );
}

#[test]
fn a_malformed_root_manifest_reports_itself() {
    let root = tempdir().expect("create a temporary workspace root");
    fs::write(root.path().join("package.json"), "{ not json").expect("write the root package.json");
    let mut overrides = overrides_map(&[("is-odd", "$is-odd")]);

    let error = resolve_version_references(&mut overrides, root.path())
        .expect_err("the unparsable manifest is the problem to report");

    assert!(
        matches!(&error, LoadWorkspaceYamlError::ReadRootManifest { .. }),
        "unexpected error: {error:?}",
    );
}

/// The read is skipped entirely when no value carries a reference, so a
/// workspace root without a manifest is not an error for everyone else.
#[test]
fn a_missing_root_manifest_is_fine_without_references() {
    let root = tempdir().expect("create a temporary workspace root");
    let mut overrides = overrides_map(&[("is-odd", "3.0.1")]);

    resolve_version_references(&mut overrides, root.path()).expect("no reference to resolve");

    assert_eq!(overrides, overrides_map(&[("is-odd", "3.0.1")]));
}

#[test]
fn an_empty_override_map_needs_no_root_manifest() {
    let mut empty = IndexMap::new();

    resolve_version_references(&mut empty, Path::new("/nonexistent"))
        .expect("no reference to resolve");

    assert!(empty.is_empty());
}
