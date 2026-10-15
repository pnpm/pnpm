use super::{LoadWorkspaceYamlError, WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, fs};

#[test]
fn rejects_invalid_no_proxy_types() {
    for (value, expected_type) in
        [("123", "number"), ("[]", "array"), ("{host: localhost}", "object")]
    {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(WORKSPACE_MANIFEST_FILENAME),
            format!("noProxy: {value}\n"),
        )
        .unwrap();
        let error = WorkspaceSettings::load_at(dir.path()).unwrap_err();
        dbg!(&error);
        let expected = format!("The \"noProxy\" setting should be a boolean or string, but got {expected_type}");
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn accepts_no_proxy_strings_booleans_and_null() {
    for value in ["true", "false", "''", "'null'", "'localhost,127.0.0.1'", "null"] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(WORKSPACE_MANIFEST_FILENAME),
            format!("noProxy: {value}\n"),
        )
        .unwrap();
        let settings = WorkspaceSettings::load_at(dir.path()).unwrap().unwrap();
        let expected = serde_saphyr::from_str::<Option<serde_json::Value>>(value).unwrap();
        assert_eq!(settings.no_proxy, expected);
    }
}

#[test]
fn skips_invalid_no_proxy_without_dropping_other_settings() {
    let mut settings: WorkspaceSettings =
        serde_saphyr::from_str("noProxy: [localhost]\nregistry: https://registry.example/\n").unwrap();
    settings.validate_sections(true).unwrap();
    assert_eq!(settings.no_proxy, None);
    assert_eq!(settings.registry.as_deref(), Some("https://registry.example/"));
}

#[test]
fn global_no_proxy_is_validated_and_can_be_skipped() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(crate::workspace_yaml::GLOBAL_CONFIG_YAML_FILENAME),
        "noProxy: [localhost]\nregistry: https://registry.example/\n",
    )
    .unwrap();
    let error = WorkspaceSettings::load_global(dir.path()).unwrap_err();
    dbg!(&error);
    assert!(matches!(error, LoadWorkspaceYamlError::InvalidNoProxy { received_type: "array" }));
    let settings = WorkspaceSettings::load_global_skipping_unreadable(dir.path(), true)
        .unwrap()
        .unwrap();
    assert_eq!(settings.no_proxy, None);
    assert_eq!(settings.registry.as_deref(), Some("https://registry.example/"));
}
