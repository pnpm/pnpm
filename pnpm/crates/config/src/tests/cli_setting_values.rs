use super::{Config, HostNoHome, fs, tempdir};
use crate::esm_node_path_loader::esm_node_path_loader_import_flag;
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

#[test]
fn empty_cli_node_options_override_the_workspace_yaml() {
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join("pnpm-workspace.yaml"), "nodeOptions: --trace-warnings\n")
        .expect("write pnpm-workspace.yaml");

    for key in ["node-options", "nodeOptions"] {
        let config = config_with_cli_settings(&[(key, "")])
            .current::<HostNoHome>(project.path())
            .expect("loads");

        assert_eq!(config.node_options.as_deref(), Some(""), "{key}");
        assert_eq!(config.explicit_settings.get("nodeOptions"), Some(&"".into()), "{key}");
    }
}

#[test]
fn extra_env_only_applies_nonempty_node_options() {
    for (node_options, previous, expected) in [
        (None, Some("--trace-warnings"), Some("--trace-warnings")),
        (Some(""), Some("--trace-warnings"), Some("--trace-warnings")),
        (Some("--no-warnings"), Some("--trace-warnings"), Some("--no-warnings")),
        (Some(" "), Some("--trace-warnings"), Some(" ")),
        (Some(""), None, None),
    ] {
        let mut config =
            Config { node_options: node_options.map(str::to_owned), ..Config::default() };
        config.extra_env.insert("BUILD_MODE".to_string(), "test".to_string());
        if let Some(previous) = previous {
            config.extra_env.insert("NODE_OPTIONS".to_string(), previous.to_string());
        }

        let extra_env = config.extra_env_with_node_options();

        assert_eq!(extra_env.get("NODE_OPTIONS").map(String::as_str), expected, "{node_options:?}");
        assert_eq!(extra_env.get("BUILD_MODE").map(String::as_str), Some("test"));
    }
}

#[test]
fn empty_node_options_preserve_extra_env_with_the_esm_loader() {
    let flag = esm_node_path_loader_import_flag();
    let previous = format!("--trace-warnings {flag}");
    for (node_options, expected) in
        [("", previous.clone()), ("--no-warnings", format!("--no-warnings {flag}"))]
    {
        let mut config =
            Config { node_options: Some(node_options.to_string()), ..Config::default() };
        config.extra_env.insert("NODE_OPTIONS".to_string(), previous.clone());

        assert_eq!(config.extra_env_with_node_options().get("NODE_OPTIONS"), Some(&expected));
    }
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
