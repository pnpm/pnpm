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
fn only_a_link_to_the_toolchain_the_file_asks_for_is_taken() {
    let checkout = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let config = config(store.path(), "http://127.0.0.1:9");
    let member = checkout.path().join("crates/member");
    fs::create_dir_all(&member).unwrap();
    let toolchain_file = checkout.path().join("rust-toolchain.toml");
    fs::write(&toolchain_file, "1.95.0").unwrap();
    assert_eq!(linked_bin_dir(&config, &member, checkout.path()), None);

    let committed = checkout.path().join(".pnpm/rust/bin");
    fs::create_dir_all(&committed).unwrap();
    assert_eq!(linked_bin_dir(&config, &member, checkout.path()), None);

    let host = super::host::host_triple().unwrap();
    let mut request = request("1.95.0");
    request.profile = Profile::Default;
    let toolchain =
        toolchain_dir(&config.store_dir.root().join("rust"), &request.channel, &host, &request);
    fs::create_dir_all(toolchain.join("bin")).unwrap();
    fs::remove_dir_all(checkout.path().join(".pnpm/rust")).unwrap();
    pnpm_fs::force_symlink_dir(&toolchain, &checkout.path().join(".pnpm/rust")).unwrap();
    let installed = Some(dunce::canonicalize(toolchain.join("bin")).unwrap());
    assert_eq!(linked_bin_dir(&config, &member, checkout.path()), installed);

    fs::write(&toolchain_file, "stable").unwrap();
    assert_eq!(linked_bin_dir(&config, &member, checkout.path()), installed);

    for changed in [
        "1.96.0",
        "[toolchain]\npath = \"/opt/rust\"\n",
        "[toolchain]\nchannel = \"1.95.0\"\nprofile = \"minimal\"\n",
    ] {
        fs::write(&toolchain_file, changed).unwrap();
        assert_eq!(linked_bin_dir(&config, &member, checkout.path()), None, "{changed}");
    }
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

#[tokio::test]
async fn a_moving_channel_does_not_move_back_to_an_older_release() {
    let mut server = mockito::Server::new_async().await;
    let _manifest = server
        .mock("GET", "/dist/channel-rust-stable.toml")
        .with_body(include_bytes!("fixtures/channel-rust-1.8.0.toml"))
        .create_async()
        .await;
    let _signature = server
        .mock("GET", "/dist/channel-rust-stable.toml.asc")
        .with_body(include_bytes!("fixtures/channel-rust-1.8.0.toml.asc"))
        .create_async()
        .await;
    let store = tempfile::tempdir().unwrap();
    let config = config(store.path(), &server.url());
    let request = request("stable");
    let toolchains = config.store_dir.root().join("rust");
    record_resolution(&toolchains, HOST, &request, &Channel::parse("1.95.0").unwrap());
    let record = toolchains.join(".channels");
    let old = std::time::SystemTime::now() - std::time::Duration::from_hours(48);
    for entry in fs::read_dir(&record).unwrap() {
        fs::File::options()
            .write(true)
            .open(entry.unwrap().path())
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    let error = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &request,
        HOST,
    )
    .await
    .expect_err("1.8.0 is older than 1.95.0");

    assert!(matches!(error, RustToolchainError::OlderRelease { .. }), "{error:?}");
}

#[tokio::test]
async fn an_unreachable_server_falls_back_to_the_newest_installed_release() {
    let store = tempfile::tempdir().unwrap();
    // Nothing listens there.
    let config = config(store.path(), "http://127.0.0.1:9");
    let toolchains = config.store_dir.root().join("rust");
    let stable = request("stable");
    let installed = toolchain_dir(&toolchains, &Channel::parse("1.95.0").unwrap(), HOST, &stable);
    fs::create_dir_all(&installed).unwrap();

    let toolchain = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &stable,
        HOST,
    )
    .await
    .unwrap();
    assert_eq!(toolchain, InstalledToolchain { dir: installed });
    // The fallback is used for a while without asking the server again.
    assert_eq!(
        super::install::recent_resolution(&toolchains, HOST, &stable),
        Some(toolchain.dir.clone()),
    );

    let error = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &request("1.96.0"),
        HOST,
    )
    .await
    .expect_err("a pinned release that is not installed needs the server");
    assert!(matches!(error, RustToolchainError::Network { .. }), "{error:?}");
}

#[tokio::test]
async fn the_fallback_does_not_move_a_channel_back() {
    let store = tempfile::tempdir().unwrap();
    let config = config(store.path(), "http://127.0.0.1:9");
    let toolchains = config.store_dir.root().join("rust");
    let stable = request("stable");
    fs::create_dir_all(toolchain_dir(
        &toolchains,
        &Channel::parse("1.95.0").unwrap(),
        HOST,
        &stable,
    ))
    .unwrap();
    record_resolution(&toolchains, HOST, &stable, &Channel::parse("1.96.0").unwrap());
    let record = toolchains.join(".channels");
    let old = std::time::SystemTime::now() - std::time::Duration::from_hours(48);
    for entry in fs::read_dir(&record).unwrap() {
        fs::File::options()
            .write(true)
            .open(entry.unwrap().path())
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    let error = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &stable,
        HOST,
    )
    .await
    .expect_err("1.95.0 is older than the 1.96.0 the channel resolved to");
    assert!(matches!(error, RustToolchainError::Network { .. }), "{error:?}");
}

#[tokio::test]
async fn a_server_that_answers_with_a_client_error_is_reported() {
    let mut server = mockito::Server::new_async().await;
    let _manifest = server
        .mock("GET", "/dist/channel-rust-stable.toml")
        .with_status(404)
        .create_async()
        .await;
    let store = tempfile::tempdir().unwrap();
    let config = config(store.path(), &server.url());
    let toolchains = config.store_dir.root().join("rust");
    let stable = request("stable");
    fs::create_dir_all(toolchain_dir(
        &toolchains,
        &Channel::parse("1.95.0").unwrap(),
        HOST,
        &stable,
    ))
    .unwrap();

    let error = install_for_host::<SilentReporter>(
        &config,
        &ThrottledClient::new_for_installs(),
        &stable,
        HOST,
    )
    .await
    .expect_err("a mirror missing the channel is a misconfiguration");
    assert!(matches!(error, RustToolchainError::StatusNotOk { status: 404, .. }), "{error:?}");
}
