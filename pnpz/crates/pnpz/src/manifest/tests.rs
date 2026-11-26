use super::{
    MANIFEST_FILE_NAME,
    Manifest,
    ManifestError,
};
use pretty_assertions::assert_eq;
use std::{
    collections::BTreeMap,
    fs,
};
use tempfile::TempDir;

#[test]
fn a_missing_manifest_is_empty() {
    let temp = TempDir::new().unwrap();
    assert_eq!(Manifest::load(temp.path()).unwrap(), Manifest::default());
}

#[test]
fn keeps_a_version_that_yaml_would_read_as_a_number() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join(MANIFEST_FILE_NAME), "packages:\n  - python@3.10\n  - jq\n")
        .unwrap();
    let packages = Manifest::load(temp.path()).unwrap().packages();
    let expected = [("jq", "latest"), ("python", "3.10")];
    assert_eq!(
        packages,
        expected
            .map(|(name, version)| (name.to_string(), version.to_string()))
            .into(),
    );
}

#[test]
fn add_replaces_the_entry_of_the_same_package() {
    let temp = TempDir::new().unwrap();
    let mut manifest = Manifest::default();
    manifest.add("python@3.11").unwrap();
    manifest.add("jq").unwrap();
    manifest.add("python@3.12").unwrap();
    manifest.save(temp.path()).unwrap();

    let text = fs::read_to_string(temp.path().join(MANIFEST_FILE_NAME)).unwrap();
    assert_eq!(text, "packages:\n- jq\n- python@3.12\n");
    assert_eq!(Manifest::load(temp.path()).unwrap(), manifest);
}

#[test]
fn rejects_an_entry_that_is_not_a_package() {
    let mut manifest = Manifest::default();
    for spec in ["", "@1.0", "python@", "a/b", "name with space"] {
        let error = manifest.add(spec).unwrap_err();
        assert!(matches!(error, ManifestError::InvalidSpec { .. }), "{spec:?}: {error}");
    }
    assert_eq!(manifest.packages(), BTreeMap::new());
}

#[test]
fn rejects_an_unknown_key() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join(MANIFEST_FILE_NAME), "dependencies: []\n").unwrap();
    let error = Manifest::load(temp.path()).unwrap_err();
    assert!(matches!(error, ManifestError::Invalid { .. }), "{error}");
}
