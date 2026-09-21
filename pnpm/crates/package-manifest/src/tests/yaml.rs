use super::{DependencyGroup, PackageManifest, assert_eq, json, read_to_string, tempdir};
use std::fs;

#[test]
fn yaml_save_preserves_comments_and_existing_key_order() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    let original = "# project\nname: fixture\n# dependencies\ndependencies:\n  zebra: '1.0.0' # keep\n  alpha: 1.0.0 # update\n# metadata\ncustom:\n  empty: {}\n  nullable: null\n  sequence: [null, {}]\n";
    fs::write(&path, original).unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("alpha", "2.0.0", DependencyGroup::Prod).unwrap();
    manifest.add_dependency("bravo", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert_eq!(
        read_to_string(&path).unwrap(),
        original
            .replace("alpha: 1.0.0", "alpha: 2.0.0")
            .replace("# metadata", "  bravo: 1.0.0\n# metadata"),
    );
    let written = read_to_string(&path).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(path).unwrap(), written);
    assert!(!dir.path().join("package.json").exists());
}

#[test]
fn yaml_from_value_updates_the_existing_document() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    fs::write(&path, "# project\nname: fixture\nversion: '1.0.0' # version\n").unwrap();
    let mut manifest =
        PackageManifest::from_value(path.clone(), json!({"name":"fixture", "version":"2.0.0"}));
    manifest.save().unwrap();
    assert_eq!(
        read_to_string(path).unwrap(),
        "# project\nname: fixture\nversion: 2.0.0 # version\n",
    );
}

#[test]
fn yaml_runtime_dependencies_round_trip_without_rewriting_a_noop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    let original = "# runtime\nname: fixture\ndevEngines:\n  runtime:\n    name: node\n    version: ^22.0.0\n    onFail: download\n";
    fs::write(&path, original).unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    assert_eq!(manifest.value()["devDependencies"]["node"], "runtime:^22.0.0");
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), original);
    manifest.value_mut()["version"] = json!("1.0.0");
    manifest.save().unwrap();
    assert_eq!(read_to_string(path).unwrap(), format!("{original}version: 1.0.0\n"));
}

#[test]
fn yaml_scalar_alias_edit_does_not_change_other_fields() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    fs::write(
        &path,
        "name: fixture\ndependencies:\n  alpha: &version 1.0.0 # version\n  bravo: *version\n",
    )
    .unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("alpha", "2.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    let reread = PackageManifest::from_path(path).unwrap();
    assert_eq!(reread.value()["dependencies"], json!({"alpha":"2.0.0","bravo":"1.0.0"}));
}

#[test]
fn yaml_failed_edit_leaves_the_file_intact() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    let original = "name: [unterminated\n";
    fs::write(&path, original).unwrap();
    let mut manifest = PackageManifest::from_value(path.clone(), json!({"name":"fixture"}));
    let error = manifest.save().unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&path.display().to_string()),
        "{error}",
    );
    assert_eq!(read_to_string(path).unwrap(), original);
}

#[test]
fn yaml_creation_and_empty_files_are_writable() {
    for source in [None, Some("# empty\n"), Some("{}\n")] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.yaml");
        if let Some(source) = source {
            fs::write(&path, source).unwrap();
        }
        let mut manifest = PackageManifest::create_if_needed(path.clone()).unwrap();
        manifest.add_dependency("alpha", "1.0.0", DependencyGroup::Prod).unwrap();
        manifest.save().unwrap();
        assert_eq!(
            PackageManifest::from_path(path).unwrap().value()["dependencies"],
            json!({"alpha":"1.0.0"}),
        );
    }
}

#[test]
fn yaml_scaffolding_preserves_trailing_newlines_in_values() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.yaml");
    PackageManifest::init(
        &path,
        super::InitOptions { license: Some("custom\n\n"), ..Default::default() },
    )
    .unwrap();
    assert_eq!(PackageManifest::from_path(path).unwrap().value()["license"], "custom\n\n");
}

#[test]
fn rejects_non_mapping_yaml_without_replacing_it() {
    for source in ["fixture\n", "- name: fixture\n", "42\n"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.yaml");
        fs::write(&path, source).unwrap();
        let error = PackageManifest::from_path(path.clone()).err().unwrap();
        assert!(error.to_string().contains("the manifest root must be an object"));
        assert!(
            error
                .to_string()
                .contains(&path.display().to_string()),
        );
        assert_eq!(fs::read_to_string(path).unwrap(), source);
    }
}
