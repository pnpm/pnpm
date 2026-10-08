use super::{
    Channel, InstalledToolchain, Profile, RustToolchainError, ToolchainRequest,
    install::{record_resolution, toolchain_dir},
    install_for_host, linked_bin_dir,
};
use pnpm_config::{Config, Tool, ToolSettings};
use pnpm_network::ThrottledClient;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::fs;

const HOST: &str = "x86_64-unknown-linux-gnu";

fn config(store: &std::path::Path, mirror: &str) -> Config {
    let mut config = Config::new();
    config.store_dir = pnpm_store_dir::StoreDir::from(store.to_path_buf());
    config.fetch_retries = 0;
    config.tools.insert(
        Tool::Rust,
        ToolSettings { mirror: Some(mirror.to_string()), ..ToolSettings::default() },
    );
    config
}

fn request(channel: &str) -> ToolchainRequest {
    ToolchainRequest {
        channel: Channel::parse(channel).unwrap(),
        profile: Profile::Minimal,
        components: Vec::new(),
        targets: Vec::new(),
    }
}

#[tokio::test]
async fn a_pinned_toolchain_in_the_store_needs_no_download() {
    let store = tempfile::tempdir().unwrap();
    // Nothing listens there, so a download would fail the test.
    let config = config(store.path(), "http://127.0.0.1:9");
    let request = request("1.90.0");
    let dir =
        toolchain_dir(&config.store_dir.root().join("rust"), &request.channel, HOST, &request);
    fs::create_dir_all(&dir).unwrap();

    let installed = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &request,
        HOST,
    )
    .await
    .unwrap();

    assert_eq!(installed, InstalledToolchain { dir });
}

#[tokio::test]
async fn offline_a_missing_toolchain_is_an_error() {
    let store = tempfile::tempdir().unwrap();
    let mut config = config(store.path(), "http://127.0.0.1:9");
    config.offline = true;

    let error = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &request("stable"),
        HOST,
    )
    .await
    .expect_err("nothing is installed");

    assert!(matches!(error, RustToolchainError::Offline { .. }), "{error:?}");
}

#[tokio::test]
async fn downloads_from_the_mirror_and_checks_the_signed_hash() {
    let mut server = mockito::Server::new_async().await;
    let manifest = server
        .mock("GET", "/dist/channel-rust-1.8.0.toml")
        .with_body(include_bytes!("fixtures/channel-rust-1.8.0.toml"))
        .expect(1)
        .create_async()
        .await;
    let signature = server
        .mock("GET", "/dist/channel-rust-1.8.0.toml.asc")
        .with_body(include_bytes!("fixtures/channel-rust-1.8.0.toml.asc"))
        .expect(1)
        .create_async()
        .await;
    let rustc = server
        .mock("GET", "/dist/2016-04-12/rustc-1.8.0-x86_64-unknown-linux-gnu.tar.gz")
        .with_body("not the archive the manifest names")
        .expect(1)
        .create_async()
        .await;
    let store = tempfile::tempdir().unwrap();
    let config = config(store.path(), &server.url());

    let error = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &request("1.8.0"),
        HOST,
    )
    .await
    .expect_err("the archive does not match the manifest");

    assert!(matches!(error, RustToolchainError::IntegrityMismatch { .. }), "{error:?}");
    manifest.assert_async().await;
    signature.assert_async().await;
    rustc.assert_async().await;
    assert_eq!(fs::read_dir(config.store_dir.root().join("rust")).unwrap().count(), 0);
}

#[test]
fn links_the_toolchain_beside_the_toolchain_file() {
    let checkout = tempfile::tempdir().unwrap();
    let member = checkout.path().join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(checkout.path().join("rust-toolchain.toml"), "stable").unwrap();
    assert_eq!(linked_bin_dir(&member, checkout.path()), None);

    let bin_dir = checkout.path().join(".pnpm/rust/bin");
    fs::create_dir_all(&bin_dir).unwrap();
    assert_eq!(linked_bin_dir(&member, checkout.path()), Some(bin_dir));
}

#[tokio::test]
async fn a_moving_channel_resolved_today_needs_no_download() {
    let store = tempfile::tempdir().unwrap();
    // Nothing listens there, so a download would fail the test.
    let config = config(store.path(), "http://127.0.0.1:9");
    let request = request("stable");
    let toolchains = config.store_dir.root().join("rust");
    let pinned = Channel::parse("1.95.0").unwrap();
    let dir = toolchain_dir(&toolchains, &pinned, HOST, &request);
    fs::create_dir_all(&dir).unwrap();
    record_resolution(&toolchains, HOST, &request, &pinned);

    let installed = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &request,
        HOST,
    )
    .await
    .unwrap();

    assert_eq!(installed, InstalledToolchain { dir });
}
