use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, HostNoHome, LinkProbe, OsString, Path,
    PathBuf, assert_eq, fs, io, tempdir, write_file,
};
use std::{collections::BTreeMap, net::IpAddr};

const LOCAL_ADDRESS: &str = "127.0.0.2";

fn local_address(raw: &str) -> Option<IpAddr> {
    Some(raw.parse().expect("parse IP address"))
}

#[test]
fn workspace_yaml_supplies_strict_ssl_and_local_address() {
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        &format!("strictSsl: false\nlocalAddress: {LOCAL_ADDRESS}\n"),
    );

    let config = Config::new().current::<HostNoHome>(project.path()).expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
}

#[test]
fn workspace_yaml_tls_settings_override_project_npmrc() {
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "strict-ssl=true\nlocal-address=127.0.0.3\n");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        &format!("strictSsl: false\nlocalAddress: {LOCAL_ADDRESS}\n"),
    );

    let config = Config::new().current::<HostNoHome>(project.path()).expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
}

#[test]
fn project_npmrc_tls_settings_apply_without_yaml_ones() {
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "strict-ssl=false\nlocal-address=127.0.0.3\n");
    write_file(&project.path().join("pnpm-workspace.yaml"), "networkConcurrency: 2\n");

    let config = Config::new().current::<HostNoHome>(project.path()).expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address("127.0.0.3"));
}

#[test]
fn global_config_yaml_supplies_tls_settings_below_the_workspace_yaml() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join("pnpm-workspace.yaml"), "strictSsl: true\n");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    write_file(
        &config_dir.join("config.yaml"),
        &format!("strictSsl: false\nlocalAddress: {LOCAL_ADDRESS}\n"),
    );

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.tls.strict_ssl, Some(true));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
}

#[test]
fn global_config_yaml_tls_settings_override_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "strict-ssl=true\nlocal-address=127.0.0.3\n");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    write_file(
        &config_dir.join("config.yaml"),
        &format!("strictSsl: false\nlocalAddress: {LOCAL_ADDRESS}\n"),
    );

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
}

#[test]
fn pnpm_config_env_tls_settings_override_the_workspace_yaml() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "strictSsl: true\nlocalAddress: 127.0.0.3\n",
    );

    set_fake_env(&[
        ("PNPM_CONFIG_STRICT_SSL", "false"),
        ("pnpm_config_local_address", LOCAL_ADDRESS),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
}

#[test]
fn cli_tls_settings_override_the_environment_and_the_workspace_yaml() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "strictSsl: true\nlocalAddress: 127.0.0.3\n",
    );
    set_fake_env(&[("PNPM_CONFIG_STRICT_SSL", "true")]);
    let cli_settings = [("strict-ssl", "false"), ("local-address", LOCAL_ADDRESS)];

    let config = Config {
        cli: crate::CliLayer {
            npmrc_tokens: cli_settings
                .iter()
                .map(|&(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            setting_values: cli_settings
                .iter()
                .map(|&(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..crate::CliLayer::default()
        },
        ..Config::default()
    }
    .current::<FakeEnv>(project.path())
    .expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
}

/// The bootstrap reads the sources outside the repository: the trusted
/// `.npmrc` files, the global `config.yaml`, the environment, and the
/// command line. The project's `pnpm-workspace.yaml` stays out of it.
#[test]
fn global_config_yaml_tls_settings_reach_the_bootstrap_over_the_user_npmrc() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "strictSsl: true\nlocalAddress: 127.0.0.4\n",
    );
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "strict-ssl=true\nlocal-address=127.0.0.3\n");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    write_file(
        &config_dir.join("config.yaml"),
        &format!("strictSsl: false\nlocalAddress: {LOCAL_ADDRESS}\n"),
    );

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = Config { npmrc_auth_file: Some(user_file), ..Config::default() }
        .current::<FakeEnv>(project.path())
        .expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(true));
    assert_eq!(config.tls.local_address, local_address("127.0.0.4"));
    assert_eq!(config.package_manager_bootstrap.tls.strict_ssl, Some(false));
    assert_eq!(config.package_manager_bootstrap.tls.local_address, local_address(LOCAL_ADDRESS));
}

#[test]
fn workspace_yaml_tls_settings_do_not_reach_the_bootstrap() {
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        &format!("strictSsl: false\nlocalAddress: {LOCAL_ADDRESS}\n"),
    );

    let config = Config::new().current::<HostNoHome>(project.path()).expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.package_manager_bootstrap.tls.strict_ssl, None);
    assert_eq!(config.package_manager_bootstrap.tls.local_address, None);
}

#[test]
fn pnpm_config_env_tls_settings_reach_the_bootstrap_over_the_global_config_yaml() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    write_file(&config_dir.join("config.yaml"), "strictSsl: true\nlocalAddress: 127.0.0.3\n");

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("PNPM_CONFIG_STRICT_SSL", "false"),
        ("PNPM_CONFIG_LOCAL_ADDRESS", LOCAL_ADDRESS),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.package_manager_bootstrap.tls.strict_ssl, Some(false));
    assert_eq!(config.package_manager_bootstrap.tls.local_address, local_address(LOCAL_ADDRESS));
}

/// The command line outranks the global `config.yaml` in the bootstrap as
/// it does in the project config, in both spellings of a setting.
#[test]
fn cli_tls_settings_reach_the_bootstrap_over_the_global_config_yaml_and_the_environment() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    write_file(&config_dir.join("config.yaml"), "strictSsl: true\n");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("PNPM_CONFIG_LOCAL_ADDRESS", "127.0.0.3"),
    ]);

    // `--config.strict-ssl=false --config.localAddress=...`, as the CLI
    // seeds them: the tokens as given, the setting values in kebab-case.
    let config = Config {
        cli: crate::CliLayer {
            npmrc_tokens: vec![
                ("strict-ssl".to_string(), "false".to_string()),
                ("localAddress".to_string(), LOCAL_ADDRESS.to_string()),
            ],
            setting_values: [("strict-ssl", "false"), ("local-address", LOCAL_ADDRESS)]
                .iter()
                .map(|&(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..crate::CliLayer::default()
        },
        ..Config::default()
    }
    .current::<FakeEnv>(project.path())
    .expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, local_address(LOCAL_ADDRESS));
    assert_eq!(config.package_manager_bootstrap.tls.strict_ssl, Some(false));
    assert_eq!(config.package_manager_bootstrap.tls.local_address, local_address(LOCAL_ADDRESS));
}
