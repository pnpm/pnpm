use super::{Config, Path, WorkspaceSettings, assert_eq};

#[test]
fn publish_wait_timeout_defaults_to_disabled_and_supports_configuration_layers() {
    let mut config = Config::default();
    assert_eq!(config.publish_wait_timeout, 0);
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("publishWaitTimeout: 600000\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.publish_wait_timeout, 600000);
    assert_eq!(WorkspaceSettings::from_resolved(&config).publish_wait_timeout, Some(600000));
    let override_settings: WorkspaceSettings =
        serde_saphyr::from_str("publishWaitTimeout: 0\n").unwrap();
    override_settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.publish_wait_timeout, 0);
}

#[test]
fn publish_wait_timeout_rejects_invalid_values() {
    for value in ["-1", "1.5", "true", "soon"] {
        let result =
            serde_saphyr::from_str::<WorkspaceSettings>(&format!("publishWaitTimeout: {value}"));
        assert!(result.is_err(), "must reject {value}: {result:?}");
    }
}

#[test]
fn publish_wait_timeout_can_be_reset() {
    let mut config = Config { publish_wait_timeout: 600000, ..Config::default() };
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "publishWaitTimeout",
        Path::new("/workspace"),
    ));
    assert_eq!(config.publish_wait_timeout, 0);
}
