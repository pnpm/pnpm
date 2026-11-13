use super::{
    Config, ConfigLocation, TempDir, Value, config_get, config_list, config_set, config_with_dir,
    flags, json, load_isolated_project_config, read_yaml,
};
use pnpm_config::TrustedSettings;
use std::path::Path;

fn load_global_only_config(config_dir: &Path) -> Config {
    let mut config = config_with_dir(config_dir);
    if let Some(trusted) = TrustedSettings::load_global(config_dir).expect("load global config") {
        config.record_explicit_settings(&trusted);
        trusted.apply_to(&mut config, config_dir);
    }
    config
}

#[test]
fn set_get_and_delete_global_tools_round_trip() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    let tools = json!({
        "node": {
            "mirror": "https://mirror.example.com/node/download",
            "channels": {
                "nightly": "https://nightly.example.com/"
            }
        },
        "python": {
            "mirror": "https://mirror.example.com/python"
        }
    });
    config_set(&config, tmp.path(), flags(true, None, true), "tools", Some(tools.to_string()))
        .unwrap();

    assert_eq!(read_yaml(&config_dir.join("config.yaml")).unwrap(), json!({ "tools": tools }));

    let loaded = load_global_only_config(&config_dir);
    let got: Value =
        serde_json::from_str(&config_get(&loaded, flags(true, None, false), "tools").unwrap())
            .unwrap();
    assert_eq!(got["node"]["mirror"], json!("https://mirror.example.com/node/download"));
    assert_eq!(got["node"]["channels"]["nightly"], json!("https://nightly.example.com/"));
    assert_eq!(got["python"]["mirror"], json!("https://mirror.example.com/python"));
    assert_eq!(
        config_get(&loaded, flags(true, None, false), "tools.node.mirror").unwrap(),
        "https://mirror.example.com/node/download",
    );

    let listed: Value = serde_json::from_str(&config_list(&loaded)).unwrap();
    assert_eq!(listed["tools"]["node"]["mirror"], got["node"]["mirror"]);

    config_set(&config, tmp.path(), flags(true, None, false), "tools", None).unwrap();
    assert!(!config_dir.join("config.yaml").exists());
}

#[test]
fn set_global_tools_rejects_invalid_values_without_writing_config_yaml() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    let scalar_err = config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "tools",
        Some("https://mirror.example.com".into()),
    )
    .unwrap_err();
    assert_eq!(scalar_err.code().unwrap().to_string(), "ERR_PNPM_CONFIG_SET_STRUCTURED_VALUE");
    assert!(!config_dir.join("config.yaml").exists());

    for invalid_json in [
        r#"{"node":"22.0.0"}"#,
        r#"{"deno":{"mirror":"https://mirror.example.com"}}"#,
        r#"{"python":{"channels":{"rc":"https://mirror.example.com"}}}"#,
        r#"{"node":{"unknownField":true}}"#,
    ] {
        let err = config_set(
            &config,
            tmp.path(),
            flags(true, None, true),
            "tools",
            Some(invalid_json.into()),
        )
        .unwrap_err();
        assert_eq!(
            err.code().unwrap().to_string(),
            "ERR_PNPM_CONFIG_SET_STRUCTURED_VALUE",
            "{invalid_json}",
        );
        assert!(!config_dir.join("config.yaml").exists(), "{invalid_json}");
    }
}

#[test]
fn set_project_tools_refuses_and_points_to_global_config_set() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    let err = config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), true),
        "tools",
        Some(r#"{"node":{"mirror":"https://mirror.example.com/node/download"}}"#.into()),
    )
    .unwrap_err();

    assert_eq!(err.code().unwrap().to_string(), "ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING");
    let help = err
        .help()
        .expect("refused-key help")
        .to_string();
    assert_eq!(help, "Set it for the machine instead: pnpm config set --global tools");
    assert!(
        !tmp.path()
            .join("pnpm-workspace.yaml")
            .exists(),
    );
}

#[test]
fn global_block_exotic_subdeps_and_exclude_links_from_lockfile_round_trip() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "blockExoticSubdeps",
        Some("true".into()),
    )
    .unwrap();
    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "excludeLinksFromLockfile",
        Some("true".into()),
    )
    .unwrap();

    let loaded_global = load_global_only_config(&config_dir);
    assert_eq!(
        config_get(&loaded_global, flags(true, None, false), "blockExoticSubdeps").unwrap(),
        "true",
    );
    assert_eq!(
        config_get(&loaded_global, flags(true, None, false), "exclude-links-from-lockfile")
            .unwrap(),
        "true",
    );

    let project_runtime = load_isolated_project_config(config, tmp.path());
    assert!(project_runtime.block_exotic_subdeps);
    assert!(project_runtime.exclude_links_from_lockfile);
}

