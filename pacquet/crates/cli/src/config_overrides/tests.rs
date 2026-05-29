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
}

#[test]
fn apply_is_a_noop_when_no_overrides_set() {
    let (overrides, _) = ConfigOverrides::extract(argv(["pacquet", "install"]));
    let default_registry = Config::default().registry;
    let mut config = Config::default();
    overrides.apply(&mut config);
    assert_eq!(config.registry, default_registry);
}
