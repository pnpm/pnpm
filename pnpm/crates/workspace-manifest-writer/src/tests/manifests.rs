use super::{
    TempDir, UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, fs,
    run_allow_builds_clearing_legacy, run_patched_deps, run_patched_deps_path,
    run_remove_overrides, run_update_field, run_with,
};

#[test]
fn allow_builds_rejects_a_manifest_with_duplicate_keys() {
    // A repo-controlled manifest with duplicate `allowBuilds` keys is
    // rejected at parse time (`DuplicateMappingKey`), so `set_allow_builds`
    // errors and writes nothing rather than rewriting only the first
    // occurrence and leaving the effective (last) value untouched. The
    // policy change fails loudly instead of being silently bypassed.
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "allowBuilds:\n  esbuild: false\n  esbuild: true\n";
    fs::write(&path, original).expect("seed manifest");

    let result = crate::set_allow_builds(dir.path(), [("esbuild", false)]);
    assert!(
        matches!(result, Err(crate::UpdateWorkspaceManifestError::Parse { .. })),
        "duplicate keys must be rejected, got {result:?}",
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read manifest"),
        original,
        "the manifest is left unchanged when the update fails",
    );
}

#[test]
fn patched_dependency_preserves_existing_manifest_content() {
    let original = "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(
        Some(original),
        &[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")],
    );
    assert_eq!(
        out,
        "packages:\n  - '*'\n\nallowBuilds:\n  foo: true\n\ncatalog:\n  react: 18.2.0\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
    );
}

#[test]
fn patched_dependency_empty_map_does_not_create_manifest() {
    let (_dir, path) = run_patched_deps_path(None, &[]);

    assert!(!path.exists(), "empty patchedDependencies should not create pnpm-workspace.yaml");
}

#[test]
fn patched_dependency_empty_map_preserves_manifest_without_patch_block() {
    let original = "packages:\n  - '*'\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, original);
}

#[test]
fn write_or_remove_manifest_ignores_missing_empty_manifest() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let manifest = crate::model::Manifest::parse(Some("")).expect("empty manifest");

    crate::write_or_remove_manifest(&path, manifest).expect("remove missing empty manifest");

    assert!(!path.exists());
}

#[test]
fn write_or_remove_manifest_reports_remove_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::create_dir(&path).expect("create manifest dir");
    let manifest = crate::model::Manifest::parse(Some("")).expect("empty manifest");

    let err =
        crate::write_or_remove_manifest(&path, manifest).expect_err("directory remove should fail");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Remove { .. }));
}

#[test]
fn write_or_remove_manifest_reports_write_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("missing").join(WORKSPACE_MANIFEST_FILENAME);
    let manifest = crate::model::Manifest::parse(Some("packages:\n  - '*'\n")).expect("manifest");

    let err =
        crate::write_or_remove_manifest(&path, manifest).expect_err("missing parent should fail");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Write { .. }));
}

#[test]
fn minimum_release_age_excludes_are_added_to_the_local_manifest_values() {
    let added = ["local@2.0.0".to_string()];
    let out = run_with(
        Some("minimumReleaseAgeExclude:\n  - local@1.0.0\n"),
        &UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: &added,
            ..Default::default()
        },
    )
    .expect("written");

    assert_eq!(out, "minimumReleaseAgeExclude:\n  - local@1.0.0 || 2.0.0\n");
}

#[test]
fn remove_overrides_is_a_noop_when_the_manifest_is_missing() {
    assert_eq!(run_remove_overrides(None, &["foo"]), None);
}

#[test]
fn allow_builds_clearing_legacy_is_a_noop_when_the_manifest_is_missing() {
    assert_eq!(run_allow_builds_clearing_legacy(None, &[]), None);
}

#[test]
fn a_manifest_already_ending_in_a_blank_line_gains_no_second_one() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nstoreDir: ~/store\n\n"),
        "virtualStoreDir",
        &serde_json::json!(".pnpm"),
    )
    .expect("file written");
    assert_eq!(out, "cacheDir: ~/cache\n\nstoreDir: ~/store\n\nvirtualStoreDir: .pnpm\n");
}

#[test]
fn a_manifest_ending_in_a_whitespace_only_line_gains_no_second_blank() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nstoreDir: ~/store\n  \n"),
        "virtualStoreDir",
        &serde_json::json!(".pnpm"),
    )
    .expect("file written");
    assert_eq!(out, "cacheDir: ~/cache\n\nstoreDir: ~/store\n  \nvirtualStoreDir: .pnpm\n");
}

#[test]
fn delete_field_without_a_manifest_is_noop() {
    let out = run_update_field(None, "virtualStoreDir", &serde_json::Value::Null);
    assert_eq!(out, None);
}
