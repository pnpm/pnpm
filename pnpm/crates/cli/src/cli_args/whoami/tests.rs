use super::{WhoamiArgs, WhoamiError};
use clap::Parser;
use pnpm_config::Config;

#[derive(Debug, Parser)]
struct TestCli {
    #[clap(subcommand)]
    command: TestCommand,
}

#[derive(Debug, Parser)]
enum TestCommand {
    Whoami(WhoamiArgs),
}

#[test]
fn parses_registry_flag() {
    let cli =
        TestCli::try_parse_from(["test", "whoami", "--registry", "https://custom.registry.org/"])
            .expect("parse command line");
    let TestCommand::Whoami(args) = cli.command;
    assert_eq!(args.registry.as_deref(), Some("https://custom.registry.org/"));
}

#[tokio::test]
async fn run_fails_when_unauthorized_for_overridden_registry() {
    let mut config =
        Config { registry: "https://default.registry.org/".to_string(), ..Config::default() };
    let mut auth_headers = (*config.auth_headers).clone();
    auth_headers.insert_url_header(
        "https://default.registry.org/",
        "Bearer default-token".to_string(),
    );
    config.auth_headers = std::sync::Arc::new(auth_headers);

    let args = WhoamiArgs { registry: Some("https://custom.unauthorized.registry/".to_string()) };

    let error =
        args.run(&config).await.expect_err("whoami without auth for overridden registry must fail");
    let root_error = error.downcast_ref::<WhoamiError>();
    assert!(
        matches!(root_error, Some(WhoamiError::Unauthorized)),
        "expected WhoamiError::Unauthorized, got: {error:?}",
    );
}

#[tokio::test]
async fn run_fails_when_unauthorized_for_default_registry() {
    let config = Config::default();
    let args = WhoamiArgs::default();

    let error =
        args.run(&config).await.expect_err("whoami without auth for default registry must fail");
    let root_error = error.downcast_ref::<WhoamiError>();
    assert!(
        matches!(root_error, Some(WhoamiError::Unauthorized)),
        "expected WhoamiError::Unauthorized, got: {error:?}",
    );
}
