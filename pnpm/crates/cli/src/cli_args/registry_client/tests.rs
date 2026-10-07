use super::{
    build_registry_client_with_otp_guard, publish_network_settings,
    resolve_registries_with_override,
};
use pnpm_config::Config;
use std::time::Duration;

#[test]
fn publish_client_waits_at_least_five_minutes() {
    let config = Config { fetch_timeout: 60_000, ..Config::default() };
    assert_eq!(publish_network_settings(&config).fetch_timeout, Duration::from_mins(5));
}

#[test]
fn publish_client_keeps_a_longer_fetch_timeout() {
    let config = Config { fetch_timeout: 10 * 60 * 1000, ..Config::default() };
    assert_eq!(publish_network_settings(&config).fetch_timeout, Duration::from_mins(10));
}

#[test]
fn resolve_registries_without_override_returns_config_default() {
    let config =
        Config { registry: "https://registry.npmjs.org/".to_string(), ..Config::default() };
    let registries = resolve_registries_with_override(&config, None);
    assert_eq!(registries.get("default").map(String::as_str), Some("https://registry.npmjs.org/"));
}

#[test]
fn resolve_registries_with_override_normalizes_trailing_slash() {
    let config =
        Config { registry: "https://registry.npmjs.org/".to_string(), ..Config::default() };
    let registries = resolve_registries_with_override(&config, Some("https://custom.registry.org"));
    assert_eq!(registries.get("default").map(String::as_str), Some("https://custom.registry.org/"));
}

#[test]
fn build_registry_client_with_otp_guard_succeeds() {
    let config = Config::default();
    let client_without_otp =
        build_registry_client_with_otp_guard(&config, None, ["https://registry.npmjs.org/"]);
    assert!(client_without_otp.is_ok());

    let client_with_otp = build_registry_client_with_otp_guard(
        &config,
        Some("123456"),
        ["https://registry.npmjs.org/"],
    );
    assert!(client_with_otp.is_ok());
}