#[test]
fn global_pm_on_fail_and_runtime_on_fail_round_trip() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(&config, tmp.path(), flags(true, None, false), "pm-on-fail", Some("ignore".into()))
        .unwrap();
    config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "runtime-on-fail",
        Some("error".into()),
    )
    .unwrap();

    let loaded_global = load_global_only_config(&config_dir);
    assert_eq!(
        config_get(&loaded_global, flags(true, None, false), "pm-on-fail").unwrap(),
        "ignore",
    );
    assert_eq!(
        config_get(&loaded_global, flags(true, None, false), "runtimeOnFail").unwrap(),
        "error",
    );

    let project_runtime = load_isolated_project_config(config, tmp.path());
    assert_eq!(project_runtime.pm_on_fail, Some(pnpm_config::PmOnFail::Ignore));
    assert_eq!(project_runtime.runtime_on_fail, Some(pnpm_config::RuntimeOnFail::Error));
}

#[test]
fn max_sockets_coerces_to_number_and_round_trips_in_both_spellings() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    let config = config_with_dir(&config_dir);

    config_set(&config, tmp.path(), flags(true, None, false), "maxSockets", Some("5".into()))
        .unwrap();
    assert_eq!(read_yaml(&config_dir.join("config.yaml")).unwrap(), json!({ "maxSockets": 5 }));

    let loaded_global = load_global_only_config(&config_dir);
    assert_eq!(loaded_global.max_sockets, Some(5));
    assert_eq!(config_get(&loaded_global, flags(true, None, false), "maxSockets").unwrap(), "5");
    assert_eq!(config_get(&loaded_global, flags(true, None, false), "max-sockets").unwrap(), "5");
    assert_eq!(config_get(&loaded_global, flags(true, None, false), "maxsockets").unwrap(), "5");

    config_set(
        &config,
        tmp.path(),
        flags(false, Some(ConfigLocation::Project), false),
        "maxSockets",
        Some("8".into()),
    )
    .unwrap();
    assert_eq!(
        read_yaml(&tmp.path().join("pnpm-workspace.yaml")).unwrap(),
        json!({ "maxSockets": 8 }),
    );
}

#[test]
fn delete_global_clears_unsupported_and_refused_keys_without_failing() {
    let tmp = TempDir::new().unwrap();
    let config_dir = tmp.path().join("global-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.yaml"),
        "nodeLinker: hoisted\nauto-install-peers: false\nconfigDir: /tmp/x\nstoreDir: ~/store\n",
    )
    .unwrap();
    let config = config_with_dir(&config_dir);

    for key in ["node-linker", "autoInstallPeers", "config-dir"] {
        config_set(&config, tmp.path(), flags(true, None, false), key, None).unwrap();
    }

    assert_eq!(read_yaml(&config_dir.join("config.yaml")), Some(json!({ "storeDir": "~/store" })));
}

#[test]
fn set_global_unsupported_key_gives_accurate_hint_for_refused_vs_workspace_keys() {
    let tmp = TempDir::new().unwrap();
    let config = config_with_dir(&tmp.path().join("global-config"));

    let dir_err =
        config_set(&config, tmp.path(), flags(true, None, false), "dir", Some("/tmp/x".into()))
            .unwrap_err();
    assert_eq!(
        dir_err.code().unwrap().to_string(),
        "ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY",
    );
    assert_eq!(dir_err.help().unwrap().to_string(), "Pass --dir on the command line instead");

    let cfg_dir_err = config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "config-dir",
        Some("/tmp/x".into()),
    )
    .unwrap_err();
    assert_eq!(cfg_dir_err.help().unwrap().to_string(), "This is not a pnpm setting");

    let ws_err = config_set(
        &config,
        tmp.path(),
        flags(true, None, false),
        "node-linker",
        Some("hoisted".into()),
    )
    .unwrap_err();
    assert_eq!(
        ws_err.help().unwrap().to_string(),
        "Try setting them instead to the local pnpm-workspace.yaml file",
    );
}
