use super::{ProfileArgs, ProfileCommand, ProfileError, render::format_profile_map};
use clap::Parser;
use miette::Diagnostic;
use mockito::Matcher;
use pnpm_config::Config;
use pnpm_network::AuthHeaders;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Parser)]
struct TestCli {
    #[clap(subcommand)]
    command: TestCommand,
}

#[derive(Debug, clap::Subcommand)]
enum TestCommand {
    Profile(ProfileArgs),
}

/// Define `ScriptedPrompts`, a prompt provider that answers the password and
/// OTP prompts from the given lists, in order. The answers live in statics
/// inside the calling test, because `read_password` reads on a blocking-pool
/// thread.
macro_rules! scripted_prompts {
    (passwords = [$($password:expr),* $(,)?], otps = [$($otp:expr),* $(,)?] $(,)?) => {
        static PASSWORDS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
        static OTPS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
        *PASSWORDS.lock().unwrap() = vec![$($password),*];
        *OTPS.lock().unwrap() = vec![$($otp),*];

        struct ScriptedPrompts;
        impl pnpm_auth_commands::login::PromptPassword for ScriptedPrompts {
            fn prompt_password(_message: &str) -> Result<String, dialoguer::Error> {
                let mut passwords = PASSWORDS.lock().unwrap();
                assert!(!passwords.is_empty(), "unexpected password prompt");
                Ok(passwords.remove(0).to_string())
            }
        }
        impl pnpm_network_web_auth::PromptOtp for ScriptedPrompts {
            async fn input(
                _message: &str,
            ) -> Result<Option<String>, pnpm_network_web_auth::PromptError> {
                let mut otps = OTPS.lock().unwrap();
                assert!(!otps.is_empty(), "unexpected OTP prompt");
                Ok(Some(otps.remove(0).to_string()))
            }
        }
    };
}

