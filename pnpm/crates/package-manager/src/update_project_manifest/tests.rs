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

fn apply(manifest: &mut PackageManifest, opts: &UpdateProjectManifestOptions<'_>) {
    update_project_manifest(manifest, opts).expect("update manifest");
}

fn wanted(alias: Option<&str>, bare_specifier: &str, update_spec: bool) -> WantedDependencyUpdate {
    WantedDependencyUpdate {
        alias: alias.map(ToString::to_string),
        bare_specifier: bare_specifier.to_string(),
        update_spec,
    }
}

/// A resolved direct dependency carrying the wanted dependency it came from.
fn resolved(
    alias: &str,
    version: Option<&str>,
    normalized_bare_specifier: Option<&str>,
    wanted_dependency: WantedDependencyUpdate,
) -> ResolvedDirectDependency {
    ResolvedDirectDependency {
        alias: alias.to_string(),
        version: version.map(ToString::to_string),
        normalized_bare_specifier: normalized_bare_specifier.map(ToString::to_string),
        catalog_lookup: None,
        wanted_dependency: Some(wanted_dependency),
    }
}

#[test]
fn preserves_workspace_protocol_when_requested() {
    let (_dir, mut manifest) =
        manifest_from_json(&json!({ "dependencies": { "foo": "workspace:../packages/foo/dist" } }));
    let foo = wanted(Some("foo"), "workspace:../packages/foo/dist", true);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: std::slice::from_ref(&foo),
            direct_dependencies: &[resolved(
                "foo",
                Some("1.0.0"),
                Some("link:../packages/foo/dist"),
                foo.clone(),
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
    let foo = wanted(Some("foo"), "workspace:../packages/foo/dist", true);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: std::slice::from_ref(&foo),
            direct_dependencies: &[resolved(
                "foo",
                Some("1.0.0"),
                Some("link:../packages/foo/dist"),
                foo.clone(),
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
    let foo = wanted(Some("foo"), "workspace:*", true);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: std::slice::from_ref(&foo),
            direct_dependencies: &[resolved(
                "foo",
                Some("1.0.0"),
                Some("workspace:^1.0.0"),
                foo.clone(),
            )],
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
    let foo = wanted(Some("foo"), "workspace:../packages/foo/dist", true);
    let direct = ResolvedDirectDependency {
        alias: "foo".to_string(),
        version: Some("1.0.0".to_string()),
        normalized_bare_specifier: Some("^1.0.0".to_string()),
        catalog_lookup: Some(CatalogLookup {
            catalog_name: "default".to_string(),
            specifier: "^1.0.0".to_string(),
            user_specified_bare_specifier: "catalog:".to_string(),
        }),
        wanted_dependency: Some(foo.clone()),
    };
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[foo],
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
    // `react-dom` is the failed optional update, so it is absent from
    // `direct_dependencies`.
    let react = wanted(Some("react"), "19.0.0", false);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[wanted(Some("react-dom"), "foo", true), react.clone()],
            direct_dependencies: &[resolved("react", Some("19.0.0"), None, react)],
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
    let selector =
        wanted(None, "pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf", true);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: std::slice::from_ref(&selector),
            direct_dependencies: &[resolved(
                "test-git-fetch",
                None,
                Some("github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf"),
                selector.clone(),
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
    let selector = wanted(None, "jsr:@foo/bar", true);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: std::slice::from_ref(&selector),
            direct_dependencies: &[resolved(
                "@foo/bar",
                Some("0.1.0"),
                Some("jsr:^0.1.0"),
                selector.clone(),
            )],
            peer: false,
            pinned_version: None,
            target_dependencies_field: None,
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(*manifest.value(), json!({ "dependencies": { "@foo/bar": "jsr:^0.1.0" } }));
}

#[test]
fn updates_aliasless_selector_that_resolves_to_an_existing_alias() {
    // The resolved alias collides with the existing (non-updating) manifest
    // entry; the resolution carries the new selector, so its spec wins.
    let (_dir, mut manifest) = manifest_from_json(&json!({
        "dependencies": {
            "test-git-fetch": "github:pnpm/test-git-fetch#0000000000000000000000000000000000000000",
        },
    }));
    let new_selector =
        wanted(None, "pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf", true);
    let existing_entry = wanted(
        Some("test-git-fetch"),
        "github:pnpm/test-git-fetch#0000000000000000000000000000000000000000",
        false,
    );
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[existing_entry, new_selector.clone()],
            direct_dependencies: &[resolved(
                "test-git-fetch",
                None,
                Some("github:pnpm/test-git-fetch#8b333f12d5357f4f25a654c305c826294cb073bf"),
                new_selector,
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
fn does_not_misattribute_a_spec_when_an_aliasless_optional_dep_fails_to_resolve() {
    // The survivor omits `normalized_bare_specifier` so its spec falls back to
    // the wanted dependency's — the path where a wrong pairing would surface.
    let (_dir, mut manifest) = manifest_from_json(&json!({}));
    let failed_optional =
        wanted(None, "github:owner/missing#1111111111111111111111111111111111111111", true);
    let survivor = wanted(None, "github:owner/good#2222222222222222222222222222222222222222", true);
    apply(
        &mut manifest,
        &UpdateProjectManifestOptions {
            wanted_dependencies: &[failed_optional, survivor.clone()],
            direct_dependencies: &[resolved("good", None, None, survivor)],
            peer: false,
            pinned_version: None,
            target_dependencies_field: None,
            preserve_workspace_protocol: false,
        },
    );
    assert_eq!(
        *manifest.value(),
        json!({
            "dependencies": { "good": "github:owner/good#2222222222222222222222222222222222222222" },
        }),
    );
}
