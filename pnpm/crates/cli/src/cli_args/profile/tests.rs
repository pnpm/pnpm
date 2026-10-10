use super::{ProfileArgs, render::format_profile_map};
use clap::Parser;
use pnpm_config::Config;
use serde_json::json;

#[derive(Debug, Parser)]
struct TestCli {
    #[clap(subcommand)]
    command: TestCommand,
}

#[derive(Debug, clap::Subcommand)]
enum TestCommand {
    Profile(ProfileArgs),
}

#[test]
fn parse_profile_get_all() {
    let cli = TestCli::parse_from(["pnpm", "profile", "get"]);
    let TestCommand::Profile(args) = cli.command;
    assert_eq!(args.params, vec!["get"]);
    assert!(!args.json);
    assert!(!args.parseable);
    assert!(args.registry.is_none());
    assert!(args.otp.is_none());
}

#[test]
fn parse_profile_get_property() {
    let cli = TestCli::parse_from(["pnpm", "profile", "get", "email", "--json"]);
    let TestCommand::Profile(args) = cli.command;
    assert_eq!(args.params, vec!["get", "email"]);
    assert!(args.json);
}

#[test]
fn parse_profile_set_property() {
    let cli = TestCli::parse_from([
        "pnpm",
        "profile",
        "set",
        "fullname",
        "Jane",
        "Doe",
        "-p",
        "--registry",
        "https://custom.registry.org/",
        "--otp",
        "123456",
    ]);
    let TestCommand::Profile(args) = cli.command;
    assert_eq!(args.params, vec!["set", "fullname", "Jane", "Doe"]);
    assert!(args.parseable);
    assert_eq!(args.registry.as_deref(), Some("https://custom.registry.org/"));
    assert_eq!(args.otp.as_deref(), Some("123456"));
}

#[test]
fn parse_profile_enable_2fa() {
    let cli = TestCli::parse_from(["pnpm", "profile", "enable-2fa", "auth-only"]);
    let TestCommand::Profile(args) = cli.command;
    assert_eq!(args.params, vec!["enable-2fa", "auth-only"]);
}

#[test]
fn parse_profile_disable_2fa() {
    let cli = TestCli::parse_from(["pnpm", "profile", "disable-2fa"]);
    let TestCommand::Profile(args) = cli.command;
    assert_eq!(args.params, vec!["disable-2fa"]);
}

#[tokio::test]
async fn run_fails_without_subcommand() {
    let args =
        ProfileArgs { registry: None, otp: None, json: false, parseable: false, params: vec![] };
    let config =
        Config { registry: "https://registry.npmjs.org/".to_string(), ..Default::default() };
    let err = args.run(&config).await.unwrap_err();
    assert!(err.to_string().contains("Subcommand is required"));
}

#[tokio::test]
async fn run_fails_without_auth() {
    let args = ProfileArgs {
        registry: None,
        otp: None,
        json: false,
        parseable: false,
        params: vec!["get".to_string()],
    };
    let config =
        Config { registry: "https://registry.npmjs.org/".to_string(), ..Default::default() };
    let err = args.run(&config).await.unwrap_err();
    assert!(err.to_string().contains("You must be logged in to view or change your profile"));
}

#[test]
fn format_profile_map_ordering_and_fields() {
    let info = json!({
        "name": "alice",
        "email": "alice@example.com",
        "email_verified": true,
        "tfa": {
            "mode": "auth-and-writes",
        },
        "fullname": "Alice Smith",
        "homepage": "https://alice.dev",
        "freenode": "alice_irc",
        "twitter": "alice_tw",
        "github": "alice_gh",
        "created": "2020-01-01T00:00:00.000Z",
        "updated": "2021-01-01T00:00:00.000Z",
        "custom_field": "extra",
    });

    let mapped = format_profile_map(&info);
    assert_eq!(mapped[0], ("name".to_string(), "alice".to_string()));
    assert_eq!(mapped[1], ("email".to_string(), "alice@example.com (verified)".to_string()));
    assert_eq!(mapped[2], ("two-factor auth".to_string(), "auth-and-writes".to_string()));
    assert_eq!(mapped[3], ("fullname".to_string(), "Alice Smith".to_string()));
    assert_eq!(mapped[4], ("homepage".to_string(), "https://alice.dev".to_string()));
    assert_eq!(mapped[5], ("freenode".to_string(), "alice_irc".to_string()));
    assert_eq!(mapped[6], ("twitter".to_string(), "alice_tw".to_string()));
    assert_eq!(mapped[7], ("github".to_string(), "alice_gh".to_string()));
    assert_eq!(mapped[8], ("created".to_string(), "2020-01-01T00:00:00.000Z".to_string()));
    assert_eq!(mapped[9], ("updated".to_string(), "2021-01-01T00:00:00.000Z".to_string()));
    assert_eq!(mapped[10], ("custom_field".to_string(), "extra".to_string()));
}
