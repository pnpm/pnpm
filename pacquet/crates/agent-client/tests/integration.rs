//! End-to-end tests for the agent client against a real pnpr agent
//! server.
//!
//! Topology: a shared [`TestRegistry`] serves the package fixtures; a
//! per-test in-process `pnpr` instance hosts the `/v1/install` +
//! `/v1/files` endpoints and resolves from that registry; the client
//! under test talks to the `pnpr` instance.

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

/// Start an in-process pnpr with the agent endpoints, resolving from
/// `upstream`. Returns the base URL and the storage guard (dropped at
/// the end of the test to clean the agent's store).
async fn start_agent(upstream: &str) -> (String, TempDir) {
    let storage = TempDir::new().expect("agent storage tempdir");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind agent");
    let addr = listener.local_addr().expect("agent addr");

    let mut config = pnpr::Config::proxy(addr, storage.path().to_path_buf());
    // The agent derives its resolve registry from the first uplink.
    config.uplinks.get_mut("npmjs").expect("default npmjs uplink").url =
        upstream.trim_end_matches('/').to_string();
    config.public_url = format!("http://{addr}");

    tokio::spawn(async move {
        let _ = pnpr::serve_listener(config, listener).await;
    });

    let base_url = format!("http://{addr}/");
    wait_until_ready(addr).await;
    (base_url, storage)
}

async fn wait_until_ready(addr: SocketAddr) {
    for _ in 0..200 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("agent server never became ready at {addr}");
}

fn deps<const COUNT: usize>(entries: [(&str, &str); COUNT]) -> BTreeMap<String, String> {
    entries.into_iter().map(|(name, range)| (name.to_string(), range.to_string())).collect()
}

#[tokio::test]
async fn resolves_and_downloads_a_package() {
    let registry = TestRegistry::start();
    let (agent_url, _storage) = start_agent(&registry.url()).await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = AgentClient::new(agent_url);

    let outcome = client
        .install(InstallOptions {
            store_dir: &store,
            dependencies: deps([("@foo/no-deps", "1.0.0")]),
            dev_dependencies: BTreeMap::new(),
        })
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

    // The forwarded index entry is now readable from the client store.
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
    let (agent_url, _storage) = start_agent(&registry.url()).await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = AgentClient::new(agent_url);

    let cold = client
        .install(InstallOptions {
            store_dir: &store,
            dependencies: deps([("@foo/no-deps", "1.0.0")]),
            dev_dependencies: BTreeMap::new(),
        })
        .await
        .expect("cold install");
    assert!(cold.files_written >= 1);

    // Second run against the now-warm store: the client reports its
    // integrities, the server marks the package already-present, and
    // nothing is downloaded.
    let warm = client
        .install(InstallOptions {
            store_dir: &store,
            dependencies: deps([("@foo/no-deps", "1.0.0")]),
            dev_dependencies: BTreeMap::new(),
        })
        .await
        .expect("warm install");

    assert!(warm.stats.already_in_store >= 1, "package should be recognized as cached");
    assert_eq!(warm.files_written, 0, "warm run should download no files");
    assert_eq!(warm.index_entries_written, 0, "warm run should write no index entries");
}

#[tokio::test]
async fn resolves_a_multi_file_package() {
    let registry = TestRegistry::start();
    let (agent_url, _storage) = start_agent(&registry.url()).await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = AgentClient::new(agent_url);

    let outcome = client
        .install(InstallOptions {
            store_dir: &store,
            dependencies: deps([("@pnpm.e2e/hello-world-js-bin", "1.0.0")]),
            dev_dependencies: BTreeMap::new(),
        })
        .await
        .expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages
            .keys()
            .any(|key| key.to_string().starts_with("@pnpm.e2e/hello-world-js-bin@1.0.0")),
    );
    // index.js + package.json at least.
    assert!(outcome.files_written >= 2, "expected multiple files, got {}", outcome.files_written);
}
