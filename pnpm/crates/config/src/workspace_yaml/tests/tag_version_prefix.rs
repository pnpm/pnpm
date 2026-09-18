use super::{Config, Path, WorkspaceSettings, assert_eq};

#[test]
fn defaults_to_v() {
    assert_eq!(Config::default().tag_version_prefix, "v");
    assert_eq!(WorkspaceSettings::default().tag_version_prefix, None);
}

#[test]
fn parses_tag_version_prefix_from_yaml_and_applies() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("tagVersionPrefix: release-\n").unwrap();
    assert_eq!(settings.tag_version_prefix.as_deref(), Some("release-"));

    let mut config = Config::default();
    assert_eq!(config.tag_version_prefix, "v", "default is v");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.tag_version_prefix, "release-");
}

#[test]
fn empty_tag_version_prefix_removes_the_default() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("tagVersionPrefix: ''\n").unwrap();
    assert_eq!(settings.tag_version_prefix.as_deref(), Some(""));

    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.tag_version_prefix, "");
}

#[test]
fn omitting_tag_version_prefix_keeps_default() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("storeDir: /s\n").unwrap();
    assert_eq!(settings.tag_version_prefix, None);

    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.tag_version_prefix, "v");
}

#[test]
fn later_layer_overrides_tag_version_prefix() {
    let mut config = Config::default();
    let earlier: WorkspaceSettings = serde_saphyr::from_str("tagVersionPrefix: a-\n").unwrap();
    earlier.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.tag_version_prefix, "a-");

    let later: WorkspaceSettings = serde_saphyr::from_str("tagVersionPrefix: b-\n").unwrap();
    later.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.tag_version_prefix, "b-");
}

#[test]
fn from_resolved_reports_tag_version_prefix() {
    let config = Config { tag_version_prefix: "release-".to_string(), ..Config::default() };
    let projected = WorkspaceSettings::from_resolved(&config);
    assert_eq!(projected.tag_version_prefix.as_deref(), Some("release-"));
}

#[test]
fn reset_tag_version_prefix_restores_the_default() {
    let defaults = Config::default();
    let mut config = Config { tag_version_prefix: "release-".to_string(), ..Config::default() };
    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &defaults,
        "tagVersionPrefix",
        Path::new("/tmp/project"),
    ));
    assert_eq!(config.tag_version_prefix, "v");
}
