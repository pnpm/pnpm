//! End-to-end tests for the agent client against a real pnpr server.
//!
//! Topology: a shared [`TestRegistry`] serves the package fixtures; a
//! per-test in-process `pnpr` hosts the `/-/pnpr` handshake +
//! `/v1/install` + `/v1/files` endpoints. The client sends the registry
//! it wants resolved from, so the pnpr server's *own* uplink is left at
//! the default — proving resolution uses the client-supplied registry.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use pacquet_agent_client::{AgentClient, InstallOptions};
use pacquet_store_dir::StoreDir;
use pacquet_testing_utils::registry::TestRegistry;
use tempfile::TempDir;
use tokio::net::TcpListener;

/// Start an in-process pnpr with the fast-path endpoints. Returns the
/// base URL and the storage guard.
async fn start_pnpr() -> (String, TempDir) {
    let storage = TempDir::new().expect("pnpr storage tempdir");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind pnpr");
    let addr = listener.local_addr().expect("pnpr addr");

    let mut config = pnpr::Config::proxy(addr, storage.path().to_path_buf());
    config.public_url = format!("http://{addr}");

    tokio::spawn(async move {
        let _ = pnpr::serve_listener(config, listener).await;
    });

    wait_until_ready(addr).await;
    (format!("http://{addr}/"), storage)
}

async fn wait_until_ready(addr: SocketAddr) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("pnpr server never became ready at {addr}");
}

fn deps<const COUNT: usize>(entries: [(&str, &str); COUNT]) -> BTreeMap<String, String> {
    entries.into_iter().map(|(name, range)| (name.to_string(), range.to_string())).collect()
}

fn options<'a>(
    store: &'a StoreDir,
    registry: &str,
    dependencies: BTreeMap<String, String>,
) -> InstallOptions<'a> {
    InstallOptions {
        store_dir: store,
        dependencies,
        dev_dependencies: BTreeMap::new(),
        registry: registry.to_string(),
        named_registries: BTreeMap::new(),
        overrides: None,
        minimum_release_age: None,
    }
}

#[tokio::test]
async fn resolves_and_downloads_a_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = AgentClient::new(pnpr_url);

    let outcome = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "lockfile should contain @foo/no-deps@1.0.0, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );

    assert!(outcome.stats.total_packages >= 1);
    assert!(outcome.stats.packages_to_fetch >= 1, "first run should fetch the package");
    assert!(outcome.files_written >= 1, "at least package.json should be written");
    assert!(outcome.index_entries_written >= 1, "the package's index entry should be written");

    let store_keys = pacquet_store_dir::StoreIndex::open_readonly_in(&store)
        .expect("open client index")
        .keys()
        .expect("read keys");
    assert!(
        store_keys.iter().any(|key| key.contains("@foo/no-deps@1.0.0")),
        "client store index should hold the package, got: {store_keys:?}",
    );
}

#[tokio::test]
async fn warm_store_skips_already_present_files() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = AgentClient::new(pnpr_url);

    let cold = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("cold install");
    assert!(cold.files_written >= 1);

    let warm = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("warm install");

    assert!(warm.stats.already_in_store >= 1, "package should be recognized as cached");
    assert_eq!(warm.files_written, 0, "warm run should download no files");
    assert_eq!(warm.index_entries_written, 0, "warm run should write no index entries");
}

#[tokio::test]
async fn resolves_a_multi_file_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = AgentClient::new(pnpr_url);

    let outcome = client
        .install(options(
            &store,
            &registry.url(),
            deps([("@pnpm.e2e/hello-world-js-bin", "1.0.0")]),
        ))
        .await
        .expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages
            .keys()
            .any(|key| key.to_string().starts_with("@pnpm.e2e/hello-world-js-bin@1.0.0")),
    );
    assert!(outcome.files_written >= 2, "expected multiple files, got {}", outcome.files_written);
}

#[tokio::test]
async fn handshake_rejects_a_non_pnpr_server() {
    // A plain registry has no `/-/pnpr` route and 404s the handshake.
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/-/pnpr").with_status(404).create_async().await;

    let client = AgentClient::new(server.url());
    let err = client.handshake().await.expect_err("a non-pnpr server should be rejected");
    assert!(err.to_string().contains("not a pnpr server"), "got: {err}");
    mock.assert_async().await;
}
