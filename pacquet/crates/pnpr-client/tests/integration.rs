//! End-to-end tests for the pnpr client against a real pnpr server.
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

use pacquet_pnpr_client::{InstallOptions, PnprClient, PnprClientError};
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

/// The nerf-darted key (`//host[:port]/path/`) a forwarded credential for
/// `url` is keyed by, mirroring `AuthHeaders`' lookup on the server —
/// keeping any registry path prefix so the key isn't wrong for one.
fn nerf_key(url: &str) -> String {
    let authority_and_path = url.split("://").nth(1).unwrap_or(url);
    let (authority, path) = authority_and_path.split_once('/').unwrap_or((authority_and_path, ""));
    let path = path.split(['?', '#']).next().unwrap_or("").trim_matches('/');
    if path.is_empty() { format!("//{authority}/") } else { format!("//{authority}/{path}/") }
}

/// Register a user with the shared test registry and return its bearer
/// token, so a test can forward it as the caller's upstream credential.
async fn register_token(registry_url: &str, username: &str) -> String {
    let body = serde_json::json!({ "name": username, "password": "password123" });
    let response = reqwest::Client::new()
        .put(format!("{registry_url}-/user/org.couchdb.user:{username}"))
        .json(&body)
        .send()
        .await
        .expect("adduser request");
    assert!(response.status().is_success(), "adduser returned {}", response.status());
    let json: serde_json::Value = response.json().await.expect("adduser response json");
    json["token"].as_str().expect("token in adduser response").to_string()
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
        auth_headers: BTreeMap::new(),
        authorization: None,
        overrides: None,
        lockfile: None,
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
        lockfile_only: false,
        trust_lockfile: false,
        minimum_release_age: None,
        minimum_release_age_exclude: None,
        minimum_release_age_ignore_missing_time: true,
        trust_policy: pacquet_config::TrustPolicy::Off,
        trust_policy_exclude: None,
        trust_policy_ignore_after: None,
    }
}

/// The forwarded per-registry credentials and the pnpr-server identity
/// header must travel on the wire: `authHeaders` in the body (so the
/// server resolves/fetches private content as the caller) and
/// `Authorization` on the request (so pnpr's gate + grant table key on
/// the right user). A `mockito` server captures the request and asserts
/// both are present; the canned 500 just short-circuits the client after
/// the match.
#[tokio::test]
async fn forwards_credentials_and_the_identity_header() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/install")
        .match_header("authorization", "Bearer pnpr-token")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"authHeaders":{"//npm.acme.test/":"Bearer upstream-token"}}"#.to_string(),
        ))
        .with_status(500)
        .with_body("stop")
        .create_async()
        .await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(format!("{}/", server.url()));

    let mut opts = options(&store, "https://npm.acme.test/", deps([("@acme/foo", "1.0.0")]));
    opts.auth_headers = deps([("//npm.acme.test/", "Bearer upstream-token")]);
    opts.authorization = Some("Bearer pnpr-token".to_string());

    let result = client.install(opts).await;
    assert!(result.is_err(), "the canned 500 should surface as an error");
    mock.assert_async().await;
}

/// End-to-end: the test registry gates `@pnpm.e2e/needs-auth` behind
/// `$authenticated`, so resolving it through the accelerator only works
/// when the caller's upstream token is forwarded and the server fetches
/// the packument + tarball as the caller.
#[tokio::test]
async fn a_forwarded_credential_resolves_a_private_package() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-forwarder").await;
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    let mut opts = options(&store, &registry.url(), deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let mut auth = BTreeMap::new();
    auth.insert(nerf_key(&registry.url()), format!("Bearer {token}"));
    opts.auth_headers = auth;

    let mut outcome = client.install(opts).await.expect("forwarded credential should resolve it");
    outcome.finish_index_writes().await;
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@pnpm.e2e/needs-auth@1.0.0")),
        "lockfile should contain the authed package, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
    assert!(outcome.files_written >= 1, "its files should be materialized");
}

/// The same install without a forwarded credential fails: the registry
/// won't serve the gated packument anonymously, so resolution can't read
/// it.
#[tokio::test]
async fn a_private_package_fails_without_a_forwarded_credential() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    let opts = options(&store, &registry.url(), deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let Err(PnprClientError::Server(message)) = client.install(opts).await else {
        panic!("expected the gated install to fail with a server error");
    };
    assert!(
        message.contains("401"),
        "expected an auth denial without a forwarded credential, got: {message}",
    );
}