fn args(params: &[&str]) -> ProfileArgs {
    ProfileArgs {
        registry: None,
        otp: None,
        json: false,
        parseable: false,
        params: params
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

fn authed_config(registry: &str) -> Config {
    let nerf_dart = registry.trim_start_matches("http:").to_string();
    Config {
        registry: registry.to_string(),
        auth_headers: Arc::new(AuthHeaders::from_creds_map([(
            nerf_dart,
            "Bearer secret-token".to_string(),
        )])),
        ..Default::default()
    }
}

fn mock_profile(server: &mut mockito::ServerGuard, profile: &serde_json::Value) -> mockito::Mock {
    server
        .mock("GET", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .with_header("content-type", "application/json")
        .with_body(profile.to_string())
        .create()
}

fn mock_update(
    server: &mut mockito::ServerGuard,
    body: serde_json::Value,
    reply: &serde_json::Value,
) -> mockito::Mock {
    server
        .mock("POST", "/-/npm/v1/user")
        .match_header("authorization", "Bearer secret-token")
        .match_body(Matcher::Json(body))
        .with_header("content-type", "application/json")
        .with_body(reply.to_string())
        .create()
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
    assert_eq!(
        args.parse_command().unwrap(),
        ProfileCommand::Set {
            property: "fullname".to_string(),
            value: Some("Jane Doe".to_string())
        },
    );
}

#[test]
fn parse_command_accepts_comma_separated_get_keys() {
    assert_eq!(
        args(&["get", "name,email", "fullname"]).parse_command().unwrap(),
        ProfileCommand::Get {
            properties: vec!["name".to_string(), "email".to_string(), "fullname".to_string()],
        },
    );
}

#[test]
fn parse_command_rejects_invalid_arguments() {
    let cases: &[(&[&str], &str)] = &[
        (&[], "ERR_PNPM_PROFILE_SUBCOMMAND_REQUIRED"),
        (&["bogus"], "ERR_PNPM_PROFILE_UNKNOWN_COMMAND"),
        (&["set", "fullname"], "ERR_PNPM_PROFILE_ARGS_REQUIRED"),
        (&["set", "name", "x"], "ERR_PNPM_PROFILE_INVALID_PROPERTY"),
        (&["set", "password", "x"], "ERR_PNPM_PROFILE_PASSWORD_ON_COMMAND_LINE"),
        (&["enable-2fa", "sometimes"], "ERR_PNPM_PROFILE_INVALID_2FA_MODE"),
        (&["enable-2fa", "auth-only", "x"], "ERR_PNPM_PROFILE_ARGS_REQUIRED"),
    ];
    for (params, expected_code) in cases {
        let err = args(params).parse_command().unwrap_err();
        let code = err.code().map(|code| code.to_string());
        assert_eq!(code.as_deref(), Some(*expected_code), "{params:?} returned {err:?}");
    }
}

#[test]
fn parse_command_rejects_machine_output_for_enable_2fa() {
    let mut json = args(&["enable-2fa"]);
    json.json = true;
    assert!(matches!(
        json.parse_command().unwrap_err(),
        ProfileError::TfaInteractive { format: "JSON" },
    ));
}

#[tokio::test]
async fn run_reports_usage_errors_without_auth() {
    let config =
        Config { registry: "https://registry.npmjs.org/".to_string(), ..Default::default() };
    let err = args(&[]).run(&config).await.unwrap_err();
    assert!(err.to_string().contains("Subcommand is required"), "{err}");
}

#[tokio::test]
async fn run_fails_without_auth() {
    let config =
        Config { registry: "https://registry.npmjs.org/".to_string(), ..Default::default() };
    let err = args(&["get"]).run(&config).await.unwrap_err();
    assert!(err.to_string().contains("You must be logged in to view or change your profile"));
}

#[tokio::test]
async fn set_sends_the_other_writable_fields() {
    scripted_prompts!(passwords = [], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let get = mock_profile(
        &mut server,
        &json!({ "name": "alice", "email": "a@example.com", "github": "alice", "created": "x" }),
    );
    let post = mock_update(
        &mut server,
        json!({ "email": "a@example.com", "github": "alice", "fullname": "Alice Smith" }),
        &json!({ "fullname": "Alice Smith" }),
    );
    let output = args(&["set", "fullname", "Alice", "Smith"])
        .execute::<ScriptedPrompts>(&config)
        .await
        .unwrap();
    get.assert_async().await;
    post.assert_async().await;
    assert_eq!(output.as_deref(), Some("Set fullname to Alice Smith"));
}

#[tokio::test]
async fn set_password_prompts_until_the_new_passwords_match() {
    scripted_prompts!(passwords = ["old", "new", "typo", "new", "new"], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get = mock_profile(&mut server, &json!({ "name": "alice" }));
    let post = mock_update(
        &mut server,
        json!({ "password": { "old": "old", "new": "new" } }),
        &json!({ "name": "alice", "password": { "old": "old", "new": "new" } }),
    );
    let output = args(&["set", "password"]).execute::<ScriptedPrompts>(&config).await.unwrap();
    post.assert_async().await;
    assert_eq!(output.as_deref(), Some("Set password"));
}

#[tokio::test]
async fn enable_2fa_confirms_a_first_time_setup() {
    scripted_prompts!(passwords = ["hunter2"], otps = ["123456"]);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get = mock_profile(&mut server, &json!({ "name": "alice", "tfa": null }));
    let enable = mock_update(
        &mut server,
        json!({ "tfa": { "password": "hunter2", "mode": "auth-only" } }),
        &json!({ "tfa": "otpauth://totp/npm:alice?secret=ABCDEF&issuer=npm" }),
    );
    let confirm = mock_update(
        &mut server,
        json!({ "tfa": ["123456"] }),
        &json!({ "tfa": ["recovery-1", "recovery-2"] }),
    );
    let output = args(&["enable-2fa", "auth-only"])
        .execute::<ScriptedPrompts>(&config)
        .await
        .unwrap()
        .unwrap();
    enable.assert_async().await;
    confirm.assert_async().await;
    assert!(output.starts_with("2FA successfully enabled."), "{output}");
    assert!(output.ends_with("\n\trecovery-1\n\trecovery-2"), "{output}");
}

#[tokio::test]
async fn enable_2fa_resets_a_pending_setup_first() {
    scripted_prompts!(passwords = ["hunter2"], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get =
        mock_profile(&mut server, &json!({ "tfa": { "pending": true, "mode": "auth-only" } }));
    let reset = mock_update(
        &mut server,
        json!({ "tfa": { "password": "hunter2", "mode": "disable" } }),
        &json!({}),
    );
    let enable = mock_update(
        &mut server,
        json!({ "tfa": { "password": "hunter2", "mode": "auth-only" } }),
        &json!({ "tfa": { "mode": "auth-only" } }),
    );
    let output =
        args(&["enable-2fa", "auth-only"]).execute::<ScriptedPrompts>(&config).await.unwrap();
    reset.assert_async().await;
    enable.assert_async().await;
    assert_eq!(output.as_deref(), Some("Two factor authentication mode changed to: auth-only"));
}

#[tokio::test]
async fn enable_2fa_skips_an_unchanged_mode() {
    scripted_prompts!(passwords = [], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get =
        mock_profile(&mut server, &json!({ "tfa": { "pending": false, "mode": "auth-only" } }));
    let output =
        args(&["enable-2fa", "auth-only"]).execute::<ScriptedPrompts>(&config).await.unwrap();
    assert_eq!(
        output.as_deref(),
        Some("Two factor authentication is already enabled and set to auth-only"),
    );
}

#[tokio::test]
async fn disable_2fa_sends_the_password() {
    scripted_prompts!(passwords = ["hunter2"], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get =
        mock_profile(&mut server, &json!({ "tfa": { "pending": false, "mode": "auth-only" } }));
    let disable = mock_update(
        &mut server,
        json!({ "tfa": { "password": "hunter2", "mode": "disable" } }),
        &json!({}),
    );
    let output = args(&["disable-2fa"]).execute::<ScriptedPrompts>(&config).await.unwrap();
    disable.assert_async().await;
    assert_eq!(output.as_deref(), Some("Two factor authentication disabled."));
}

#[tokio::test]
async fn update_rejects_a_malformed_reply() {
    scripted_prompts!(passwords = [], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get = mock_profile(&mut server, &json!({}));
    let _post = server
        .mock("POST", "/-/npm/v1/user")
        .with_body("not json")
        .create();
    let err =
        args(&["set", "fullname", "x"]).execute::<ScriptedPrompts>(&config).await.unwrap_err();
    assert!(format!("{err:?}").contains("parsing profile response"), "{err:?}");
}

#[tokio::test]
async fn update_does_not_follow_redirects() {
    scripted_prompts!(passwords = ["hunter2"], otps = []);
    let mut server = mockito::Server::new_async().await;
    let mut elsewhere = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get =
        mock_profile(&mut server, &json!({ "tfa": { "pending": false, "mode": "auth-only" } }));
    let _redirect = server
        .mock("POST", "/-/npm/v1/user")
        .with_status(307)
        .with_header("location", &format!("{}/-/npm/v1/user", elsewhere.url()))
        .create();
    let forwarded = elsewhere
        .mock("POST", Matcher::Any)
        .expect(0)
        .create();
    let err = args(&["disable-2fa"]).execute::<ScriptedPrompts>(&config).await.unwrap_err();
    forwarded.assert_async().await;
    assert!(err.to_string().contains("307"), "{err}");
}

#[tokio::test]
async fn otp_challenge_asks_for_otp() {
    scripted_prompts!(passwords = [], otps = []);
    let mut server = mockito::Server::new_async().await;
    let config = authed_config(&format!("{}/", server.url()));
    let _get = server
        .mock("GET", "/-/npm/v1/user")
        .with_status(401)
        .with_body(r#"{"error":"You must provide a one-time pass."}"#)
        .create();
    let err = args(&["get"]).execute::<ScriptedPrompts>(&config).await.unwrap_err();
    assert!(err.to_string().contains("--otp"), "{err}");
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

#[test]
fn format_profile_map_reports_pending_2fa_as_disabled() {
    let mapped = format_profile_map(&json!({ "tfa": { "pending": true, "mode": "auth-only" } }));
    assert_eq!(mapped[2], ("two-factor auth".to_string(), "disabled".to_string()));
}
