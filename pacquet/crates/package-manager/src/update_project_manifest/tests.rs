use super::{
    CatalogLookup, ResolvedDirectDependency, UpdateProjectManifestOptions, WantedDependencyUpdate,
    update_project_manifest,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use serde_json::{Value, json};
use tempfile::TempDir;

fn manifest_from_json(value: &Value) -> (TempDir, PackageManifest) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(value).expect("serialize fixture"))
        .expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("read package.json");
    (dir, manifest)
}

fn wanted(alias: Option<&str>, bare_specifier: &str, update_spec: bool) -> WantedDependencyUpdate {
    WantedDependencyUpdate {
        alias: alias.map(ToString::to_string),
        bare_specifier: bare_specifier.to_string(),
        update_spec,
    }
}

fn resolved(
    alias: &str,
    version: Option<&str>,
    normalized_bare_specifier: Option<&str>,
) -> ResolvedDirectDependency {
    ResolvedDirectDependency {
        alias: alias.to_string(),
        version: version.map(ToString::to_string),
        normalized_bare_specifier: normalized_bare_specifier.map(ToString::to_string),
        catalog_lookup: None,
    }
}

#[test]
fn preserves_workspace_protocol_when_requested() {
    let (_dir, mut manifest) =
        manifest_from_json(&json!({ "dependencies": { "foo": "workspace:../packages/foo/dist" } }));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(Some("foo"), "workspace:../packages/foo/dist", true)],
            direct_dependencies: &[resolved(
                "foo",
                Some("1.0.0"),
                Some("link:../packages/foo/dist"),
            )],
            peer: false,
            pinned_version: None,
            target_dependencies_field: Some(DependencyGroup::Prod),
            preserve_workspace_protocol: true,
        },
    );
    assert_eq!(manifest.value()["dependencies"]["foo"], json!("workspace:../packages/foo/dist"));
}

#[test]
fn saves_normalized_local_spec_when_workspace_protocol_not_preserved() {
    let (_dir, mut manifest) =
        manifest_from_json(&json!({ "dependencies": { "foo": "workspace:../packages/foo/dist" } }));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(Some("foo"), "workspace:../packages/foo/dist", true)],
            direct_dependencies: &[resolved(
                "foo",
                Some("1.0.0"),
                Some("link:../packages/foo/dist"),
            )],
            peer: false,
            pinned_version: None,
            target_dependencies_field: Some(DependencyGroup::Prod),
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(manifest.value()["dependencies"]["foo"], json!("link:../packages/foo/dist"));
}

#[test]
fn saves_normalized_workspace_range_spec() {
    let (_dir, mut manifest) =
        manifest_from_json(&json!({ "dependencies": { "foo": "workspace:*" } }));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(Some("foo"), "workspace:*", true)],
            direct_dependencies: &[resolved("foo", Some("1.0.0"), Some("workspace:^1.0.0"))],
            peer: false,
            pinned_version: None,
            target_dependencies_field: Some(DependencyGroup::Prod),
            preserve_workspace_protocol: true,
        },
    );
    assert_eq!(manifest.value()["dependencies"]["foo"], json!("workspace:^1.0.0"));
}

#[test]
fn preserves_catalog_specifier_precedence() {
    let (_dir, mut manifest) =
        manifest_from_json(&json!({ "dependencies": { "foo": "workspace:../packages/foo/dist" } }));
    let direct = ResolvedDirectDependency {
        alias: "foo".to_string(),
        version: Some("1.0.0".to_string()),
        normalized_bare_specifier: Some("^1.0.0".to_string()),
        catalog_lookup: Some(CatalogLookup {
            catalog_name: "default".to_string(),
            specifier: "^1.0.0".to_string(),
            user_specified_bare_specifier: "catalog:".to_string(),
        }),
    };
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(Some("foo"), "workspace:../packages/foo/dist", true)],
            direct_dependencies: &[direct],
            peer: false,
            pinned_version: None,
            target_dependencies_field: Some(DependencyGroup::Prod),
            preserve_workspace_protocol: true,
        },
    );
    assert_eq!(manifest.value()["dependencies"]["foo"], json!("catalog:"));
}

#[test]
fn does_not_update_unrelated_dependency_when_optional_update_fails_to_resolve() {
    let (_dir, mut manifest) = manifest_from_json(&json!({
        "devDependencies": { "react": "19.0.0" },
        "optionalDependencies": { "react-dom": "19.0.0" },
    }));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[
                wanted(Some("react-dom"), "foo", true),
                wanted(Some("react"), "19.0.0", false),
            ],
            // `react-dom` failed to resolve, so only `react` is in the
            // resolved set.
            direct_dependencies: &[resolved("react", Some("19.0.0"), None)],
            peer: false,
            pinned_version: None,
            target_dependencies_field: None,
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(
        *manifest.value(),
        json!({
            "devDependencies": { "react": "19.0.0" },
            "optionalDependencies": { "react-dom": "19.0.0" },
        }),
    );
}

#[test]
fn updates_manifest_for_github_shorthand_without_alias() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(
                None,
                "pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf",
                true,
            )],
            direct_dependencies: &[resolved(
                "test-git-fetch",
                None,
                Some("github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf"),
            )],
            peer: false,
            pinned_version: None,
            target_dependencies_field: None,
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(
        *manifest.value(),
        json!({
            "dependencies": {
                "test-git-fetch": "github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf",
            },
        }),
    );
}

#[test]
fn updates_manifest_for_aliasless_dep_whose_specifier_does_not_resemble_resolution() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(None, "jsr:@foo/bar", true)],
            direct_dependencies: &[resolved("@foo/bar", Some("0.1.0"), Some("jsr:^0.1.0"))],
            peer: false,
            pinned_version: None,
            target_dependencies_field: None,
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(*manifest.value(), json!({ "dependencies": { "@foo/bar": "jsr:^0.1.0" } }));
}

#[test]
fn pairs_multiple_aliasless_deps_with_resolutions_in_order() {
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    update_project_manifest(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[
                wanted(Some("foo"), "^1.0.0", true),
                wanted(None, "jsr:@rus/greet@0.0.3", true),
                wanted(None, "github:kevva/is-positive#97edff6", true),
            ],
            direct_dependencies: &[
                resolved("foo", Some("1.0.0"), Some("^1.0.0")),
                resolved("@rus/greet", Some("0.0.3"), Some("jsr:^0.0.3")),
                resolved("is-positive", None, Some("github:kevva/is-positive#97edff6")),
            ],
            peer: false,
            pinned_version: None,
            target_dependencies_field: Some(DependencyGroup::Prod),
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(
        *manifest.value(),
        json!({
            "dependencies": {
                "foo": "^1.0.0",
                "@rus/greet": "jsr:^0.0.3",
                "is-positive": "github:kevva/is-positive#97edff6",
            },
        }),
    );
}
