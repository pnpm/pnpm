use super::{PackageManifestError, RawPackageManifest, assert_eq, json, read_to_string, tempdir};

#[test]
fn raw_manifest_leaves_runtime_engines_unfolded_on_read_and_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let parsed = json!({
        "name": "fixture",
        "devEngines": {
            "runtime": {
                "name": "node",
                "version": "22.14.0",
                "onFail": "download",
            },
        },
        "engines": {
            "runtime": {
                "name": "node",
                "version": "22.14.0",
                "onFail": "download",
            },
        },
    });
    let initial = format!("{}\n", serde_json::to_string_pretty(&parsed).unwrap());
    std::fs::write(&path, &initial).unwrap();

    let mut manifest = RawPackageManifest::from_path(path.clone()).unwrap();
    assert!(
        manifest
            .value()
            .get("devDependencies")
            .is_none(),
    );
    assert!(
        manifest
            .value()
            .get("dependencies")
            .is_none(),
    );

    manifest.value_mut()["devEngines"]["runtime"]["version"] = json!("24.0.0");
    manifest.save().unwrap();

    let saved: serde_json::Value = serde_json::from_str(&read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["devEngines"]["runtime"]["version"], "24.0.0");
    assert!(saved.get("devDependencies").is_none());
    assert!(saved.get("dependencies").is_none());
}

#[test]
fn raw_manifest_preserves_empty_dependency_maps_and_key_order() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let initial = "{\n\t\"name\": \"fixture\",\n\t\"dependencies\": {\n\t\t\"zebra\": \"1.0.0\",\n\t\t\"alpha\": \"2.0.0\"\n\t}\n}";
    std::fs::write(&path, initial).unwrap();

    let mut manifest = RawPackageManifest::from_path(path.clone()).unwrap();
    manifest.value_mut()["devDependencies"] = json!({});
    manifest.save().unwrap();

    assert_eq!(
        read_to_string(&path).unwrap(),
        "{\n\t\"name\": \"fixture\",\n\t\"dependencies\": {\n\t\t\"zebra\": \"1.0.0\",\n\t\t\"alpha\": \"2.0.0\"\n\t},\n\t\"devDependencies\": {}\n}",
    );
}

#[test]
fn raw_manifest_skips_noop_writes_and_writes_reordered_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let with_bom = "\u{feff}{\n  \"scripts\": {\n    \"a\": \"1\",\n    \"b\": \"2\"\n  }\n}\n";
    std::fs::write(&path, with_bom).unwrap();

    let mut manifest = RawPackageManifest::from_path(path.clone()).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), with_bom);

    let mut reordered = serde_json::Map::new();
    reordered.insert("b".to_string(), json!("2"));
    reordered.insert("a".to_string(), json!("1"));
    manifest.value_mut()["scripts"] = serde_json::Value::Object(reordered);
    manifest.save().unwrap();

    assert_eq!(
        read_to_string(&path).unwrap(),
        "{\n  \"scripts\": {\n    \"b\": \"2\",\n    \"a\": \"1\"\n  }\n}\n",
    );
}

#[test]
fn raw_manifest_rejects_non_object_root_on_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, "{\"name\":\"fixture\"}\n").unwrap();

    let mut manifest = RawPackageManifest::from_path(path).unwrap();
    *manifest.value_mut() = json!(["not", "an", "object"]);
    assert!(matches!(manifest.save(), Err(PackageManifestError::InvalidRoot { .. })));
}
