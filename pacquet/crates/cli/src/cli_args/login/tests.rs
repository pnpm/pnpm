use std::path::{Path, PathBuf};

use pacquet_config::Config;
use pacquet_network_web_auth_testing::{ok_token, web_auth_fake};
use pacquet_reporter::SilentReporter;

use super::LoginArgs;

/// Add the login-specific capability impls to the `web_auth_fake!`-generated
/// `FakeHost` so it satisfies `LoginHost`. The web-login path these tests drive
/// never prompts for credentials, so the two prompt impls are unreachable;
/// `auth.ini` reads return empty and writes are dropped.
macro_rules! login_host_fake {
    ($fake:ident) => {
        impl pacquet_auth_commands::login::PromptInput for $fake {
            fn prompt_input(_message: &str) -> Result<String, dialoguer::Error> {
                unreachable!("the web-login path does not prompt for credentials")
            }
        }
        impl pacquet_auth_commands::login::PromptPassword for $fake {
            fn prompt_password(_message: &str) -> Result<String, dialoguer::Error> {
                unreachable!("the web-login path does not prompt for credentials")
            }
        }
        impl pacquet_auth_commands::logout::FsReadToString for $fake {
            fn read_to_string(_path: &Path) -> std::io::Result<String> {
                Ok(String::new())
            }
        }
        impl pacquet_auth_commands::logout::FsWrite for $fake {
            fn write(_path: &Path, _bytes: &[u8]) -> std::io::Result<()> {
                Ok(())
            }
        }
    };
}

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

/// `execute` performs the web-login flow end-to-end against a mock registry and
/// returns the success message `run` would print, driven through a fake host so
/// no real terminal or network is touched. The web-login `POST` goes over the
/// real HTTP client to `mockito`; the token poll is served by the fake fetch.
#[tokio::test]
async fn execute_performs_web_login_and_returns_the_success_message() {
    web_auth_fake!();
    login_host_fake!(FakeHost);
    reset();
    set_fetch(Box::new(|| Ok(ok_token("web-token"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(serde_json::json!({"loginUrl": "https://example.org/auth/login", "doneUrl": "https://example.org/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();

    let config = Config { config_dir: Some(PathBuf::from("/mock/config")), ..Default::default() };
    let args = LoginArgs { registry: Some(registry.clone()), scope: None };

    let message =
        args.execute::<FakeHost, RecordingReporter>(&config).await.expect("web login succeeds");

    assert_eq!(message, format!("Logged in on {registry}/"));
}

/// `execute` propagates `login`'s non-interactive-terminal error when the fake
/// host reports no TTY, covering the path from the config-dir guard through the
/// HTTP-client build to the login call.
#[tokio::test]
async fn execute_propagates_the_non_interactive_error_from_login() {
    web_auth_fake!();
    login_host_fake!(FakeHost);
    reset();
    set_stdin_tty(false);

    let config = Config { config_dir: Some(PathBuf::from("/mock/config")), ..Default::default() };
    let args = LoginArgs { registry: Some("http://127.0.0.1:9/".to_owned()), scope: None };

    let err = args.execute::<FakeHost, RecordingReporter>(&config).await.unwrap_err();

    assert!(
        err.to_string().contains("requires an interactive terminal"),
        "unexpected error: {err}",
    );
}
