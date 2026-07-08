use std::path::Path;

use pacquet_config::Config;
use pacquet_reporter::SilentReporter;

use super::LoginArgs;

/// `Config::default()` leaves `config_dir` as `None`; `run` must reject that
/// before touching the network, since it cannot locate `auth.ini`. Mirrors the
/// `logout` adapter's guard.
#[tokio::test]
async fn errors_when_config_dir_is_unavailable() {
    let err = LoginArgs { registry: None, scope: None }
        .run::<SilentReporter>(&Config::default())
        .await
        .expect_err("missing config dir should error");
    assert!(
        err.to_string().contains("Could not determine the pnpm config directory"),
        "unexpected error: {err}",
    );
}

/// `--registry` overrides the resolved `config.registry`.
#[test]
fn registry_flag_overrides_the_configured_registry() {
    let config = Config::default();
    let args = LoginArgs { registry: Some("https://flag.example/".to_owned()), scope: None };

    let options = args.login_options(&config, Path::new("/cfg"));

    assert_eq!(options.registry, Some("https://flag.example/"));
    assert_ne!(
        options.registry,
        Some(config.registry.as_str()),
        "the flag must win over the configured registry",
    );
}

/// Without `--registry`, the resolved `config.registry` is used; `--scope`,
/// `config_dir`, and every fetch knob pass straight through.
#[test]
fn resolves_configured_registry_scope_and_fetch_settings() {
    let config = Config::default();
    let args = LoginArgs { registry: None, scope: Some("my-org".to_owned()) };
    let config_dir = Path::new("/cfg");

    let options = args.login_options(&config, config_dir);

    assert_eq!(options.registry, Some(config.registry.as_str()));
    assert_eq!(options.scope, Some("my-org"));
    assert_eq!(options.config_dir, config_dir);
    assert_eq!(options.fetch_retries, config.fetch_retries);
    assert_eq!(options.fetch_retry_factor, config.fetch_retry_factor);
    assert_eq!(options.fetch_retry_mintimeout, config.fetch_retry_mintimeout);
    assert_eq!(options.fetch_retry_maxtimeout, config.fetch_retry_maxtimeout);
    assert_eq!(options.fetch_timeout, config.fetch_timeout);
}
