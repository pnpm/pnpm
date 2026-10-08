use super::{Config, Path, WorkspaceSettings, assert_eq};

#[test]
fn provenance_is_unset_by_default_and_applies_from_yaml() {
    let mut config = Config::default();
    assert_eq!(config.provenance, None);
    let settings: WorkspaceSettings = serde_saphyr::from_str("provenance: false\n").unwrap();
    settings.apply_to(&mut config, Path::new("/workspace"));
    assert_eq!(config.provenance, Some(false));
    assert_eq!(WorkspaceSettings::from_resolved(&config).provenance, Some(false));
}

#[test]
fn provenance_can_be_reset() {
    let mut config = Config { provenance: Some(false), ..Config::default() };
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "provenance",
        Path::new("/workspace"),
    ));
    assert_eq!(config.provenance, None);
}
