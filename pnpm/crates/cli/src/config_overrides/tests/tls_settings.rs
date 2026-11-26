use super::{
    Config,
    ConfigOverrides,
    argv,
    assert_eq,
};
use crate::no_env_host::NoEnv;

#[test]
fn dotted_tls_settings_override_the_workspace_yaml() {
    let project = tempfile::tempdir().expect("project tempdir");
    std::fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "strictSsl: true\nlocalAddress: 127.0.0.3\n",
    )
    .expect("write pnpm-workspace.yaml");
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.strict-ssl=false",
        "--config.localAddress=127.0.0.2",
        "install",
    ]));

    let config = Config { cli: overrides.cli_layer(None), ..Config::default() }
        .current::<NoEnv>(project.path())
        .expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(false));
    assert_eq!(config.tls.local_address, Some([127, 0, 0, 2].into()));
    assert_eq!(config.package_manager_bootstrap.tls.strict_ssl, Some(false));
    assert_eq!(config.package_manager_bootstrap.tls.local_address, Some([127, 0, 0, 2].into()));
}

#[test]
fn bare_tls_flags_override_the_workspace_yaml() {
    let project = tempfile::tempdir().expect("project tempdir");
    std::fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "strictSsl: false\nlocalAddress: 127.0.0.3\n",
    )
    .expect("write pnpm-workspace.yaml");
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--strict-ssl",
        "--local-address",
        "127.0.0.2",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let config = Config { cli: overrides.cli_layer(None), ..Config::default() }
        .current::<NoEnv>(project.path())
        .expect("load config");

    assert_eq!(config.tls.strict_ssl, Some(true));
    assert_eq!(config.tls.local_address, Some([127, 0, 0, 2].into()));
}
