use super::{
    ReadProjectManifestError, ReadProjectManifestOnlyError, read_exact_project_manifest,
    read_project_manifest_only, safe_read_project_manifest_only, try_read_project_manifest,
};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

fn write_manifest(dir: &std::path::Path, body: &str) {
    fs::write(dir.join("package.json"), body).unwrap();
}

fn write_yaml_manifest(dir: &std::path::Path, body: &str) {
    fs::write(dir.join("package.yaml"), body).unwrap();
}

#[test]
fn try_read_returns_manifest_when_present() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), r#"{"name": "alpha", "version": "1.2.3"}"#);
    let result = try_read_project_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(result.0, "package.json");
    assert_eq!(
        result.1
            .value()
            .get("name")
            .and_then(|v| v.as_str()),
        Some("alpha"),
    );
}

#[test]
fn try_read_returns_yaml_manifest_when_json_is_missing() {
    let tmp = TempDir::new().unwrap();
    write_yaml_manifest(tmp.path(), "name: alpha\nversion: 1.2.3\n");
    let result = try_read_project_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(result.0, "package.yaml");
    assert_eq!(
        result.1
            .value()
            .get("name")
            .and_then(|v| v.as_str()),
        Some("alpha"),
    );
}

#[test]
fn try_read_prefers_json_over_yaml() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), r#"{"name": "json", "version": "1.2.3"}"#);
    write_yaml_manifest(tmp.path(), "name: yaml\nversion: 1.2.3\n");
    let result = try_read_project_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(result.0, "package.json");
    assert_eq!(
        result.1
            .value()
            .get("name")
            .and_then(|v| v.as_str()),
        Some("json"),
    );
}

#[test]
fn try_read_returns_none_when_missing() {
    let tmp = TempDir::new().unwrap();
    assert!(try_read_project_manifest(tmp.path()).unwrap().is_none());
}

#[test]
fn safe_read_returns_none_when_missing() {
    let tmp = TempDir::new().unwrap();
    assert!(safe_read_project_manifest_only(tmp.path()).unwrap().is_none());
}

#[test]
fn strict_read_errors_when_missing() {
    let tmp = TempDir::new().unwrap();
    match read_project_manifest_only(tmp.path()) {
        Ok(_) => panic!("expected NoImporterManifestFound"),
        Err(ReadProjectManifestOnlyError::NoImporterManifestFound { project_dir }) => {
            assert_eq!(project_dir, tmp.path());
        }
        Err(err) => panic!("unexpected error: {err}"),
    }
}

#[test]
fn read_exact_rejects_other_basenames() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("package.toml");
    fs::write(&path, "name = 'alpha'\n").unwrap();
    match read_exact_project_manifest(&path) {
        Ok(_) => panic!("expected UnsupportedName"),
        Err(ReadProjectManifestError::UnsupportedName { basename }) => {
            assert_eq!(basename, "package.toml");
        }
        Err(err) => panic!("unexpected error: {err}"),
    }
}

#[test]
fn read_exact_accepts_package_json() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("package.json");
    fs::write(&path, r#"{"name": "beta", "version": "0.1.0"}"#).unwrap();
    let manifest = read_exact_project_manifest(&path).unwrap();
    assert_eq!(
        manifest
            .value()
            .get("name")
            .and_then(|v| v.as_str()),
        Some("beta"),
    );
}

#[test]
fn read_exact_accepts_package_yaml() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("package.yaml");
    fs::write(&path, "name: beta\nversion: 0.1.0\n").unwrap();
    let manifest = read_exact_project_manifest(&path).unwrap();
    assert_eq!(
        manifest
            .value()
            .get("name")
            .and_then(|v| v.as_str()),
        Some("beta"),
    );
}

#[test]
fn read_exact_accepts_package_yml() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("package.yml");
    fs::write(&path, "name: beta\nversion: 0.1.0\n").unwrap();
    let manifest = read_exact_project_manifest(&path).unwrap();
    assert_eq!(
        manifest
            .value()
            .get("name")
            .and_then(|v| v.as_str()),
        Some("beta"),
    );
}

/// A leading UTF-8 BOM is accepted in every manifest format, the same as
/// in `package.json`.
#[test]
fn reads_a_package_yaml_that_starts_with_a_utf8_bom() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("package.yaml");
    fs::write(&path, "\u{feff}name: bom\nversion: 1.0.0\n").unwrap();

    let manifest = read_exact_project_manifest(&path).unwrap();
    assert_eq!(manifest.value().get("name").unwrap(), "bom");
}

#[test]
fn read_exact_accepts_package_json5() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("package.json5");
    fs::write(&path, "{ name: 'json5', version: '1.2.3', }").unwrap();
    let manifest = read_exact_project_manifest(&path).unwrap();
    assert_eq!(manifest.value()["name"], "json5");
    assert_eq!(manifest.path(), path);
}

#[test]
fn try_read_prefers_json5_over_yaml() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("package.json5"), "{ name: 'json5' }").unwrap();
    write_yaml_manifest(tmp.path(), "name: yaml\n");
    let (name, mut manifest) = try_read_project_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(name, "package.json5");
    assert_eq!(manifest.value()["name"], "json5");
    manifest.value_mut()["version"] = "2.0.0".into();
    manifest.save().unwrap();
    assert_eq!(fs::read_to_string(tmp.path().join("package.yaml")).unwrap(), "name: yaml\n");
    let selected = crate::project_manifest_path(tmp.path());
    assert_eq!(selected, tmp.path().join("package.json5"));
}

#[test]
fn try_read_ignores_invalid_unselected_json5() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), r#"{"name":"json"}"#);
    fs::write(tmp.path().join("package.json5"), "{ invalid:").unwrap();
    write_yaml_manifest(tmp.path(), "name: yaml\n");
    let (name, manifest) = try_read_project_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(name, "package.json");
    assert_eq!(manifest.value()["name"], "json");
    assert_eq!(crate::project_manifest_path(tmp.path()), tmp.path().join("package.json"));
}

#[test]
fn try_read_does_not_fall_back_from_invalid_preferred_json() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), "{ invalid:");
    fs::write(tmp.path().join("package.json5"), "{ name: 'json5' }").unwrap();
    write_yaml_manifest(tmp.path(), "name: yaml\n");
    let error = try_read_project_manifest(tmp.path()).err().unwrap();
    eprintln!("ERROR: {error}");
    assert!(error.to_string().contains("package.json"));
}

#[test]
fn try_read_does_not_fall_back_from_invalid_preferred_json5() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("package.json5"), "{ invalid:").unwrap();
    write_yaml_manifest(tmp.path(), "name: yaml\n");
    let error = try_read_project_manifest(tmp.path()).err().unwrap();
    eprintln!("ERROR: {error}");
    assert!(error.to_string().contains("package.json5"));
}
