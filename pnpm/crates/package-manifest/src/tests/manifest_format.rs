use crate::{ManifestFormat, project_manifest_path, safe_read_project_manifest_from_dir};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn precedence_puts_the_preferred_format_first() {
    let orders =
        [ManifestFormat::Json, ManifestFormat::Json5, ManifestFormat::Yaml].map(|format| {
            format.precedence().collect::<Vec<_>>()
        });
    assert_eq!(
        orders,
        [
            vec!["package.json", "package.json5", "package.yaml"],
            vec!["package.json5", "package.json", "package.yaml"],
            vec!["package.yaml", "package.json", "package.json5"],
        ],
    );
}

#[test]
fn deserializes_from_the_setting_values() {
    let formats: Vec<ManifestFormat> =
        serde_json::from_value(json!(["json", "json5", "yaml"])).unwrap();
    assert_eq!(formats, [ManifestFormat::Json, ManifestFormat::Json5, ManifestFormat::Yaml]);
    serde_json::from_value::<ManifestFormat>(json!("jsonc")).unwrap_err();
}

#[test]
fn project_manifest_path_prefers_the_preferred_format() {
    let dir = tempdir().unwrap();
    for basename in ["package.json", "package.json5", "package.yaml"] {
        fs::write(dir.path().join(basename), "{}").unwrap();
    }
    assert_eq!(
        project_manifest_path(dir.path(), ManifestFormat::Yaml),
        dir.path().join("package.yaml"),
    );
    assert_eq!(
        project_manifest_path(dir.path(), ManifestFormat::Json),
        dir.path().join("package.json"),
    );
}

#[test]
fn project_manifest_path_falls_back_to_the_default_order() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), "{}").unwrap();
    fs::write(dir.path().join("package.yaml"), "{}").unwrap();
    assert_eq!(
        project_manifest_path(dir.path(), ManifestFormat::Json5),
        dir.path().join("package.json"),
    );
}

#[test]
fn project_manifest_path_is_package_json_for_a_new_project() {
    let dir = tempdir().unwrap();
    assert_eq!(
        project_manifest_path(dir.path(), ManifestFormat::Yaml),
        dir.path().join("package.json"),
    );
}

#[test]
fn safe_read_reads_the_preferred_format() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name": "stub"}"#).unwrap();
    fs::write(dir.path().join("package.json5"), "{name: 'real', // comment\n}").unwrap();
    let manifest =
        safe_read_project_manifest_from_dir(dir.path(), ManifestFormat::Json5).unwrap().unwrap();
    assert_eq!(manifest, json!({ "name": "real" }));
}
