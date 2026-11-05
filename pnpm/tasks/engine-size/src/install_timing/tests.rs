use super::benchmark_env;
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::Path};

#[test]
fn benchmark_env_replaces_both_spellings_of_each_owned_setting() {
    let project = Path::new("/bench/project");
    let env = benchmark_env(project, "https://registry.example/");
    let expected: Vec<(String, Option<OsString>)> = vec![
        ("pnpm_config_registry".into(), None),
        ("pnpm_config_store_dir".into(), None),
        ("pnpm_config_cache_dir".into(), None),
        ("pnpm_config_ignore_scripts".into(), None),
        ("PNPM_CONFIG_REGISTRY".into(), Some("https://registry.example/".into())),
        ("PNPM_CONFIG_STORE_DIR".into(), Some(project.join("store-dir").into())),
        ("PNPM_CONFIG_CACHE_DIR".into(), Some(project.join("cache-dir").into())),
        ("PNPM_CONFIG_IGNORE_SCRIPTS".into(), Some("true".into())),
    ];
    assert_eq!(env, expected);
}
