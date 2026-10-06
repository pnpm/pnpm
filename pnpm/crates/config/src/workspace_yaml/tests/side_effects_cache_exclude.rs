use super::{Config, Path, WorkspaceSettings, assert_eq};
use crate::api::EnvVar;

#[test]
fn side_effects_cache_exclude_is_read_resolved_and_reset() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("sideEffectsCacheExclude:\n  - java\n  - '@native/*'\n").unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));
    let expected = Some(vec!["java".to_string(), "@native/*".to_string()]);
    assert_eq!(config.side_effects_cache_exclude, expected);
    assert_eq!(WorkspaceSettings::from_resolved(&config).side_effects_cache_exclude, expected);

    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "sideEffectsCacheExclude",
        Path::new("/workspace"),
    ));
    assert_eq!(config.side_effects_cache_exclude, None);
}

#[test]
fn side_effects_cache_exclude_reads_from_environment() {
    struct Env;
    impl EnvVar for Env {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SIDE_EFFECTS_CACHE_EXCLUDE").then(|| r#"["java"]"#.to_owned())
        }
    }
    assert_eq!(
        WorkspaceSettings::from_pnpm_config_env::<Env>().side_effects_cache_exclude,
        Some(vec!["java".to_string()]),
    );
}
