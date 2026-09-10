use super::{PackageManifest, PackageManifestError, tempdir};

#[test]
fn from_path_errors_no_importer_when_missing() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist").join("package.json");
    let result = PackageManifest::from_path(missing);
    let Err(err) = result else { panic!("missing package.json should not parse") };
    assert!(
        matches!(err, PackageManifestError::NoImporterManifestFound(_)),
        "expected NoImporterManifestFound, got {err:?}",
    );
}
