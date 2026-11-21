use super::{
    Config,
    Path,
    WorkspaceSettings,
    assert_eq,
};

#[test]
fn access_applies_from_yaml_and_reports_back() {
    let mut config = Config::default();
    assert_eq!(config.access, None);
    let settings: WorkspaceSettings = serde_saphyr::from_str("access: restricted\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.access.as_deref(), Some("restricted"));
    assert_eq!(WorkspaceSettings::from_resolved(&config).access.as_deref(), Some("restricted"));
}

/// pnpm assigns a yaml `access` unchecked and lets the registry reject it, so
/// a value outside `public` / `restricted` still reaches [`Config`].
#[test]
fn access_from_yaml_is_kept_verbatim() {
    let mut config = Config::default();
    let settings: WorkspaceSettings = serde_saphyr::from_str("access: restrictd\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.access.as_deref(), Some("restrictd"));
}

#[test]
fn access_can_be_reset() {
    let mut config = Config { access: Some("public".to_owned()), ..Config::default() };
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "access",
        Path::new("/workspace"),
    ));
    assert_eq!(config.access, None);
}

/// `access` is an npm key pnpm's `isConfigFileKey` accepts, so the global
/// `config.yaml` may set it.
#[test]
fn access_survives_in_global_configuration() {
    let mut settings: WorkspaceSettings = serde_saphyr::from_str("access: public\n").unwrap();
    settings.clear_workspace_only_fields();
    assert_eq!(settings.access.as_deref(), Some("public"));
}
