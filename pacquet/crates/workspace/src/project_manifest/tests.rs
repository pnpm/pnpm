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

#[test]
fn try_read_returns_manifest_when_present() {
    let tmp = TempDir::new().unwrap();
    write_manifest(tmp.path(), r#"{"name": "alpha", "version": "1.2.3"}"#);
    let result = try_read_project_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(result.0, "package.json");
    assert_eq!(result.1.value().get("name").and_then(|v| v.as_str()), Some("alpha"));
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
    let path = tmp.path().join("package.yaml");
    fs::write(&path, "name: alpha\n").unwrap();
    match read_exact_project_manifest(&path) {
        Ok(_) => panic!("expected UnsupportedName"),
        Err(ReadProjectManifestError::UnsupportedName { basename }) => {
            assert_eq!(basename, "package.yaml");
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
    assert_eq!(manifest.value().get("name").and_then(|v| v.as_str()), Some("beta"));
}
