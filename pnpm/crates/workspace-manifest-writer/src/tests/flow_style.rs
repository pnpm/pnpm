use super::{
    TempDir,
    UpdateWorkspaceManifestOptions,
    WORKSPACE_MANIFEST_FILENAME,
    catalogs,
    fs,
    run,
    run_age_excludes,
    run_allow_builds,
    run_config_dep,
    run_ignore_ghsas,
    run_patched_deps,
    run_remove_overrides,
    run_scaffold_allow_builds,
    update_workspace_manifest,
};

#[test]
fn catalog_entry_is_added_to_a_flow_mapping() {
    let out =
        run(Some("catalog: { foo: ^1.0.0 }\n"), &catalogs(&[("default", &[("bar", "^2.0.0")])]));
    assert_eq!(out.as_deref(), Some("catalog: { bar: ^2.0.0, foo: ^1.0.0 }\n"));
}

#[test]
fn catalog_entry_is_updated_in_a_flow_mapping() {
    let out = run(
        Some("catalog: { foo: ^1.0.0 } # pins\n"),
        &catalogs(&[("default", &[("foo", "^2.0.0")])]),
    );
    assert_eq!(out.as_deref(), Some("catalog: { foo: ^2.0.0 } # pins\n"));
}

#[test]
fn named_catalog_entry_is_added_to_a_nested_flow_mapping() {
    let out = run(
        Some("catalogs: { myCatalog: { foo: ^1.0.0 } }\n"),
        &catalogs(&[("myCatalog", &[("bar", "^2.0.0")])]),
    );
    assert_eq!(out.as_deref(), Some("catalogs: { myCatalog: { bar: ^2.0.0, foo: ^1.0.0 } }\n"));
}

#[test]
fn a_new_named_catalog_is_added_to_a_flow_catalogs_mapping() {
    let out = run(
        Some("catalogs: { myCatalog: { foo: ^1.0.0 } }\n"),
        &catalogs(&[("newCatalog", &[("bar", "^2.0.0")])]),
    );
    assert_eq!(
        out.as_deref(),
        Some("catalogs: { myCatalog: { foo: ^1.0.0 }, newCatalog: { bar: ^2.0.0 } }\n"),
    );
}

#[test]
fn config_dependency_is_added_to_a_flow_mapping() {
    let out = run_config_dep(Some("configDependencies: { foo: 1.0.0 }\n"), "bar", "2.0.0");
    assert_eq!(out, "configDependencies: { bar: 2.0.0, foo: 1.0.0 }\n");
}

#[test]
fn allow_build_is_added_to_a_flow_mapping() {
    let out = run_allow_builds(Some("allowBuilds: { foo: true }\n"), &[("bar", false)]);
    assert_eq!(out.as_deref(), Some("allowBuilds: { bar: false, foo: true }\n"));
}

#[test]
fn allow_build_is_updated_in_a_flow_mapping() {
    let out = run_allow_builds(Some("allowBuilds: { foo: true }\n"), &[("foo", false)]);
    assert_eq!(out.as_deref(), Some("allowBuilds: { foo: false }\n"));
}

#[test]
fn undecided_allow_build_is_added_to_a_flow_mapping() {
    let out = run_scaffold_allow_builds(Some("allowBuilds: { foo: true }\n"), &["bar"]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds: { bar: set this to true or false, foo: true }\n"),
    );
}

#[test]
fn undecided_allow_build_leaves_a_decided_flow_entry_alone() {
    let original = "allowBuilds: { foo: true }\n";
    let out = run_scaffold_allow_builds(Some(original), &["foo"]);
    assert_eq!(out.as_deref(), Some(original));
}

#[test]
fn patched_dependency_is_added_to_a_flow_mapping() {
    let out = run_patched_deps(
        Some("patchedDependencies: { foo: patches/foo.patch }\n"),
        &[("foo", "patches/foo.patch"), ("bar", "patches/bar.patch")],
    );
    assert_eq!(out, "patchedDependencies: { bar: patches/bar.patch, foo: patches/foo.patch }\n");
}

#[test]
fn omitted_patched_dependency_is_dropped_from_a_flow_mapping() {
    let out = run_patched_deps(
        Some("patchedDependencies: { foo: patches/foo.patch, bar: patches/bar.patch }\n"),
        &[("bar", "patches/bar.patch")],
    );
    assert_eq!(out, "patchedDependencies: { bar: patches/bar.patch }\n");
}

