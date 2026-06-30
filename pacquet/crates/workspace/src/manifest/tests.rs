use super::{
    InvalidWorkspaceManifestError, ReadWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceManifest, read_workspace_manifest, workspace_package_patterns,
};
use pacquet_catalogs_types::{Catalog, Catalogs};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

#[test]
fn missing_file_returns_none() {
    let tmp = TempDir::new().unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap();
    assert_eq!(manifest, None);
}

#[test]
fn empty_file_returns_default() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "").unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap();
    assert_eq!(manifest, Some(WorkspaceManifest::default()));
}

#[test]
fn parses_packages_array() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\n  - apps/*\n",
    )
    .unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(manifest.packages, Some(vec!["packages/*".to_string(), "apps/*".to_string()]));
}

#[test]
fn settings_only_manifest_leaves_packages_none() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join(WORKSPACE_MANIFEST_FILENAME),
        "storeDir: /tmp/store\nregistry: https://example.com/\n",
    )
    .unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(manifest.packages, None);
}

#[test]
fn workspace_package_patterns_default_settings_only_manifest_to_root() {
    let manifest = WorkspaceManifest { packages: None, ..WorkspaceManifest::default() };
    assert_eq!(workspace_package_patterns(&manifest), vec![".".to_string()]);
}

#[test]
fn workspace_package_patterns_preserve_explicit_empty_packages() {
    let manifest = WorkspaceManifest { packages: Some(Vec::new()), ..WorkspaceManifest::default() };
    assert_eq!(workspace_package_patterns(&manifest), Vec::<String>::new());
}

#[test]
fn empty_packages_array_preserved_as_some_empty() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages: []\n").unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(manifest.packages, Some(Vec::<String>::new()));
}

#[test]
fn parses_top_level_catalog_field() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join(WORKSPACE_MANIFEST_FILENAME),
        "catalog:\n  foo: ^1.0.0\n  bar: ^2.0.0\n",
    )
    .unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    let mut expected = Catalog::new();
    expected.insert("foo".to_string(), "^1.0.0".to_string());
    expected.insert("bar".to_string(), "^2.0.0".to_string());
    assert_eq!(manifest.catalog, Some(expected));
    assert_eq!(manifest.catalogs, None);
}

#[test]
fn parses_named_catalogs_field() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join(WORKSPACE_MANIFEST_FILENAME),
        "catalogs:\n  default:\n    foo: ^1.0.0\n  legacy:\n    bar: ^2.0.0\n",
    )
    .unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    let mut expected = Catalogs::new();
    expected
        .insert("default".to_string(), Catalog::from([("foo".to_string(), "^1.0.0".to_string())]));
    expected
        .insert("legacy".to_string(), Catalog::from([("bar".to_string(), "^2.0.0".to_string())]));
    assert_eq!(manifest.catalog, None);
    assert_eq!(manifest.catalogs, Some(expected));
}

#[test]
fn empty_package_entry_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages:\n  - ''\n  - apps/*\n")
        .unwrap();
    let err = read_workspace_manifest(tmp.path()).unwrap_err();
    assert!(
        matches!(
            err,
            ReadWorkspaceManifestError::Invalid(InvalidWorkspaceManifestError::EmptyPackageEntry),
        ),
        "unexpected error: {err}",
    );
}
