use super::{BTreeMap, Config, Path, WorkspaceSettings, assert_eq};

#[test]
fn tag_defaults_to_latest_and_applies_from_yaml() {
    let mut config = Config::default();
    assert_eq!(config.tag, None);
    assert_eq!(config.default_tag(), "latest");
    let settings: WorkspaceSettings = serde_saphyr::from_str("tag: next\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.default_tag(), "next");
    assert_eq!(WorkspaceSettings::from_resolved(&config).tag.as_deref(), Some("next"));
}

#[test]
fn an_empty_tag_falls_back_to_latest() {
    let mut config = Config::default();
    let settings: WorkspaceSettings = serde_saphyr::from_str("tag: ''\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.default_tag(), "latest");
}

#[test]
fn tag_is_read_from_a_config_override() {
    let values = BTreeMap::from([("tag".to_string(), "beta".to_string())]);
    let mut config = Config::default();
    WorkspaceSettings::from_string_values(&values).apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.default_tag(), "beta");
}

#[test]
fn tag_can_be_reset() {
    let mut config = Config { tag: Some("next".to_string()), ..Config::default() };
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "tag",
        Path::new("/workspace"),
    ));
    assert_eq!(config.tag, None);
}
