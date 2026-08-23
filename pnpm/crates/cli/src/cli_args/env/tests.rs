use super::{EnvArgs, EnvError, EnvSubcommand};
use pnpm_config::Config;
use pnpm_reporter::SilentReporter;
use std::path::PathBuf;

/// A config that looks like a standalone-script pnpm install: it has a
/// global bin directory to link a runtime into.
fn config_with_global_bin() -> Config {
    Config { global_bin: Some(PathBuf::from("/home/user/.local/share/pnpm")), ..Config::default() }
}

fn args(global: bool, params: &[&str]) -> EnvArgs {
    EnvArgs { global, remote: false, params: params.iter().map(ToString::to_string).collect() }
}

#[test]
fn a_bare_env_asks_for_a_subcommand() {
    let error =
        args(false, &[]).subcommand::<SilentReporter>(&config_with_global_bin()).unwrap_err();
    assert!(matches!(error, EnvError::NoSubcommand), "{error:?}");
}

#[test]
fn an_unrecognized_subcommand_is_rejected() {
    let error = args(false, &["install"])
        .subcommand::<SilentReporter>(&config_with_global_bin())
        .unwrap_err();
    assert!(matches!(error, EnvError::UnknownSubcommand), "{error:?}");
}

#[test]
fn managing_node_needs_a_global_bin_dir() {
    let error =
        args(true, &["use", "24"]).subcommand::<SilentReporter>(&Config::default()).unwrap_err();
    assert!(matches!(error, EnvError::CannotManageNode), "{error:?}");
}

#[test]
fn use_installs_the_version_as_a_global_runtime() {
    let subcommand =
        args(true, &["use", "24"]).subcommand::<SilentReporter>(&config_with_global_bin()).unwrap();
    let EnvSubcommand::Use { package_name } = subcommand else {
        panic!("expected a `use` subcommand");
    };
    assert_eq!(package_name, "node@runtime:24");
}

#[test]
fn use_is_global_only() {
    let error = args(false, &["use", "24"])
        .subcommand::<SilentReporter>(&config_with_global_bin())
        .unwrap_err();
    assert!(matches!(error, EnvError::LocalUseUnsupported), "{error:?}");
}

#[test]
fn use_requires_a_version() {
    let error = args(true, &["use", "  "])
        .subcommand::<SilentReporter>(&config_with_global_bin())
        .unwrap_err();
    assert!(matches!(error, EnvError::MissingNodeVersion), "{error:?}");
}

#[test]
fn list_takes_an_optional_selector() {
    for (params, expected) in [
        (vec!["list"], None),
        (vec!["ls"], None),
        (vec!["list", "  "], None),
        (vec!["list", "lts"], Some("lts".to_string())),
        (vec!["ls", "rc/24"], Some("rc/24".to_string())),
    ] {
        let subcommand =
            args(false, &params).subcommand::<SilentReporter>(&config_with_global_bin()).unwrap();
        let EnvSubcommand::List { version_spec } = subcommand else {
            panic!("expected a `list` subcommand for {params:?}");
        };
        assert_eq!(version_spec, expected, "{params:?}");
    }
}
