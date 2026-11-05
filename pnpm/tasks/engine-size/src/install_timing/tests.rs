use super::benchmark_env;
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::Path};

#[test]
fn benchmark_env_removes_inherited_settings_but_keeps_credentials() {
    let project = Path::new("/bench/project");
    let inherited = [
        "PNPM_CONFIG_MODULES_DIR",
        "pnpm_config_lockfile_dir",
        "npm_config_store_dir",
        "PNPM_CONFIG_REGISTRY",
        "npm_config_//registry.example/:_authToken",
        "NPM_CONFIG__AUTH",
        "NPM_CONFIG_USERCONFIG",
        "PATH",
    ]
    .map(OsString::from);
    let env = benchmark_env(project, "https://registry.example/", inherited);
    let expected: Vec<(OsString, Option<OsString>)> = vec![
        ("PNPM_CONFIG_MODULES_DIR".into(), None),
        ("pnpm_config_lockfile_dir".into(), None),
        ("npm_config_store_dir".into(), None),
        ("PNPM_CONFIG_REGISTRY".into(), None),
        ("PNPM_CONFIG_REGISTRY".into(), Some("https://registry.example/".into())),
        ("PNPM_CONFIG_STORE_DIR".into(), Some(project.join("store-dir").into())),
        ("PNPM_CONFIG_CACHE_DIR".into(), Some(project.join("cache-dir").into())),
        ("PNPM_CONFIG_IGNORE_SCRIPTS".into(), Some("true".into())),
    ];
    assert_eq!(env, expected);
}
