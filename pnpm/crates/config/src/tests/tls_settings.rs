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
