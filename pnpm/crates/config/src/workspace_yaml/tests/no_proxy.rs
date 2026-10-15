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
        assert!(
            matches!(error, LoadWorkspaceYamlError::InvalidNoProxy { received_type } if received_type == expected_type)
        );
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
