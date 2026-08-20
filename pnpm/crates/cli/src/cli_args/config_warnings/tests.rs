use super::{take_unemitted, unmatched_registry_options_warning};
use pnpm_config::Config;
use pnpm_lockfile::{RegistryOptions, RegistryServerType};
use pretty_assertions::assert_eq;
use std::collections::HashSet;

fn config_with(registries: &[(&str, &str)], registry_options_by_url: &[&str]) -> Config {
    let mut config = Config::new();
    config.registries_by_scope =
        registries.iter().map(|(scope, url)| ((*scope).to_string(), (*url).to_string())).collect();
    config.registry_options_by_url = registry_options_by_url
        .iter()
        .map(|registry| {
            (
                (*registry).to_string(),
                RegistryOptions {
                    server_type: Some(RegistryServerType::Artifactory),
                    supports_time_field: None,
                },
            )
        })
        .collect();
    config
}

#[test]
fn no_warning_when_every_entry_matches_a_configured_registry() {
    let config = config_with(
        &[("default", "https://npm.example.com/"), ("@acme", "https://acme.example.com/")],
        &["https://npm.example.com/", "https://acme.example.com/"],
    );
    assert_eq!(unmatched_registry_options_warning(&config), None);
}

#[test]
fn no_warning_without_any_registry_options() {
    let config = config_with(&[("default", "https://npm.example.com/")], &[]);
    assert_eq!(unmatched_registry_options_warning(&config), None);
}

/// A built-in named registry is a legitimate target even though the user never
/// declared it, so an entry for it must not be reported as unmatched.
#[test]
fn no_warning_for_a_builtin_named_registry() {
    let config =
        config_with(&[("default", "https://npm.example.com/")], &["https://npm.pkg.github.com/"]);
    assert_eq!(unmatched_registry_options_warning(&config), None);
}

#[test]
fn warns_about_an_entry_matching_no_configured_registry() {
    let config = config_with(
        &[("default", "https://npm.example.com/")],
        &["https://npm.example.com/", "https://typo.example.com/"],
    );
    let received = unmatched_registry_options_warning(&config).expect("a warning");
    assert!(
        received.contains(r#"were ignored: "https://typo.example.com/"."#),
        "the unmatched entry must be named: {received}",
    );
    assert!(
        received.contains(r#""https://npm.example.com/""#),
        "the configured registries must be listed: {received}",
    );
}

/// A registry URL can carry `user:pass@` credentials, and this warning names
/// every configured registry, so it must not echo one into a CI log.
#[test]
fn redacts_credentials_in_the_warning() {
    let config = config_with(
        &[("default", "https://ci-user-6e42:hunter2@npm.example.com/")],
        &["https://typo.example.com/"],
    );
    let received = unmatched_registry_options_warning(&config).expect("a warning");
    assert!(!received.contains("hunter2"), "the password must not be echoed: {received}");
    assert!(!received.contains("ci-user-6e42"), "the username must not be echoed: {received}");
    assert!(received.contains("npm.example.com"), "the host is still named: {received}");
}

#[test]
fn a_repeated_config_load_reports_each_warning_once() {
    let mut emitted = HashSet::new();

    let mut first = Config { config_warnings: vec![warn("a"), warn("b")], ..Config::new() };
    assert_eq!(take_unemitted(&mut emitted, &mut first), vec![warn("a"), warn("b")]);
    assert!(first.config_warnings.is_empty(), "the drained config keeps no warnings");

    let mut reload =
        Config { config_warnings: vec![warn("a"), warn("b"), warn("c")], ..Config::new() };
    assert_eq!(take_unemitted(&mut emitted, &mut reload), vec![warn("c")]);
}

fn warn(id: &str) -> String {
    format!("warning {id}")
}
