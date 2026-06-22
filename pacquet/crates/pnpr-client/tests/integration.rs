//! End-to-end tests for the pnpr client against a real pnpr server.
//!
//! Topology: a shared [`TestRegistry`] serves the package fixtures; a
//! per-test in-process `pnpr` hosts the `/-/pnpr` handshake +
//! `/-/pnpr/v0/resolve` endpoints. The client sends the registry it wants
//! resolved from, so the pnpr server's *own* uplink is left at the
//! default — proving resolution uses the client-supplied registry. pnpr
//! serves no file content; the client receives only the resolved
//! lockfile.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use pacquet_network::{AuthHeadersByScope, DEFAULT_REGISTRY_SCOPE};
use pacquet_pnpr_client::{PnprClient, PnprClientError, ResolveOptions, VerifyLockfileOptions};
use pacquet_testing_utils::registry::TestRegistry;
use tempfile::TempDir;
use tokio::net::TcpListener;

/// Basic auth for `pnpr-client:password123`, registered by [`start_pnpr`].
const PNPR_AUTHORIZATION: &str = "Basic cG5wci1jbGllbnQ6cGFzc3dvcmQxMjM=";

/// Start an in-process pnpr with the fast-path endpoints. Returns the
/// base URL and the storage guard.
async fn start_pnpr() -> (String, TempDir) {
    let storage = TempDir::new().expect("pnpr storage tempdir");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind pnpr");
    let addr = listener.local_addr().expect("pnpr addr");

    let mut config = pnpr::Config::proxy(addr, storage.path().to_path_buf());
    config.public_url = format!("http://{addr}");
    config.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;

    tokio::spawn(async move {
        let _ = pnpr::serve_listener(config, listener).await;
    });

    wait_until_ready(addr).await;
    let base_url = format!("http://{addr}/");
    let _ = register_token(&base_url, "pnpr-client").await;
    (base_url, storage)
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

fn auth_headers<const COUNT: usize>(entries: [(&str, &str, &str); COUNT]) -> AuthHeadersByScope {
    let mut result = AuthHeadersByScope::new();
    for (uri, scope, value) in entries {
        result.entry(uri.to_string()).or_default().insert(scope.to_string(), value.to_string());
    }
    result
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

/// Register a user with an npm-compatible registry and return its bearer
/// token. The pnpr fixture reuses the account through Basic auth; registry
/// tests forward the token as an upstream credential.
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

fn options(registry: &str, dependencies: BTreeMap<String, String>) -> ResolveOptions {
    ResolveOptions {
        dependencies,
        dev_dependencies: BTreeMap::new(),
        optional_dependencies: BTreeMap::new(),
        registry: registry.to_string(),
        named_registries: BTreeMap::new(),
        auth_headers: BTreeMap::new(),
        authorization: Some(PNPR_AUTHORIZATION.to_string()),
        overrides: None,
        lockfile: None,
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        ignore_manifest_check: false,
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
/// server resolves private content as the caller) and `Authorization` on
/// the request (so pnpr identifies the caller). A `mockito` server
/// captures the request and asserts both are present; the canned 500 just
/// short-circuits the client after the match.
#[tokio::test]
async fn forwards_credentials_and_the_identity_header() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_header("authorization", "Bearer pnpr-token")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"authHeaders":{"//npm.acme.test/":{"@":"Bearer upstream-token"}}}"#.to_string(),
        ))
        .with_status(500)
        .with_body("stop")
        .create_async()
        .await;

    let client = PnprClient::new(format!("{}/", server.url()));

    let mut opts = options("https://npm.acme.test/", deps([("@acme/foo", "1.0.0")]));
    opts.auth_headers =
        auth_headers([("//npm.acme.test/", DEFAULT_REGISTRY_SCOPE, "Bearer upstream-token")]);
    opts.authorization = Some("Bearer pnpr-token".to_string());

    let result = client.resolve(opts).await;
    assert!(result.is_err(), "the canned 500 should surface as an error");
    mock.assert_async().await;
}