#[test]
fn minimum_release_age_excludes_stay_a_flow_sequence() {
    let out = run_age_excludes(
        Some("minimumReleaseAgeExclude: [foo@1.0.0]\n"),
        &["foo@1.0.0", "bar@2.0.0"],
    );
    assert_eq!(out.as_deref(), Some("minimumReleaseAgeExclude: [ foo@1.0.0, bar@2.0.0 ]\n"));
}

#[test]
fn ignore_ghsas_stay_a_flow_sequence_under_a_block_audit_config() {
    let out = run_ignore_ghsas(
        Some("auditConfig:\n  ignoreGhsas: [GHSA-aaaa-bbbb-cccc, GHSA-dddd-eeee-ffff]\n"),
        &["GHSA-gggg-hhhh-iiii"],
    );
    assert_eq!(out.as_deref(), Some("auditConfig:\n  ignoreGhsas: [ GHSA-gggg-hhhh-iiii ]\n"));
}

#[test]
fn ignore_ghsas_are_added_to_a_flow_audit_config() {
    let out = run_ignore_ghsas(Some("auditConfig: {}\n"), &["GHSA-aaaa-bbbb-cccc"]);
    assert_eq!(out.as_deref(), Some("auditConfig: { ignoreGhsas: [ GHSA-aaaa-bbbb-cccc ] }\n"));
}

#[test]
fn a_multiline_flow_mapping_is_refused_rather_than_flattened() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "allowBuilds: {\n  foo: true, # decided\n}\n";
    fs::write(&path, original).expect("seed manifest");

    let err = crate::set_allow_builds(dir.path(), [("bar", true)])
        .expect_err("must refuse a multi-line inline allowBuilds block");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
}

#[test]
fn a_multiline_flow_sequence_is_refused_rather_than_rewritten() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "minimumReleaseAgeExclude: [\n  foo@1.0.0, # pinned\n  bar@2.0.0,\n]\n";
    fs::write(&path, original).expect("seed manifest");

    let err = crate::set_minimum_release_age_excludes(dir.path(), &["baz@3.0.0".to_string()])
        .expect_err("must refuse a multi-line inline sequence");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
}

#[test]
fn a_multiline_flow_block_is_dropped_whole_when_it_empties() {
    let original = "packages:\n  - '*'\noverrides: {\n  foo: link:../foo, # pinned\n}\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn a_multiline_flow_block_is_deleted_whole_by_config_delete() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "overrides: {\n  foo: 1.0.0, # pinned\n}\npackages:\n  - '*'\n")
        .expect("seed manifest");

    crate::update_manifest_field(&path, "overrides", &serde_json::Value::Null)
        .expect("update_manifest_field succeeds");

    assert_eq!(fs::read_to_string(&path).expect("read manifest"), "packages:\n  - '*'\n");
}

/// A block whose value has the wrong shape for its setting never reaches
/// the writers: the typed parse rejects the manifest first.
#[test]
fn a_flow_collection_of_the_wrong_kind_fails_to_parse() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "allowBuilds: [ foo ]\n";
    fs::write(&path, original).expect("seed manifest");

    let err = crate::set_allow_builds(dir.path(), [("bar", true)])
        .expect_err("must refuse a sequence where allowBuilds expects a mapping");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Parse { .. }));
    assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
}

#[test]
fn a_whole_document_flow_mapping_is_refused_rather_than_corrupted() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // The keys of a document written as one flow mapping are not
    // top-level lines, so no splice — nor a new top-level block — can
    // address them.
    let original = "{ overrides: { foo: 1.0.0 } }\n";
    fs::write(&path, original).expect("seed manifest");

    let err = crate::set_overrides(dir.path(), [("bar", "2.0.0")])
        .expect_err("must refuse a whole-document flow mapping");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
}

#[test]
fn an_aliased_block_is_refused_rather_than_corrupted() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "catalog: &pins { foo: ^1.0.0 }\ncatalogs: { other: *pins }\n";
    fs::write(&path, original).expect("seed manifest");

    let err = update_workspace_manifest(
        dir.path(),
        &UpdateWorkspaceManifestOptions {
            updated_catalogs: Some(&catalogs(&[("other", &[("bar", "^2.0.0")])])),
            ..Default::default()
        },
    )
    .expect_err("must refuse an aliased catalog block");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    assert_eq!(fs::read_to_string(&path).expect("read manifest"), original);
}
