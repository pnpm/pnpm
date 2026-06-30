use super::LogoutArgs;
use pacquet_config::Config;
use pacquet_reporter::SilentReporter;

/// `Config::default()` leaves `config_dir` as `None`; `run` must reject
/// that before touching the network, since it cannot locate `auth.ini`.
#[tokio::test]
async fn errors_when_config_dir_is_unavailable() {
    let err = LogoutArgs { registry: None }
        .run::<SilentReporter>(&Config::default(), "/prefix")
        .await
        .expect_err("missing config dir should error");
    assert!(
        err.to_string().contains("Could not determine the pnpm config directory"),
        "unexpected error: {err}",
    );
}
