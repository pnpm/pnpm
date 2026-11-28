use super::newer_release;
use crate::{
    Channel,
    RustToolchainError,
};
use pnpm_config::{
    Config,
    Tool,
    ToolSettings,
};
use pnpm_network::ThrottledClient;
use pretty_assertions::assert_eq;

fn config(mirror: &str) -> Config {
    let mut config = Config::new();
    config.fetch_retries = 0;
    config.tools.insert(
        Tool::Rust,
        ToolSettings { mirror: Some(mirror.to_string()), ..ToolSettings::default() },
    );
    config
}

/// A server whose `stable` channel is Rust 1.8.0, published on 2016-04-12.
async fn stable_server() -> mockito::ServerGuard {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/dist/channel-rust-stable.toml")
        .with_body(include_bytes!("../fixtures/channel-rust-1.8.0.toml"))
        .create_async()
        .await;
    server
        .mock("GET", "/dist/channel-rust-stable.toml.asc")
        .with_body(include_bytes!("../fixtures/channel-rust-1.8.0.toml.asc"))
        .create_async()
        .await;
    server
}

async fn newer_than(pin: &str) -> Option<String> {
    let server = stable_server().await;
    newer_release(
        &config(&server.url()),
        &ThrottledClient::new_for_installs(),
        &Channel::parse(pin).unwrap(),
    )
    .await
    .unwrap()
    .map(|release| release.to_string())
}

#[tokio::test]
async fn a_version_moves_to_the_newest_stable_release_as_precisely_as_it_was_written() {
    assert_eq!(newer_than("1.7.0").await.as_deref(), Some("1.8.0"));
    assert_eq!(newer_than("1.7").await.as_deref(), Some("1.8"));
    assert_eq!(newer_than("stable-2016-01-01").await.as_deref(), Some("stable-2016-04-12"));
}

#[tokio::test]
async fn the_newest_release_or_an_older_one_is_not_newer() {
    assert_eq!(newer_than("1.8.0").await, None);
    assert_eq!(newer_than("1.8").await, None);
    assert_eq!(newer_than("1.9.0").await, None);
}

#[tokio::test]
async fn a_channel_that_moves_by_itself_is_not_looked_up() {
    let config = config("http://127.0.0.1:1");
    let client = ThrottledClient::new_for_installs();
    for pin in ["stable", "beta", "nightly"] {
        let newer = newer_release(&config, &client, &Channel::parse(pin).unwrap())
            .await
            .unwrap();
        assert_eq!(newer, None, "{pin}");
    }
}

#[tokio::test]
async fn offline_a_pin_cannot_be_moved() {
    let mut config = config("http://127.0.0.1:1");
    config.offline = true;
    let error = newer_release(
        &config,
        &ThrottledClient::new_for_installs(),
        &Channel::parse("1.7.0").unwrap(),
    )
    .await
    .expect_err("the distribution server cannot be asked offline");
    assert!(matches!(error, RustToolchainError::OfflineUpdate { .. }), "{error:?}");
}
