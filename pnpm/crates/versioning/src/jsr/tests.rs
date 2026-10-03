use std::fs;

use pretty_assertions::assert_eq;

use super::{jsr_manifest_updates, save_with_jsr_manifests, top_level_version_span};
use crate::error::VersioningError;

fn replace_version(text: &str) -> Option<String> {
    let span = top_level_version_span(text)?;
    let mut updated = text.to_string();
    updated.replace_range(span, r#""2.0.0""#);
    Some(updated)
}

#[test]
fn replaces_only_the_root_version_literal() {
    let text = "\u{feff}{\n  // \"version\": \"0.0.0\"\n  \"name\": \"@scope/pkg\",\n  \"exports\": { \"version\": \"./version.ts\" },\n  /* \"version\": */ \"version\" : \"1.0.0\",\n}\n";
    assert_eq!(
        replace_version(text).as_deref(),
        Some(
            "\u{feff}{\n  // \"version\": \"0.0.0\"\n  \"name\": \"@scope/pkg\",\n  \"exports\": { \"version\": \"./version.ts\" },\n  /* \"version\": */ \"version\" : \"2.0.0\",\n}\n"
        ),
    );
}

#[test]
fn string_values_named_version_are_not_keys() {
    let text = r#"{"name":"version","description":"a \"version\": \"x\"","version":"1.0.0"}"#;
    assert_eq!(
        replace_version(text).as_deref(),
        Some(r#"{"name":"version","description":"a \"version\": \"x\"","version":"2.0.0"}"#),
    );
}

#[test]
fn nested_version_keys_are_skipped() {
    assert_eq!(replace_version(r#"{"exports":{"version":"1"},"tags":["version"]}"#), None);
}

fn bump_jsr_manifest(basename: &str, text: &str) -> Result<String, VersioningError> {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(basename);
    fs::write(&path, text).expect("write JSR manifest");
    let updates = jsr_manifest_updates(dir.path(), "2.0.0")?;
    save_with_jsr_manifests(&updates, || Ok::<_, VersioningError>(()))?;
    Ok(fs::read_to_string(&path).expect("read JSR manifest"))
}

#[test]
fn single_quoted_json5_strings_do_not_hide_the_root_version() {
    let text = r#"{ note: '} //', "exports": { "version": "./v.ts" }, "version": '1.0.0' }"#;
    assert_eq!(
        bump_jsr_manifest("jsr.jsonc", text).expect("bump jsr.jsonc"),
        r#"{ note: '} //', "exports": { "version": "./v.ts" }, "version": "2.0.0" }"#,
    );
}

#[test]
fn an_unquoted_version_key_fails_instead_of_editing_another_field() {
    let text = r#"{ "exports": { "version": "./v.ts" }, version: "1.0.0" }"#;
    let error = bump_jsr_manifest("jsr.jsonc", text).expect_err("an unlocatable version must fail");
    assert!(matches!(error, VersioningError::InvalidJsrManifest { .. }), "{error:?}");
}

#[test]
fn a_duplicate_version_key_fails_instead_of_editing_the_shadowed_one() {
    let text = r#"{"version":"1.0.0","version":"1.5.0"}"#;
    let error = bump_jsr_manifest("jsr.json", text).expect_err("a shadowed version must fail");
    assert!(matches!(error, VersioningError::InvalidJsrManifest { .. }), "{error:?}");
}

#[test]
fn a_failed_package_manifest_save_restores_the_jsr_manifests() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let original = "{\n  \"version\": \"1.0.0\" // released\n}\n";
    fs::write(dir.path().join("jsr.jsonc"), original).expect("write jsr.jsonc");
    let updates = jsr_manifest_updates(dir.path(), "2.0.0").expect("prepare the JSR update");

    let saved_contents = std::cell::RefCell::new(String::new());
    let result = save_with_jsr_manifests(&updates, || {
        *saved_contents.borrow_mut() =
            fs::read_to_string(dir.path().join("jsr.jsonc")).expect("read jsr.jsonc");
        Err(VersioningError::InvalidJsrManifest {
            path: dir.path().join("package.json"),
            reason: "simulated save failure".to_string(),
        })
    });

    assert!(result.is_err());
    assert_eq!(saved_contents.into_inner(), "{\n  \"version\": \"2.0.0\" // released\n}\n");
    assert_eq!(fs::read_to_string(dir.path().join("jsr.jsonc")).expect("read jsr.jsonc"), original);
}

#[test]
fn a_failed_restore_names_the_jsr_manifest_left_at_the_new_version() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let jsr_manifest = dir.path().join("jsr.json");
    fs::write(&jsr_manifest, r#"{"version":"1.0.0"}"#).expect("write jsr.json");
    let updates = jsr_manifest_updates(dir.path(), "2.0.0").expect("prepare the JSR update");

    let result = save_with_jsr_manifests(&updates, || {
        fs::remove_file(&jsr_manifest).expect("remove jsr.json");
        fs::create_dir(&jsr_manifest).expect("block the restore with a directory");
        Err(VersioningError::InvalidJsrManifest {
            path: dir.path().join("package.json"),
            reason: "simulated save failure".to_string(),
        })
    });

    let Err(VersioningError::RestoreJsrManifest { path, interrupted_by, .. }) = result else {
        panic!("expected a restore failure, got {result:?}");
    };
    assert_eq!(path, jsr_manifest);
    assert!(interrupted_by.contains("simulated save failure"), "{interrupted_by}");
}
