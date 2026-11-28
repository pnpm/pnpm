use super::{
    ConfigLocation,
    TempDir,
    config_set,
    config_with_dir,
    flags,
    json,
    read_yaml,
};

#[test]
fn dotted_set_edits_only_the_named_global_field() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(tmp.path());
    let path = tmp.path().join("config.yaml");
    std::fs::write(
        &path,
        "globalPackagesConfig:\n  trustPolicy: no-downgrade # keep this comment\n",
    )
    .unwrap();
    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "globalPackagesConfig.virtual-store-type".into(),
        Some("project".into()),
    )
    .unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("# keep this comment"));
    assert_eq!(
        read_yaml(&path).unwrap(),
        json!({"globalPackagesConfig":{"trustPolicy":"no-downgrade","virtualStoreType":"project"}}),
    );
    config_set(
        &config,
        tmp.path(),
        flags(true, None, true),
        "globalPackagesConfig".into(),
        Some(r#"{"virtualStoreType":"global"}"#.into()),
    )
    .unwrap();
    assert_eq!(
        read_yaml(&path).unwrap(),
        json!({"globalPackagesConfig":{"virtualStoreType":"global"}}),
    );
}

#[test]
fn dotted_set_creates_a_project_mapping() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(tmp.path());
    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "catalog.example".into(),
        Some("1.0.0".into()),
    )
    .unwrap();
    assert_eq!(
        read_yaml(&tmp.path().join("pnpm-workspace.yaml")).unwrap(),
        json!({"catalog":{"example":"1.0.0"}}),
    );
}

#[test]
fn dotted_set_updates_the_file_that_owns_the_catalog() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("pnpm-workspace.yaml");
    let include = tmp.path().join("settings/catalog.yaml");
    std::fs::create_dir(include.parent().unwrap()).unwrap();
    let index = "include:\n  ./settings/catalog.yaml: [catalogs.tools]\n";
    std::fs::write(&root, index).unwrap();
    std::fs::write(&include, "catalogs:\n  tools:\n    existing: '2'\n").unwrap();
    config_set(
        &config_with_dir(tmp.path()),
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "catalogs.tools.example".into(),
        Some("link:./local".into()),
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(root).unwrap(), index);
    assert_eq!(
        read_yaml(&include).unwrap(),
        json!({"catalogs":{"tools":{"example":"link:../local","existing":"2"}}}),
    );
}

#[test]
fn dotted_set_refuses_to_turn_a_scalar_setting_into_a_mapping() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(tmp.path());
    assert!(
        config_set(
            &config,
            tmp.path(),
            flags(true, None, false),
            "storeDir.example".into(),
            Some("value".into())
        )
        .is_err(),
    );
    assert!(!tmp.path().join("config.yaml").exists());
}

#[test]
fn dotted_set_keeps_delegated_settings_with_their_owner() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("pnpm-workspace.yaml");
    let include = tmp.path().join("catalog.yaml");
    let index = "include:\n  ./catalog.yaml: [catalog]\n";
    std::fs::write(&root, index).unwrap();
    let original = "catalog:\n  entries: {}\n";
    std::fs::write(&include, original).unwrap();
    let result = config_set(
        &config_with_dir(tmp.path()),
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "catalog.mode".into(),
        Some("strict".into()),
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("catalogMode"),
    );
    assert_eq!(std::fs::read_to_string(include).unwrap(), original);
}

#[test]
fn dotted_set_refuses_a_second_spelling_of_the_default_catalog() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pnpm-workspace.yaml");
    let config = config_with_dir(tmp.path());
    for (original, key) in [
        ("catalog: {example: '1'}\n", "catalogs.default.other"),
        ("catalogs: {default: {example: '1'}}\n", "catalog.other"),
    ] {
        std::fs::write(&path, original).unwrap();
        let err = config_set(
            &config,
            tmp.path(),
            flags(false, Some(ConfigLocation::Project), false),
            key.into(),
            Some("1".into()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("default catalog"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}

#[test]
fn dotted_set_uses_the_field_type_for_plain_numbers() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(tmp.path());
    let project = flags(false, Some(ConfigLocation::Project), false);
    for (key, raw) in [("tasks.compile.priority", "1"), ("catalog.priority", "1")] {
        config_set(&config, tmp.path(), project, key.into(), Some(raw.into())).unwrap();
    }
    let yaml = read_yaml(&tmp.path().join("pnpm-workspace.yaml")).unwrap();
    assert_eq!(yaml["tasks"]["compile"]["priority"], json!(1));
    assert_eq!(yaml["catalog"]["priority"], json!("1"));
}