/// End-to-end: the test registry gates `@pnpm.e2e/needs-auth` behind
/// `$authenticated`, so resolving it through the resolver only works
/// when the caller's upstream token is forwarded and the server fetches
/// the packument as the caller.
#[tokio::test]
async fn a_forwarded_credential_resolves_a_private_package() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-forwarder").await;
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let mut opts = options(&registry.url(), deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let registry_key = nerf_key(&registry.url());
    let bearer = format!("Bearer {token}");
    opts.auth_headers =
        auth_headers([(registry_key.as_str(), DEFAULT_REGISTRY_SCOPE, bearer.as_str())]);

    let outcome = client.resolve(opts).await.expect("forwarded credential should resolve it");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@pnpm.e2e/needs-auth@1.0.0")),
        "lockfile should contain the authed package, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

/// The same install without a forwarded credential fails: the registry
/// won't serve the gated packument anonymously, so resolution can't read
/// it.
#[tokio::test]
async fn a_private_package_fails_without_a_forwarded_credential() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let Err(PnprClientError::Server(message)) = client.resolve(opts).await else {
        panic!("expected the gated install to fail with a server error");
    };
    assert!(
        message.contains("401"),
        "expected an auth denial without a forwarded credential, got: {message}",
    );
}

#[tokio::test]
async fn resolves_a_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let outcome = client
        .resolve(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "lockfile should contain @foo/no-deps@1.0.0, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );

    assert!(outcome.stats.total_packages >= 1);
}

/// The streaming API surfaces each resolved tarball as a `package`
/// frame *before* the terminal `done` frame carrying the lockfile, and
/// every streamed package appears in the final lockfile. This is the
/// overlap lever: the caller can begin fetching each tarball the moment
/// its frame arrives, while the server is still resolving.
#[tokio::test]
async fn streams_resolved_packages_before_the_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let mut streamed: Vec<String> = Vec::new();
    let outcome = client
        .resolve_streaming(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])), |pkg| {
            assert!(!pkg.integrity.is_empty(), "a package frame carries an integrity");
            assert!(pkg.tarball.starts_with("http"), "a package frame carries a tarball URL");
            assert_eq!(pkg.id, format!("{}@{}", pkg.name, pkg.version), "id is name@version");
            streamed.push(pkg.id);
        })
        .await
        .expect("streaming resolve should succeed");

    assert!(!streamed.is_empty(), "at least one package frame streams before `done`");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    for id in &streamed {
        assert!(
            packages.keys().any(|key| key.to_string() == *id),
            "streamed package {id} should appear in the resolved lockfile, got: {:?}",
            packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
        );
    }
}

/// Optional dependencies must reach the server in the request, not be
/// silently dropped, so the resolved lockfile includes their edges.
#[tokio::test]
async fn forwards_optional_dependencies() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let mut opts = options(&registry.url(), BTreeMap::new());
    opts.optional_dependencies = deps([("@foo/no-deps", "1.0.0")]);

    let outcome = client.resolve(opts).await.expect("install should succeed");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "the optional dependency should be resolved into the lockfile, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

#[tokio::test]
async fn verifies_and_accepts_a_clean_input_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    // A first install with no lockfile produces a valid resolved one.
    let first = client
        .resolve(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Sending it back as the input lockfile makes the server verify it
    // under the (default, policy-free) client policy before resolving;
    // a clean lockfile passes and the install succeeds.
    let mut opts = options(&registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    let second = client.resolve(opts).await.expect("verified-input install should succeed");
    assert!(second.lockfile.packages.is_some(), "resolution still produced a lockfile");
}

#[tokio::test]
async fn rejects_an_input_lockfile_that_violates_the_clients_policy() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Re-send the same lockfile under a ~100-year minimumReleaseAge: no
    // real publish time can satisfy it, so the server rejects the input
    // lockfile and the client rebuilds the identical `VerifyError`.
    let mut opts = options(&registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;

    let Err(PnprClientError::Verification(verify_err)) = client.resolve(opts).await else {
        panic!("expected a verification error rejecting the input lockfile");
    };
    assert!(
        verify_err.to_string().contains("minimumReleaseAge"),
        "expected a minimumReleaseAge breakdown, got: {verify_err}",
    );
}

#[tokio::test]
async fn verify_lockfile_endpoint_accepts_a_clean_input_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    let mut opts = options(&registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile);
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&opts).expect("lockfile is present");

    client.verify_lockfile(verify_opts).await.expect("lockfile should verify");
}

