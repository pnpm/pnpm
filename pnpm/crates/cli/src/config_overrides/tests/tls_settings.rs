use super::{
    Config, ConfigOverrides, EnvVar, GetCurrentDir, GetHomeDir, LinkProbe, Path, PathBuf, argv,
    assert_eq,
};
use pnpm_config::EnvVarOs;
use std::ffi::OsString;

/// No environment and no home directory, so only the fixture's files and
/// the command line reach the config.
struct NoEnv;

impl EnvVar for NoEnv {
    fn var(_: &str) -> Option<String> {
        None
    }
}

impl EnvVarOs for NoEnv {
    fn var_os(_: &str) -> Option<OsString> {
        None
    }
}

impl GetHomeDir for NoEnv {
    fn home_dir() -> Option<PathBuf> {
        None
    }
}

impl GetCurrentDir for NoEnv {
    fn current_dir() -> std::io::Result<PathBuf> {
        std::env::current_dir()
    }
}

impl LinkProbe for NoEnv {
    fn can_link_between_dirs(_: &Path, _: &Path) -> bool {
        true
    }
}

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
}
