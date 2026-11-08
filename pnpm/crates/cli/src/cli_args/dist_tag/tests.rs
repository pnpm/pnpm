use super::{DistTagArgs, DistTagContext, registry::registry_for_package};
use mockito::Matcher;
use pnpm_config::Config;
use pnpm_network_web_auth_testing::{InputResponse, ok_token, web_auth_fake};
use serde_json::json;
use std::collections::HashMap;

fn dist_tag_args(registry: &str, params: &[&str]) -> DistTagArgs {
    DistTagArgs {
        registry: Some(registry.to_owned()),
        otp: None,
        params: params
            .iter()
            .map(|param| (*param).to_owned())
            .collect(),
    }
}

#[tokio::test]
async fn add_answers_a_web_auth_challenge_and_retries_with_the_token() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    reset();
    set_fetch(Box::new(|| Ok(ok_token("web-token"))));

    let mut server = mockito::Server::new_async().await;
    let challenge_mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "dist-tag")
        .match_header("npm-otp", Matcher::Missing)
        .with_status(401)
        .with_body(
            json!({
                "authUrl": "https://auth.example/login",
                "doneUrl": "https://auth.example/done",
            })
            .to_string(),
        )
        .create_async()
        .await;
    let retry_mock = server
        .mock("PUT", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "dist-tag")
        .match_header("npm-otp", "web-token")
        .match_body(Matcher::Exact(r#""1.0.0""#.to_string()))
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let output = dist_tag_args(&format!("{}/", server.url()), &["add", "pkg@1.0.0", "beta"])
        .execute::<FakeHost, RecordingReporter>(&Config::default())
        .await
        .expect("the challenge is answered and the tag is set");

    assert_eq!(output.as_deref(), Some("+beta: pkg@1.0.0"));
    challenge_mock.assert_async().await;
    retry_mock.assert_async().await;
}

#[tokio::test]
async fn rm_answers_a_classic_otp_challenge_with_the_prompted_code() {
    web_auth_fake!(FakeHost, RecordingReporter, set_input);
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));

    let mut server = mockito::Server::new_async().await;
    let tags_mock = server
        .mock("GET", "/-/package/pkg/dist-tags")
        .with_status(200)
        .with_body(json!({ "latest": "1.0.0", "beta": "1.1.0" }).to_string())
        .create_async()
        .await;
    let challenge_mock = server
        .mock("DELETE", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-command", "dist-tag")
        .match_header("npm-otp", Matcher::Missing)
        .with_status(401)
        .with_body(json!({ "error": "one-time pass required" }).to_string())
        .create_async()
        .await;
    let retry_mock = server
        .mock("DELETE", "/-/package/pkg/dist-tags/beta")
        .match_header("npm-command", "dist-tag")
        .match_header("npm-otp", "123456")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let output = dist_tag_args(&format!("{}/", server.url()), &["rm", "pkg", "beta"])
        .execute::<FakeHost, RecordingReporter>(&Config::default())
        .await
        .expect("the challenge is answered and the tag is removed");

    assert_eq!(output.as_deref(), Some("-beta: pkg@1.1.0"));
    tags_mock.assert_async().await;
    challenge_mock.assert_async().await;
    retry_mock.assert_async().await;
}

#[test]
fn registry_for_package_uses_scoped_registry_by_default() {
    let config = Config::default();
    let registries = HashMap::from([
        ("default".to_string(), "https://registry.example/npm/".to_string()),
        ("@scope".to_string(), "https://scoped.example/npm/".to_string()),
    ]);
    let context = DistTagContext {
        config: &config,
        http_client: pnpm_network::ThrottledClient::default(),
        retry_opts: pnpm_network::RetryOpts::default(),
        registries,
        registry_override: None,
        otp: None,
    };
    let registry = registry_for_package(&context, "@scope/my-pkg");
    assert_eq!(registry, "https://scoped.example/npm/");
}

#[test]
fn registry_for_package_honors_registry_override_for_scoped_package() {
    let config = Config::default();
    let registries = HashMap::from([
        ("default".to_string(), "https://registry.example/npm/".to_string()),
        ("@scope".to_string(), "https://scoped.example/npm/".to_string()),
    ]);
    let context = DistTagContext {
        config: &config,
        http_client: pnpm_network::ThrottledClient::default(),
        retry_opts: pnpm_network::RetryOpts::default(),
        registries,
        registry_override: Some("https://override.example/npm/"),
        otp: None,
    };
    let registry = registry_for_package(&context, "@scope/my-pkg");
    assert_eq!(registry, "https://override.example/npm/");
}