#[tokio::test]
async fn verify_lockfile_endpoint_rejects_policy_violation() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    let mut opts = options(&registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile);
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&opts).expect("lockfile is present");

    let Err(PnprClientError::Verification(verify_err)) = client.verify_lockfile(verify_opts).await
    else {
        panic!("expected a verification error rejecting the input lockfile");
    };
    assert!(
        verify_err.to_string().contains("minimumReleaseAge"),
        "expected a minimumReleaseAge breakdown, got: {verify_err}",
    );
}

/// The verification fan-out fetches each entry's packument, so a gated
/// package verifies only when `/-/pnpr/v0/verify-lockfile` forwards the
/// client's credential map — and fails closed when it doesn't. Each
/// verify call targets a fresh pnpr so neither the whole-lockfile
/// verdict cache nor the metadata mirror warmed by an earlier call can
/// satisfy it without exercising the forwarded credential.
#[tokio::test]
async fn verify_lockfile_endpoint_forwards_credentials() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-verifier").await;
    let (resolve_pnpr_url, _resolve_storage) = start_pnpr().await;

    let mut resolve_opts = options(&registry.url(), deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let registry_key = nerf_key(&registry.url());
    let bearer = format!("Bearer {token}");
    resolve_opts.auth_headers =
        auth_headers([(registry_key.as_str(), DEFAULT_REGISTRY_SCOPE, bearer.as_str())]);
    let first = PnprClient::new(resolve_pnpr_url)
        .resolve(resolve_opts.clone())
        .await
        .expect("authed install");

    // An active policy makes the verifier fetch the gated packument.
    resolve_opts.lockfile = Some(first.lockfile);
    resolve_opts.minimum_release_age = Some(1);
    resolve_opts.minimum_release_age_ignore_missing_time = false;
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&resolve_opts).expect("lockfile is present");

    let (authed_pnpr_url, _authed_storage) = start_pnpr().await;
    PnprClient::new(authed_pnpr_url)
        .verify_lockfile(verify_opts)
        .await
        .expect("forwarded credential should let the gated entry verify");

    let mut anonymous_opts = resolve_opts.clone();
    anonymous_opts.auth_headers = BTreeMap::new();
    let anonymous_verify_opts =
        VerifyLockfileOptions::from_resolve_options(&anonymous_opts).expect("lockfile is present");
    let (anonymous_pnpr_url, _anonymous_storage) = start_pnpr().await;
    assert!(
        PnprClient::new(anonymous_pnpr_url).verify_lockfile(anonymous_verify_opts).await.is_err(),
        "without the credential the gated entry's metadata fetch must fail closed",
    );
}

#[tokio::test]
async fn trust_lockfile_makes_the_server_skip_verification() {
    let registry = TestRegistry::start();
    let (pnpr_url, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Same policy that `rejects_an_input_lockfile_that_violates_the_clients_policy`
    // trips on, but with the client's `trustLockfile` opt-out set: the
    // server must skip the verify gate and resolve normally, matching the
    // local `--trust-lockfile` path.
    let mut opts = options(&registry.url(), deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    opts.minimum_release_age = Some(60 * 24 * 365 * 100);
    opts.minimum_release_age_ignore_missing_time = false;
    opts.trust_lockfile = true;

    let outcome = client.resolve(opts).await.expect("trustLockfile should skip verification");
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
