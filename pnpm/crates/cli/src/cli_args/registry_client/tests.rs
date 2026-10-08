use super::{
    build_registry_client_with_otp_guard, join_registry_endpoint, publish_network_settings,
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

#[test]
fn join_registry_endpoint_resolves_path_relative_to_registry() {
    assert_eq!(
        join_registry_endpoint("https://registry.npmjs.org", "-/package/foo/dist-tags").unwrap(),
        "https://registry.npmjs.org/-/package/foo/dist-tags",
    );
    assert_eq!(
        join_registry_endpoint("https://registry.npmjs.org/", "-/package/foo/dist-tags").unwrap(),
        "https://registry.npmjs.org/-/package/foo/dist-tags",
    );
    assert_eq!(
        join_registry_endpoint("https://custom.registry.com/prefix", "-/package/foo/dist-tags")
            .unwrap(),
        "https://custom.registry.com/prefix/-/package/foo/dist-tags",
    );
}

#[test]
fn resolve_registry_for_package_picks_and_normalizes() {
    let mut registries = std::collections::HashMap::new();
    registries.insert("default".to_string(), "https://registry.npmjs.org".to_string());
    registries.insert("@my-scope".to_string(), "https://npm.pkg.github.com/my-org".to_string());

    assert_eq!(
        super::resolve_registry_for_package(&registries, "lodash", None),
        "https://registry.npmjs.org/",
    );
    assert_eq!(
        super::resolve_registry_for_package(&registries, "@my-scope/pkg", None),
        "https://npm.pkg.github.com/my-org/",
    );
}

#[test]
fn package_endpoint_url_escapes_scoped_names() {
    assert_eq!(
        super::package_endpoint_url("https://registry.npmjs.org", "lodash").unwrap(),
        "https://registry.npmjs.org/lodash",
    );
    assert_eq!(
        super::package_endpoint_url("https://registry.npmjs.org/", "@scope/pkg").unwrap(),
        "https://registry.npmjs.org/@scope%2fpkg",
    );
    assert_eq!(
        super::package_endpoint_url("https://registry.org/prefix", "@scope/pkg").unwrap(),
        "https://registry.org/prefix/@scope%2fpkg",
    );
}

#[test]
fn apply_auth_and_otp_attaches_headers() {
    let client = reqwest::Client::new();

    let with_both = super::apply_auth_and_otp(
        client.get("https://registry.npmjs.org"),
        Some("Bearer token"),
        Some("123456"),
    )
    .build()
    .unwrap();
    assert_eq!(
        with_both
            .headers()
            .get("authorization")
            .unwrap(),
        "Bearer token",
    );
    assert_eq!(
        with_both
            .headers()
            .get("npm-otp")
            .unwrap(),
        "123456",
    );

    let with_none = super::apply_auth_and_otp(client.get("https://registry.npmjs.org"), None, None)
        .build()
        .unwrap();
    assert!(!with_none.headers().contains_key("authorization"));
    assert!(!with_none.headers().contains_key("npm-otp"));
}
