use super::{Config, Path, SideEffectsCacheSetting, WorkspaceSettings, assert_eq};
use crate::api::EnvVar;

fn exclude_of(setting: Option<&SideEffectsCacheSetting>) -> Option<&Vec<String>> {
    match setting {
        Some(SideEffectsCacheSetting::Settings(settings)) => settings.exclude.as_ref(),
        _ => None,
    }
}

#[test]
fn side_effects_cache_exclude_is_read_resolved_and_reset() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("sideEffectsCache:\n  exclude:\n    - java\n    - '@native/*'\n")
            .unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/workspace"));
    let expected = Some(vec!["java".to_string(), "@native/*".to_string()]);
    assert_eq!(config.side_effects_cache_exclude, expected);
    assert!(config.side_effects_cache_read() && config.side_effects_cache_write());
    let resolved = WorkspaceSettings::from_resolved(&config);
    assert_eq!(exclude_of(resolved.side_effects_cache.as_ref()), expected.as_ref());

    assert!(WorkspaceSettings::reset_setting_to_default::<crate::Host>(
        &mut config,
        &Config::default(),
        "sideEffectsCache",
        Path::new("/workspace"),
    ));
    assert_eq!(config.side_effects_cache_exclude, None);
}

#[test]
fn side_effects_cache_exclude_reads_from_environment() {
    struct Env;
    impl EnvVar for Env {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SIDE_EFFECTS_CACHE").then(|| r#"{"exclude":["java"]}"#.to_owned())
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<Env>();
    assert_eq!(exclude_of(settings.side_effects_cache.as_ref()), Some(&vec!["java".to_string()]));
}
