use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use pacquet_config::Config;
use pacquet_network::nerf_dart;
use pacquet_network_web_auth_testing::{ok_token, web_auth_fake};
use pacquet_reporter::SilentReporter;

use super::LoginArgs;

/// Add the login-specific capability impls to the `web_auth_fake!`-generated
/// `FakeHost` so it satisfies `LoginHost`. The web-login path these tests drive
/// never prompts for credentials, so the two prompt impls are unreachable;
/// `auth.ini` reads return empty and writes are recorded in fn-local state.
macro_rules! login_host_fake {
    ($fake:ident $(, $helper:ident)* $(,)?) => {
        thread_local! {
            static INI_WRITES: RefCell<Vec<(PathBuf, String)>> = const { RefCell::new(Vec::new()) };
        }

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
            fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
                let text = String::from_utf8(bytes.to_vec()).expect("auth.ini is UTF-8");
                INI_WRITES.with(|writes| writes.borrow_mut().push((path.to_path_buf(), text)));
                Ok(())
            }
        }

        $( login_host_fake!(@helper $helper); )*
    };

    (@helper auth_ini_writes) => {
        fn auth_ini_writes() -> Vec<(PathBuf, String)> {
            INI_WRITES.with(|writes| writes.borrow().clone())
        }
    };

    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `login_host_fake!` helper `",
            stringify!($unknown),
            "`; expected one of: auth_ini_writes",
        ));
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

#[test]
fn falls_back_to_the_configured_scope_when_the_flag_is_absent() {
    let config = Config { scope: Some("@my-org".to_owned()), ..Default::default() };
    let args = LoginArgs { registry: None, scope: None };

    let options = args.login_options(&config, Path::new("/cfg"));

    assert_eq!(options.scope, Some("@my-org"));
}

#[test]
fn scope_flag_overrides_the_configured_scope() {
    let config = Config { scope: Some("@from-config".to_owned()), ..Default::default() };
    let args = LoginArgs { registry: None, scope: Some("@from-flag".to_owned()) };

    let options = args.login_options(&config, Path::new("/cfg"));

    assert_eq!(options.scope, Some("@from-flag"));
}

#[test]
fn no_scope_when_neither_flag_nor_config_is_set() {
    let config = Config::default();
    let args = LoginArgs { registry: None, scope: None };

    let options = args.login_options(&config, Path::new("/cfg"));

    assert_eq!(options.scope, None);
}

/// Serves only the handshake; the caller must script the fake fetch that
/// answers the token poll.
async fn web_login_server(server: &mut mockito::Server) -> String {
    let body = serde_json::json!({
        "loginUrl": "https://example.org/auth/login",
        "doneUrl": "https://example.org/auth/done",
    })
    .to_string();
    server.mock("POST", "/-/v1/login").with_status(200).with_body(body).create_async().await;
    server.url()
}

fn last_auth_ini(writes: &[(PathBuf, String)]) -> (&Path, &str) {
    let (path, text) = writes.last().expect("login must write auth.ini");
    (path.as_path(), text.as_str())
}

/// Pins the composition the option-level tests above and the write-path tests
/// in `pacquet-auth-commands` each cover only half of: [`Config::scope`] —
/// wherever it came from — reaching `auth.ini` through the adapter.
#[tokio::test]
async fn a_config_scope_persists_the_scoped_token_and_registry_mapping() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    login_host_fake!(FakeHost, auth_ini_writes);
    reset();
    set_fetch(Box::new(|| Ok(ok_token("config-scope-token"))));

    let mut server = mockito::Server::new_async().await;
    let registry = web_login_server(&mut server).await;

    let config = Config {
        config_dir: Some(PathBuf::from("/mock/config")),
        scope: Some("@my-org".to_owned()),
        ..Default::default()
    };
    let args = LoginArgs { registry: Some(registry.clone()), scope: None };

    args.execute::<FakeHost, RecordingReporter>(&config).await.expect("web login succeeds");

    let writes = auth_ini_writes();
    let (path, text) = last_auth_ini(&writes);
    assert_eq!(path, Path::new("/mock/config").join("auth.ini"));
    let registry_key = nerf_dart(&format!("{registry}/"));
    assert!(
        text.contains(&format!("{registry_key}:@my-org:_authToken=config-scope-token")),
        "auth.ini is missing the scoped token: {text}",
    );
    assert!(
        text.contains(&format!("@my-org:registry={registry}/")),
        "auth.ini is missing the scope-to-registry mapping: {text}",
    );
    assert!(
        !text.contains(&format!("{registry_key}:_authToken=")),
        "the token must not also be written unscoped: {text}",
    );
}

