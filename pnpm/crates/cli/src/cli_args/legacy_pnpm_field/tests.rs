use super::ignored_pnpm_field_keys;
use std::fs;

fn write_manifest(dir: &std::path::Path, contents: &str) {
    fs::write(dir.join("package.json"), contents).expect("write package.json");
}

#[test]
fn reports_migrated_keys_in_declaration_order() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(
        dir.path(),
        r#"{"pnpm":{"onlyBuiltDependencies":["a"],"app":{},"overrides":{"x":"1"}}}"#,
    );
    assert_eq!(
        ignored_pnpm_field_keys(dir.path()),
        vec!["onlyBuiltDependencies".to_string(), "overrides".to_string()],
    );
}

#[test]
fn ignores_manifests_without_a_migrated_key() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), r#"{"pnpm":{"app":{}},"name":"x"}"#);
    assert!(ignored_pnpm_field_keys(dir.path()).is_empty());
}

/// A non-object `pnpm` field belongs to some other tool; there is
/// nothing to warn about, and a malformed or missing manifest is
/// reported by the install path with far better context.
#[test]
fn tolerates_absent_malformed_and_non_object_manifests() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(ignored_pnpm_field_keys(dir.path()).is_empty(), "no manifest");

    write_manifest(dir.path(), "{ not json");
    assert!(ignored_pnpm_field_keys(dir.path()).is_empty(), "malformed manifest");

    write_manifest(dir.path(), r#"{"pnpm":"11.0.0"}"#);
    assert!(ignored_pnpm_field_keys(dir.path()).is_empty(), "non-object pnpm field");
}
