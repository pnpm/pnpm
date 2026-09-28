use super::{Config, HostNoHome, fs, tempdir};
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

fn config_with_cli_settings(settings: &[(&str, &str)]) -> Config {
    Config {
        cli_setting_values: settings
            .iter()
            .map(|&(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>(),
        ..Config::default()
    }
}

#[test]
fn cli_setting_values_win_over_the_workspace_yaml() {
    let workspace = tempdir().expect("workspace tempdir");
    fs::write(
        workspace.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nfrozenLockfile: false\nnetworkConcurrency: 2\n",
    )
    .expect("write pnpm-workspace.yaml");
    let project = workspace.path().join("packages/foo");
    fs::create_dir_all(&project).expect("create project dir");

    let config =
        config_with_cli_settings(&[("frozen-lockfile", "true"), ("autoInstallPeers", "false")])
            .current::<HostNoHome>(&project)
            .expect("loads");

    assert_eq!(config.frozen_lockfile, Some(true));
    assert!(!config.auto_install_peers);
    assert_eq!(config.network_concurrency, 2);
    assert_eq!(config.explicit_settings.get("frozenLockfile"), Some(&true.into()));
    assert_eq!(config.workspace_dir.as_deref(), Some(workspace.path()));
}

/// The derivations `Config::current` runs after every layer see a value the
/// command line set, as they see one from a config file.
#[test]
fn derived_settings_follow_cli_setting_values() {
    let project = tempdir().expect("project tempdir");

    let config = config_with_cli_settings(&[("global-bin-dir", "bin")])
        .current::<HostNoHome>(project.path())
        .expect("loads");

    assert_eq!(config.global_bin, Some(project.path().join("bin")));
}

/// The command line is a trusted source, so its proxy also reaches the
/// requests that fetch the package manager itself, as `PNPM_CONFIG_PROXY` does.
#[test]
fn cli_proxy_reaches_the_package_manager_bootstrap() {
    let project = tempdir().expect("project tempdir");

    let config = config_with_cli_settings(&[("proxy", "http://proxy.example.com:8080")])
        .current::<HostNoHome>(project.path())
        .expect("loads");

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://proxy.example.com:8080"));
    assert_eq!(config.package_manager_bootstrap.proxy, config.proxy);
}