#[tokio::test]
async fn the_scope_flag_beats_a_config_scope_in_the_persisted_auth_ini() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    login_host_fake!(FakeHost, auth_ini_writes);
    reset();
    set_fetch(Box::new(|| Ok(ok_token("flag-scope-token"))));

    let mut server = mockito::Server::new_async().await;
    let registry = web_login_server(&mut server).await;

    let config = Config {
        config_dir: Some(PathBuf::from("/mock/config")),
        scope: Some("@from-config".to_owned()),
        ..Default::default()
    };
    let args = LoginArgs { registry: Some(registry.clone()), scope: Some("@from-flag".to_owned()) };

    args.execute::<FakeHost, RecordingReporter>(&config).await.expect("web login succeeds");

    let writes = auth_ini_writes();
    let (_, text) = last_auth_ini(&writes);
    let registry_key = nerf_dart(&format!("{registry}/"));
    assert!(
        text.contains(&format!("{registry_key}:@from-flag:_authToken=flag-scope-token")),
        "auth.ini is missing the flag's scoped token: {text}",
    );
    assert!(
        text.contains(&format!("@from-flag:registry={registry}/")),
        "auth.ini is missing the flag's scope-to-registry mapping: {text}",
    );
    assert!(!text.contains("@from-config"), "the config scope must not reach auth.ini: {text}");
}

/// `execute` performs the web-login flow end-to-end against a mock registry and
/// returns the success message `run` would print, driven through a fake host so
/// no real terminal or network is touched. The web-login `POST` goes over the
/// real HTTP client to `mockito`; the token poll is served by the fake fetch.
#[tokio::test]
async fn execute_performs_web_login_and_returns_the_success_message() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    login_host_fake!(FakeHost);
    reset();
    set_fetch(Box::new(|| Ok(ok_token("web-token"))));

    let mut server = mockito::Server::new_async().await;
    let registry = web_login_server(&mut server).await;

    let config = Config { config_dir: Some(PathBuf::from("/mock/config")), ..Default::default() };
    let args = LoginArgs { registry: Some(registry.clone()), scope: None };

    let message =
        args.execute::<FakeHost, RecordingReporter>(&config).await.expect("web login succeeds");

    assert_eq!(message, format!("Logged in on {registry}/"));
}

/// `execute` propagates `login`'s non-interactive-terminal error when the fake
/// host reports no TTY and the registry answers the web-login probe with 404,
/// forcing the classic (prompting) fallback. Covers the path from the
/// config-dir guard through the HTTP-client build to the login call; the
/// `unreachable!` prompt impls double as proof the guard fires before any
/// credential prompt.
#[tokio::test]
async fn execute_propagates_the_non_interactive_error_from_login() {
    web_auth_fake!(FakeHost, RecordingReporter, set_stdin_tty);
    login_host_fake!(FakeHost);
    reset();
    set_stdin_tty(false);

    let mut server = mockito::Server::new_async().await;
    let web_login_probe = server
        .mock("POST", "/-/v1/login")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;
    let registry = server.url();

    let config = Config { config_dir: Some(PathBuf::from("/mock/config")), ..Default::default() };
    let args = LoginArgs { registry: Some(registry), scope: None };

    let err = args.execute::<FakeHost, RecordingReporter>(&config).await.unwrap_err();

    web_login_probe.assert_async().await;
    assert!(
        err.to_string().contains("requires an interactive terminal"),
        "unexpected error: {err}",
    );
}
