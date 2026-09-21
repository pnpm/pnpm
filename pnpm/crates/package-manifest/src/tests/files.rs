use super::{
    DependencyGroup, PackageManifest, PackageManifestError, assert_eq, json, read_to_string,
    tempdir,
};

#[test]
fn failed_save_preserves_existing_file_contents() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let raw = serde_json::to_string_pretty(&json!({
        "name": "fixture",
        "devEngines": "invalid",
        "devDependencies": {
            "node": "runtime:22",
        },
    }))
    .unwrap();
    std::fs::write(&path, &raw).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    assert!(matches!(manifest.save(), Err(PackageManifestError::InvalidAttribute(_))));
    assert_eq!(read_to_string(path).unwrap(), raw);
}

/// A save round-trips the source file's final-newline state: a file that
/// ends with a newline keeps it, a file that doesn't stays without one.
#[test]
fn save_preserves_the_final_newline_state_of_the_source_file() {
    let dir = tempdir().unwrap();

    let with_newline = dir.path().join("with-newline.json");
    std::fs::write(&with_newline, "{\n  \"name\": \"foo\"\n}\n").unwrap();
    let mut manifest = PackageManifest::from_path(with_newline.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert!(read_to_string(with_newline).unwrap().ends_with('\n'));

    let without_newline = dir.path().join("without-newline.json");
    std::fs::write(&without_newline, "{\n  \"name\": \"foo\"\n}").unwrap();
    let mut manifest = PackageManifest::from_path(without_newline.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert!(!read_to_string(without_newline).unwrap().ends_with('\n'));
}

/// A save that wouldn't change the manifest leaves the file byte-for-byte
/// untouched — even a file in non-canonical form (unsorted dependencies)
/// is only normalized when a real change triggers a write.
#[test]
fn noop_save_does_not_rewrite_the_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let original = "{\n  \"name\": \"foo\",\n  \"dependencies\": {\n    \"zebra\": \"1.0.0\",\n    \"aardvark\": \"2.0.0\"\n  }\n}";
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), original);

    // Re-adding an already-declared dependency at its existing version is
    // also a no-op.
    manifest.add_dependency("zebra", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), original);
}

#[test]
fn save_preserves_crlf_line_endings_of_the_source_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let original = "{\r\n  \"name\": \"foo\"\r\n}\r\n";
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    let result = read_to_string(&path).unwrap();
    assert!(result.contains("\r\n"));
    assert!(!result.replace("\r\n", "").contains('\n'));
    assert_eq!(
        result,
        "{\r\n  \"name\": \"foo\",\r\n  \"dependencies\": {\r\n    \"fastify\": \"1.0.0\"\r\n  }\r\n}\r\n"
    );
}

#[test]
fn save_preserves_crlf_line_endings_of_yaml_manifest() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    let original = "name: foo\r\nversion: 1.0.0\r\n";
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    let result = read_to_string(&path).unwrap();
    assert!(result.contains("fastify: 1.0.0"));
    assert!(result.contains("\r\n"));
    assert!(!result.replace("\r\n", "").contains('\n'));
}
