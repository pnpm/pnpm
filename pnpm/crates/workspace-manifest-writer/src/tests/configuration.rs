use super::{
    TempDir, WORKSPACE_MANIFEST_FILENAME, fs, run_config_dep, run_ignore_ghsas,
    run_patched_deps_path,
};

#[test]
fn config_dependency_creates_block_when_absent() {
    let out = run_config_dep(None, "@pnpm.e2e/foo", "1.0.0");
    assert_eq!(out, "configDependencies:\n  '@pnpm.e2e/foo': 1.0.0\n");
}

#[test]
fn config_dependency_added_to_existing_block() {
    let original = "configDependencies:\n  '@pnpm.e2e/bar': 2.0.0\n";
    let out = run_config_dep(Some(original), "@pnpm.e2e/foo", "1.0.0");
    assert_eq!(out, "configDependencies:\n  '@pnpm.e2e/bar': 2.0.0\n  '@pnpm.e2e/foo': 1.0.0\n");
}

#[test]
fn config_dependencies_batch_updates_all_entries_in_one_manifest() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "# preserved comment\nconfigDependencies:\n  existing-package: 0.1.0\n")
        .expect("seed manifest");

    crate::set_config_dependencies(
        dir.path(),
        [("@pnpm.e2e/foo", "1.0.0"), ("@pnpm.e2e/bar", "2.0.0")],
    )
    .expect("batch update succeeds");

    let out = fs::read_to_string(path).expect("read updated manifest");
    assert_eq!(
        out,
        "# preserved comment\nconfigDependencies:\n  '@pnpm.e2e/bar': 2.0.0\n  '@pnpm.e2e/foo': 1.0.0\n  existing-package: 0.1.0\n",
    );
}

#[test]
fn config_dependency_upserts_existing_entry() {
    let original = "configDependencies:\n  '@pnpm.e2e/foo': 1.0.0\n";
    let out = run_config_dep(Some(original), "@pnpm.e2e/foo", "2.0.0");
    assert_eq!(out, "configDependencies:\n  '@pnpm.e2e/foo': 2.0.0\n");
}

#[test]
fn config_dependency_preserves_other_keys_and_comments() {
    let original = "# top comment\nstoreDir: ../store\n";
    let out = run_config_dep(Some(original), "pnpm-plugin-x", "1.2.3");
    assert!(out.contains("# top comment"), "comment preserved");
    assert!(out.contains("storeDir: ../store"), "existing key preserved");
    assert!(out.contains("configDependencies:\n  pnpm-plugin-x: 1.2.3"), "block appended");
}

#[test]
fn config_dependency_noop_when_unchanged_returns_false() {
    use crate::{edit, model::Manifest};

    let original = "configDependencies:\n  '@pnpm.e2e/foo': 1.0.0\n";

    let mut manifest = Manifest::parse(Some(original)).unwrap();
    assert!(
        !edit::add_config_dependency(&mut manifest, "@pnpm.e2e/foo", "1.0.0").unwrap(),
        "re-adding the same specifier should report no change",
    );

    let mut manifest = Manifest::parse(Some(original)).unwrap();
    assert!(
        edit::add_config_dependency(&mut manifest, "@pnpm.e2e/foo", "2.0.0").unwrap(),
        "changing the specifier should report a change",
    );
}

#[test]
fn patched_dependency_removes_manifest_when_last_setting_is_removed() {
    let original = "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n";
    let (_dir, path) = run_patched_deps_path(Some(original), &[]);

    assert!(!path.exists(), "empty pnpm-workspace.yaml should be removed");
}

#[test]
fn audit_config_block_is_created() {
    let out = run_ignore_ghsas(None, &["GHSA-aaaa-bbbb-cccc"]).expect("written");
    assert_eq!(out, "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n");
}

#[test]
fn audit_config_block_with_multiple_ghsas() {
    let out =
        run_ignore_ghsas(None, &["GHSA-aaaa-bbbb-cccc", "GHSA-dddd-eeee-ffff"]).expect("written");
    assert_eq!(
        out,
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n    - GHSA-dddd-eeee-ffff\n",
    );
}

#[test]
fn ignore_ghsas_adds_key_to_existing_audit_config() {
    let original = "auditConfig:\n  other: keep\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-aaaa-bbbb-cccc"]).expect("written");
    assert_eq!(out, "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n  other: keep\n");
}

#[test]
fn ignore_ghsas_empty_preserves_sibling_audit_config_keys() {
    let original = "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n  other: keep\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "auditConfig:\n  other: keep\n");
}

#[test]
fn ignore_ghsas_edits_an_inline_flow_audit_config() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "auditConfig: { other: keep, ignoreGhsas: [GHSA-aaaa-bbbb-cccc] }\n")
        .expect("seed");

    crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-dddd-eeee-ffff".to_string()])
        .expect("set_audit_ignore_ghsas succeeds");

    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "auditConfig: { other: keep, ignoreGhsas: [ GHSA-dddd-eeee-ffff ] }\n");
}

#[test]
fn ignore_ghsas_refuses_a_multiline_flow_audit_config() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // Rebuilding a multi-line flow mapping onto one line would drop the
    // comments between its entries, so the write is refused instead.
    let original = "auditConfig: {\n  ignoreGhsas: [GHSA-aaaa-bbbb-cccc], # pinned\n}\n";
    fs::write(&path, original).expect("seed");

    let err = crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-dddd-eeee-ffff".to_string()])
        .expect_err("must refuse a multi-line inline auditConfig");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, original);
}

#[test]
fn setting_a_field_after_deleting_the_last_one_keeps_a_single_blank_line() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "cacheDir: ~/cache\n\nstoreDir: ~/store\n";
    fs::write(&path, original).expect("seed manifest");
    let with_field = format!("{original}\nvirtualStoreDir: .pnpm\n");

    for value in [serde_json::json!(".pnpm"), serde_json::Value::Null, serde_json::json!(".pnpm")] {
        crate::update_manifest_field(&path, "virtualStoreDir", &value).expect("update succeeds");
    }

    assert_eq!(fs::read_to_string(&path).expect("file kept"), with_field);
}
