use super::{
    TempDir, WORKSPACE_MANIFEST_FILENAME, run_allow_builds, run_allow_builds_clearing_legacy,
    run_ignore_ghsas, run_prune_allow_builds, run_remove_overrides, run_scaffold_allow_builds,
    run_update_field,
};

#[test]
fn allow_builds_no_op_when_unchanged_keeps_file() {
    let original = "allowBuilds:\n  esbuild: true\n";
    let out = run_allow_builds(Some(original), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some(original));
}

/// Every install that keeps ignoring the same build re-runs the scaffold;
/// the second one must not rewrite the file (and bump its mtime).
#[test]
fn scaffold_allow_builds_no_op_when_already_scaffolded_keeps_file() {
    let original = "allowBuilds:\n  es5-ext: set this to true or false\n";
    let out = run_scaffold_allow_builds(Some(original), &["es5-ext"]);
    assert_eq!(out.as_deref(), Some(original));
}

#[test]
fn ignore_ghsas_replaces_an_existing_list() {
    let original = "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-aaaa-bbbb-cccc", "GHSA-dddd-eeee-ffff"])
        .expect("written");
    assert_eq!(
        out,
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n    - GHSA-dddd-eeee-ffff\n",
    );
}

#[test]
fn ignore_ghsas_noop_when_already_present() {
    let original = "auditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-aaaa-bbbb-cccc"]).expect("written");
    assert_eq!(out, original);
}

#[test]
fn ignore_ghsas_empty_removes_the_block() {
    let original = "packages:\n  - '*'\nauditConfig:\n  ignoreGhsas:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn ignore_ghsas_empty_with_sibling_only_is_a_noop() {
    let original = "auditConfig:\n  other: keep\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, original);
}

#[test]
fn ignore_ghsas_targets_the_canonical_audit_ignore_list() {
    let original = "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-dddd-eeee-ffff"]).expect("written");
    assert_eq!(out, "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-dddd-eeee-ffff\n");
}

#[test]
fn ignore_ghsas_removes_the_shadowed_deprecated_list_when_both_are_present() {
    let original = "audit:\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\nauditConfig:\n  ignoreGhsas:\n    - GHSA-1111-2222-3333\n";
    let out = run_ignore_ghsas(Some(original), &["GHSA-dddd-eeee-ffff"]).expect("written");
    assert_eq!(out, "audit:\n  ignore:\n    - GHSA-dddd-eeee-ffff\n");
}

#[test]
fn ignore_ghsas_empty_removes_audit_ignore_and_keeps_siblings() {
    let original = "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "audit:\n  ignorePrune: true\n");
}

#[test]
fn ignore_ghsas_empty_removes_the_audit_block_when_ignore_is_its_only_key() {
    let original = "packages:\n  - '.'\naudit:\n  ignore:\n    - GHSA-aaaa-bbbb-cccc\n";
    let out = run_ignore_ghsas(Some(original), &[]).expect("written");
    assert_eq!(out, "packages:\n  - '.'\n");
}

#[test]
fn ignore_ghsas_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);

    // A newline in the value would splice into a multi-line scalar.
    let err = crate::set_audit_ignore_ghsas(dir.path(), &["GHSA-aaaa\nbreak".to_string()])
        .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
    assert!(!path.exists(), "nothing should be written");
}

#[test]
fn remove_overrides_deletes_the_file_when_nothing_remains() {
    let original = "overrides:\n  foo: link:../foo\n  bar: link:../bar\n";
    assert_eq!(run_remove_overrides(Some(original), &["foo", "bar"]), None);
}

#[test]
fn allow_builds_clearing_legacy_deletes_the_file_when_nothing_remains() {
    let original = "onlyBuiltDependencies:\n  - esbuild\n";
    assert_eq!(run_allow_builds_clearing_legacy(Some(original), &[]), None);
}

#[test]
fn set_scalar_field_into_existing_file() {
    let out =
        run_update_field(Some("storeDir: ~/store\n"), "fetchTimeout", &serde_json::json!(1000))
            .expect("file written");
    let parsed: indexmap::IndexMap<String, serde_json::Value> =
        serde_saphyr::from_str(&out).expect("parse");
    assert_eq!(parsed["storeDir"], serde_json::json!("~/store"));
    assert_eq!(parsed["fetchTimeout"], serde_json::json!(1000));
}

#[test]
fn delete_last_field_removes_file() {
    let out = run_update_field(
        Some("virtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    );
    assert_eq!(out, None);
}

#[test]
fn prune_allow_builds_deletes_block_and_file_when_empty() {
    let original = "allowBuilds:\n  foo: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out, None);
}

#[test]
fn prune_allow_builds_prunes_a_dep_path_key_by_its_package_name() {
    let original = "allowBuilds:\n  \
         foo@git+https://github.com/org/foo.git#0000000000000000000000000000000000000000: set this to true or false\n  \
         bar@git+https://github.com/org/bar.git#0000000000000000000000000000000000000000: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &["foo"]);
    assert_eq!(
        out.as_deref(),
        Some(
            "allowBuilds:\n  foo@git+https://github.com/org/foo.git#0000000000000000000000000000000000000000: set this to true or false\n"
        ),
    );
}
