use super::ConfigOverrides;
use pacquet_config::Config;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn argv<Items: IntoIterator<Item = &'static str>>(items: Items) -> Vec<OsString> {
    items.into_iter().map(OsString::from).collect()
}

#[test]
fn extract_separates_config_tokens_from_argv() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.registry=https://example.test/",
        "install",
        "--frozen-lockfile",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install", "--frozen-lockfile"]));
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, "https://example.test/");
    assert_eq!(config.package_manager_bootstrap.registry, "https://example.test/");
    assert_eq!(config.registries.get("default").map(String::as_str), Some("https://example.test/"));
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://example.test/"),
    );
}

#[test]
fn extract_applies_scoped_registry_overrides() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.@private:registry=https://private.example/npm",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(
        config.registries.get("@private").map(String::as_str),
        Some("https://private.example/npm/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@private").map(String::as_str),
        Some("https://private.example/npm/"),
    );
}

#[test]
fn scoped_registry_override_wins_over_existing_config() {
    let (overrides, _) =
        ConfigOverrides::extract(argv(["--config.@private:registry=https://cli.example/npm/"]));
    let mut config = Config::default();
    config.registries.insert("@private".to_string(), "https://workspace.example/npm/".to_string());
    config
        .package_manager_bootstrap
        .registries
        .insert("@private".to_string(), "https://json-env.example/npm/".to_string());
    overrides.apply(&mut config);
    assert_eq!(
        config.registries.get("@private").map(String::as_str),
        Some("https://cli.example/npm/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@private").map(String::as_str),
        Some("https://cli.example/npm/"),
    );
}

#[test]
fn unknown_keys_are_dropped_silently() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.unknown-key=whatever", "install"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let default_registry = Config::default().registry;
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, default_registry, "no known key set ⇒ registry untouched");
}

#[test]
fn config_tokens_after_external_command_stay_in_argv() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.registry=https://example.test/",
        "--dir",
        "project",
        "commitlint",
        "--config.foo=bar",
    ]));
    let expected = argv(["pacquet", "--dir", "project", "commitlint", "--config.foo=bar"]);
    assert_eq!(remaining, expected);
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, "https://example.test/");
}

#[cfg(unix)]
#[test]
fn non_utf8_token_stops_config_token_extraction() {
    use std::os::unix::ffi::OsStringExt;

    let non_utf8 = OsString::from_vec(vec![0xff]);
    let (overrides, remaining) = ConfigOverrides::extract(vec![
        OsString::from("pacquet"),
        OsString::from("--config.registry=https://example.test/"),
        non_utf8.clone(),
        OsString::from("--config.foo=bar"),
    ]);
    let expected = vec![OsString::from("pacquet"), non_utf8, OsString::from("--config.foo=bar")];
    assert_eq!(remaining, expected);

    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, "https://example.test/");
}

#[test]
fn malformed_tokens_are_dropped() {
    let (_, remaining) =
        ConfigOverrides::extract(argv(["--config.registry", "--config.=missing-key", "install"]));
    assert_eq!(remaining, argv(["install"]));
}

#[test]
fn last_value_wins_for_repeated_keys() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "--config.registry=https://first.test/",
        "--config.registry=https://second.test/",
    ]));
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, "https://second.test/");
    assert_eq!(config.package_manager_bootstrap.registry, "https://second.test/");
}

#[test]
fn apply_is_a_noop_when_no_overrides_set() {
    let (overrides, _) = ConfigOverrides::extract(argv(["pacquet", "install"]));
    let default_registry = Config::default().registry;
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, default_registry);
}
