use super::remove_dependencies;
use std::collections::BTreeSet;

#[test]
fn removes_each_shape_of_declaration_and_preserves_unrelated_entries() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Cargo.toml");
    for source in [
        "[dependencies]\nkept='2'\nold={package='example',version='1'}",
        "[dependencies]\nold.package='example'\nkept='2'\nold.version='1'",
        "[dependencies.old]\npackage='example'\nversion='1'\n[dependencies.kept]\nversion='2'",
        "dependencies={old={package='example',version='1'},kept='2'}",
        "target={cfg={dependencies={old={package='example',version='1'},kept='2'}}}",
    ] {
        std::fs::write(&path, source).unwrap();
        let names = BTreeSet::from(["example".to_owned()]);
        let mut removed = BTreeSet::new();
        remove_dependencies(&path, &names, None, &mut removed).unwrap();
        assert_eq!(removed, names);
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("kept"), "{updated}");
        assert!(!updated.contains("old"), "{updated}");
    }
}

#[test]
fn removing_optional_dependency_prunes_only_its_feature_references() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("Cargo.toml");
    std::fs::write(&path, "[dependencies]\nold = { package = 'example', version = '1', optional = true }\n[features]\nold = []\npublic = ['dep:old', 'old?/derive', 'old/derive', 'old']\n").unwrap();
    remove_dependencies(
        &path,
        &BTreeSet::from(["example".to_string()]),
        None,
        &mut BTreeSet::new(),
    )
    .unwrap();
    let manifest: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        manifest["features"]["public"].as_array().unwrap(),
        &[toml::Value::String("old".into())],
    );
}
