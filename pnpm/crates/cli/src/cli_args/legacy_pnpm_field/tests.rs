use super::{ignored_pnpm_field_keys, root_manifest};
use std::{fs, path::Path};

fn write_manifest(dir: &Path, contents: &str) {
    fs::write(dir.join("package.json"), contents).expect("write package.json");
}

fn keys_in(dir: &Path) -> Vec<String> {
    ignored_pnpm_field_keys(root_manifest(dir).as_ref())
}

#[test]
fn reports_migrated_keys_in_declaration_order() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(
        dir.path(),
        r#"{"pnpm":{"onlyBuiltDependencies":["a"],"app":{},"overrides":{"x":"1"}}}"#,
    );
    assert_eq!(
        keys_in(dir.path()),
        vec!["onlyBuiltDependencies".to_string(), "overrides".to_string()],
    );
}

#[test]
fn ignores_manifests_without_a_migrated_key() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), r#"{"pnpm":{"app":{}},"name":"x"}"#);
    assert!(keys_in(dir.path()).is_empty());
}

/// An editor-written manifest may open with a UTF-8 BOM, which every
/// other manifest read in the CLI strips. The warning has to see the
/// same keys those reads do.
#[test]
fn reports_migrated_keys_through_a_utf8_bom() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), "\u{feff}{\"pnpm\":{\"overrides\":{\"x\":\"1\"}}}");
    assert_eq!(keys_in(dir.path()), vec!["overrides".to_string()]);
}

/// A non-object `pnpm` field belongs to some other tool; there is
/// nothing to warn about, and a malformed or missing manifest is
/// reported by the install path with far better context.
#[test]
fn tolerates_absent_malformed_and_non_object_manifests() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(keys_in(dir.path()).is_empty(), "no manifest");

    write_manifest(dir.path(), "{ not json");
    assert!(keys_in(dir.path()).is_empty(), "malformed manifest");

    write_manifest(dir.path(), r#"{"pnpm":"11.0.0"}"#);
    assert!(keys_in(dir.path()).is_empty(), "non-object pnpm field");
}
