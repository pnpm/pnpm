use super::publish_network_settings;
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