#[tokio::test]
async fn resolves_and_downloads_a_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    let mut outcome = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("install should succeed");
    outcome.finish_index_writes().await;

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
async fn lockfile_only_resolves_without_fetching_files() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    // `--lockfile-only`: the server resolves and returns the lockfile but
    // fetches nothing and serves no files, so the client store stays
    // empty. Mirrors pnpm's resolve + write, fetch nothing, link nothing.
    let mut opts = options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile_only = true;
    let mut outcome = client.install(opts).await.expect("lockfile-only install should succeed");
    outcome.finish_index_writes().await;

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "lockfile should still contain @foo/no-deps@1.0.0",
    );
    assert_eq!(outcome.files_written, 0, "lockfile-only should download no files");
    assert_eq!(outcome.index_entries_written, 0, "lockfile-only should write no index entries");
    assert!(
        pacquet_store_dir::StoreIndex::open_readonly_in(&store)
            .map(|index| index.keys().unwrap_or_default().is_empty())
            .unwrap_or(true),
        "client store index should stay empty after a lockfile-only install",
    );
}

#[tokio::test]
async fn warm_store_skips_already_present_files() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    let mut cold = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("cold install");
    cold.finish_index_writes().await;
    assert!(cold.files_written >= 1);

    let mut warm = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("warm install");
    warm.finish_index_writes().await;

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
    let client = PnprClient::new(pnpr_url);

    let mut outcome = client
        .install(options(
            &store,
            &registry.url(),
            deps([("@pnpm.e2e/hello-world-js-bin", "1.0.0")]),
        ))
        .await
        .expect("install should succeed");
    outcome.finish_index_writes().await;

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages
            .keys()
            .any(|key| key.to_string().starts_with("@pnpm.e2e/hello-world-js-bin@1.0.0")),
    );
    assert!(outcome.files_written >= 2, "expected multiple files, got {}", outcome.files_written);
}

#[tokio::test]
async fn verifies_and_accepts_a_clean_input_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    // A first install with no lockfile produces a valid resolved one.
    let mut first = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");
    first.finish_index_writes().await;

    // Sending it back as the input lockfile makes the server verify it
    // under the (default, policy-free) client policy before resolving;
    // a clean lockfile passes and the install succeeds.
    let mut opts = options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    let mut second = client.install(opts).await.expect("verified-input install should succeed");
    second.finish_index_writes().await;
    assert!(second.lockfile.packages.is_some(), "resolution still produced a lockfile");
}

#[tokio::test]
async fn rejects_an_input_lockfile_that_violates_the_clients_policy() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    let mut first = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");
    first.finish_index_writes().await;

    // Re-send the same lockfile under a ~100-year minimumReleaseAge: no
    // real publish time can satisfy it, so the server rejects the input
    // lockfile and the client rebuilds the identical `VerifyError`.
    let mut opts = options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;

    let Err(PnprClientError::Verification(verify_err)) = client.install(opts).await else {
        panic!("expected a verification error rejecting the input lockfile");
    };
    assert!(
        verify_err.to_string().contains("minimumReleaseAge"),
        "expected a minimumReleaseAge breakdown, got: {verify_err}",
    );
}

#[tokio::test]
async fn trust_lockfile_makes_the_server_skip_verification() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client_store = TempDir::new().unwrap();
    let store = StoreDir::new(client_store.path().to_path_buf());
    let client = PnprClient::new(pnpr_url);

    let mut first = client
        .install(options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");
    first.finish_index_writes().await;

    // Same policy that `rejects_an_input_lockfile_that_violates_the_clients_policy`
    // trips on, but with the client's `trustLockfile` opt-out set: the
    // server must skip the verify gate and resolve normally, matching the
    // local `--trust-lockfile` path.
    let mut opts = options(&store, &registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;
    opts.trust_lockfile = true;

    let mut outcome = client.install(opts).await.expect("trustLockfile should skip verification");
    outcome.finish_index_writes().await;
    assert!(outcome.lockfile.packages.is_some(), "install still resolved a lockfile");
}

#[tokio::test]
async fn handshake_rejects_a_non_pnpr_server() {
    // A plain registry has no `/-/pnpr` route and 404s the handshake.
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/-/pnpr").with_status(404).create_async().await;

    let client = PnprClient::new(server.url());
    let err = client.handshake().await.expect_err("a non-pnpr server should be rejected");
    assert!(err.to_string().contains("not a pnpr server"), "got: {err}");
    mock.assert_async().await;
}
