use super::{Config, Path, WorkspaceSettings, assert_eq};

#[test]
fn publish_branch_supports_workspace_configuration_layers() {
    let mut config = Config::default();
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("publishBranch: production\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.publish_branch.as_deref(), Some("production"));
    assert_eq!(
        WorkspaceSettings::from_resolved(&config).publish_branch.as_deref(),
        Some("production"),
    );

    let override_settings: WorkspaceSettings =
        serde_saphyr::from_str("publishBranch: release\n").unwrap();
    override_settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.publish_branch.as_deref(), Some("release"));
}

#[test]
fn publish_branch_can_be_reset() {
    let mut config = Config { publish_branch: Some("production".to_owned()), ..Config::default() };
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "publishBranch",
        Path::new("/workspace"),
    ));
    assert_eq!(config.publish_branch, None);
}

#[test]
fn publish_branch_is_ignored_in_global_configuration() {
    let mut settings: WorkspaceSettings =
        serde_saphyr::from_str("publishBranch: production\n").unwrap();
    settings.clear_workspace_only_fields();
    assert_eq!(settings.publish_branch, None);
}
