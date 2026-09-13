use super::{Config, Path, WorkspaceSettings, assert_eq};

/// `verifyStoreIntegrity` is a camelCase key that serde's rename
/// has to pick up, and the `apply_to` wiring has to thread it onto
/// the `Config` field. Parse a yaml that flips the default-true
/// setting to false and assert both steps. Guards against silent
/// regressions in the key mapping or the apply step (a copy-paste
/// omission in `apply_to` would leave `config.verify_store_integrity`
/// at its default).
#[test]
fn parses_verify_store_integrity_from_yaml_and_applies() {
    let yaml = "verifyStoreIntegrity: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.verify_store_integrity, Some(false));

    let mut config = Config::new();
    assert!(config.verify_store_integrity, "the default is `true` to match pnpm");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.verify_store_integrity, "yaml override wins");
}
