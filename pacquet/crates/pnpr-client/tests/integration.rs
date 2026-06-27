//! End-to-end tests for the pnpr client against a real pnpr server.
//!
//! Topology: a shared [`TestRegistry`] serves the package fixtures; a
//! per-test in-process `pnpr` hosts the `/-/pnpr` handshake +
//! `/-/pnpr/v0/resolve` endpoints. The client sends the registry it wants
//! resolved from, so the pnpr server's *own* uplink is left at the
//! default — proving resolution uses the client-supplied registry. pnpr
//! serves no file content; the client receives only the resolved
//! lockfile.
//!
//! The client authenticates to pnpr with a bearer token but never
//! forwards its own upstream registry credentials. Private upstream
//! content resolves only when the pnpr server is configured with an
//! upstream credential alias the caller is authorized to use.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use pacquet_pnpr_client::{PnprClient, PnprClientError, ResolveOptions, VerifyLockfileOptions};
use pacquet_testing_utils::registry::TestRegistry;
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

/// Start an in-process pnpr with the fast-path endpoints. Returns the
/// base URL, the bearer `Authorization` for the registered `pnpr-client`
/// caller (pnpr only honors `_authToken` on requests — the resolver
/// endpoints reject Basic credentials), and the storage guard.
async fn start_pnpr() -> (String, String, TempDir) {
    start_pnpr_with_aliases(Vec::new()).await
}

/// Like [`start_pnpr`] but registers operator-managed upstream credential
/// aliases, so the server can fetch private upstream content on behalf of
/// an authorized caller without the client forwarding any credential.
async fn start_pnpr_with_aliases(
    aliases: Vec<(String, pnpr::UpstreamAlias)>,
) -> (String, String, TempDir) {
    let storage = TempDir::new().expect("pnpr storage tempdir");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind pnpr");
    let addr = listener.local_addr().expect("pnpr addr");

    let mut config = pnpr::Config::proxy(addr, storage.path().to_path_buf());
    config.public_url = format!("http://{addr}");
    config.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;
    for (name, alias) in aliases {
        config.upstream_aliases.insert(name, alias);
    }

    tokio::spawn(async move {
        let _ = pnpr::serve_listener(config, listener).await;
    });

    wait_until_ready(addr).await;
    let base_url = format!("http://{addr}/");
    let token = register_token(&base_url, "pnpr-client").await;
    (base_url, format!("Bearer {token}"), storage)
}

/// An upstream credential alias that serves `registry_url` with `token`,
/// usable by any authenticated pnpr caller.
fn registry_alias(registry_url: &str, token: &str) -> (String, pnpr::UpstreamAlias) {
    (
        "test-registry".to_string(),
        pnpr::UpstreamAlias {
            registry: registry_url.to_string(),
            package: None,
            authorization: format!("Bearer {token}"),
            access: pnpr::AccessList::parse("$authenticated"),
            generation: 1,
        },
    )
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

/// Accept one HTTP request, return its full raw bytes (headers + body),
/// and answer `500` so the client stops after the capture.
async fn capture_one_request(listener: TcpListener) -> String {
    let (mut socket, _) = listener.accept().await.expect("accept request");
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = socket.read(&mut chunk).await.expect("read request");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        // Stop once the full body (per Content-Length) has arrived.
        let text = String::from_utf8_lossy(&buffer);
        if let Some(headers_end) = text.find("\r\n\r\n") {
            let content_length = text[..headers_end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if buffer.len() >= headers_end + 4 + content_length {
                break;
            }
        }
    }
    let _ = socket
        .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\nstop")
        .await;
    let _ = socket.shutdown().await;
    String::from_utf8_lossy(&buffer).into_owned()
}

fn deps<const COUNT: usize>(entries: [(&str, &str); COUNT]) -> BTreeMap<String, String> {
    entries.into_iter().map(|(name, range)| (name.to_string(), range.to_string())).collect()
}

/// Register a user with an npm-compatible registry and return its bearer
/// token. The pnpr fixture authenticates its own caller with this token;
/// an upstream alias uses one as its server-side upstream credential.
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

fn options(
    registry: &str,
    authorization: &str,
    dependencies: BTreeMap<String, String>,
) -> ResolveOptions {
    ResolveOptions {
        dependencies,
        dev_dependencies: BTreeMap::new(),
        optional_dependencies: BTreeMap::new(),
        registry: registry.to_string(),
        named_registries: BTreeMap::new(),
        authorization: Some(authorization.to_string()),
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

/// The request must identify the caller to pnpr (`Authorization`) but
/// must never carry the client's own upstream registry credentials in the
/// body — pnpr selects upstream auth from its route policy. A raw TCP
/// listener captures the wire bytes and asserts both invariants; the
/// canned 500 just short-circuits the client after the capture.
#[tokio::test]
async fn sends_the_identity_header_but_no_upstream_credentials() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind capture");
    let addr = listener.local_addr().expect("capture addr");
    let capture = tokio::spawn(capture_one_request(listener));

    let client = PnprClient::new(format!("http://{addr}/"));
    let opts =
        options("https://npm.acme.test/", "Bearer pnpr-token", deps([("@acme/foo", "1.0.0")]));
    let result = client.resolve(opts).await;
    assert!(result.is_err(), "the canned 500 should surface as an error");

    let request = capture.await.expect("capture task");
    assert!(
        request.to_lowercase().contains("authorization: bearer pnpr-token"),
        "the identity header must be sent, got:\n{request}",
    );
    assert!(
        !request.contains("authHeaders"),
        "the request body must not carry upstream credentials, got:\n{request}",
    );
}

/// End-to-end: the test registry gates `@pnpm.e2e/needs-auth` behind
/// `$authenticated`. The client never forwards its own credentials, so
/// resolving it works only when the pnpr server is configured with an
/// upstream credential alias for that registry that the caller is
/// authorized to use.
#[tokio::test]
async fn an_upstream_alias_resolves_a_private_package() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-forwarder").await;
    let (pnpr_url, pnpr_auth, _storage) =
        start_pnpr_with_aliases(vec![registry_alias(&registry.url(), &token)]).await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), &pnpr_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let outcome = client.resolve(opts).await.expect("the upstream alias should resolve it");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@pnpm.e2e/needs-auth@1.0.0")),
        "lockfile should contain the authed package, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

/// The same install against a server with no matching upstream alias
/// fails: the client forwards no credential and pnpr has none to select,
/// so the gated packument can only be fetched anonymously — which the
/// registry refuses.
#[tokio::test]
async fn a_private_package_fails_without_an_upstream_alias() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), &pnpr_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let Err(PnprClientError::Server(message)) = client.resolve(opts).await else {
        panic!("expected the gated install to fail with a server error");
    };
    assert!(
        message.contains("401"),
        "expected an auth denial without an upstream alias, got: {message}",
    );
}

#[tokio::test]
async fn resolves_a_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let outcome = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let mut streamed: Vec<String> = Vec::new();
    let outcome = client
        .resolve_streaming(
            options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])),
            |pkg| {
                assert!(!pkg.integrity.is_empty(), "a package frame carries an integrity");
                assert!(pkg.tarball.starts_with("http"), "a package frame carries a tarball URL");
                assert_eq!(pkg.id, format!("{}@{}", pkg.name, pkg.version), "id is name@version");
                streamed.push(pkg.id);
            },
        )
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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let mut opts = options(&registry.url(), &pnpr_auth, BTreeMap::new());
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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    // A first install with no lockfile produces a valid resolved one.
    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Sending it back as the input lockfile makes the server verify it
    // under the (default, policy-free) client policy before resolving;
    // a clean lockfile passes and the install succeeds.
    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile.clone());
    let second = client.resolve(opts).await.expect("verified-input install should succeed");
    assert!(second.lockfile.packages.is_some(), "resolution still produced a lockfile");
}

#[tokio::test]
async fn rejects_an_input_lockfile_that_violates_the_clients_policy() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Re-send the same lockfile under a ~100-year minimumReleaseAge: no
    // real publish time can satisfy it, so the server rejects the input
    // lockfile and the client rebuilds the identical `VerifyError`.
    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.lockfile = Some(first.lockfile);
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&opts).expect("lockfile is present");

    client.verify_lockfile(verify_opts).await.expect("lockfile should verify");
}

#[tokio::test]
async fn verify_lockfile_endpoint_rejects_policy_violation() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
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
/// package verifies only when the pnpr server has an upstream alias for
/// the registry — and fails closed against a server without one. Each
/// verify targets a fresh pnpr so neither the whole-lockfile verdict
/// cache nor the metadata mirror warmed by an earlier call can satisfy it
/// without exercising the alias.
#[tokio::test]
async fn verify_lockfile_endpoint_uses_upstream_aliases() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-verifier").await;

    let (resolve_pnpr_url, resolve_auth, _resolve_storage) =
        start_pnpr_with_aliases(vec![registry_alias(&registry.url(), &token)]).await;
    let mut resolve_opts =
        options(&registry.url(), &resolve_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let first = PnprClient::new(resolve_pnpr_url)
        .resolve(resolve_opts.clone())
        .await
        .expect("aliased install");

    // An active policy makes the verifier fetch the gated packument.
    resolve_opts.lockfile = Some(first.lockfile);
    resolve_opts.minimum_release_age = Some(1);
    resolve_opts.minimum_release_age_ignore_missing_time = false;

    // A fresh pnpr that carries the alias verifies the gated entry.
    let (aliased_pnpr_url, aliased_auth, _aliased_storage) =
        start_pnpr_with_aliases(vec![registry_alias(&registry.url(), &token)]).await;
    let mut aliased_opts = resolve_opts.clone();
    aliased_opts.authorization = Some(aliased_auth);
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&aliased_opts).expect("lockfile is present");
    PnprClient::new(aliased_pnpr_url)
        .verify_lockfile(verify_opts)
        .await
        .expect("the upstream alias should let the gated entry verify");

    // A pnpr without the alias has no credential to select, so the gated
    // entry's metadata fetch must fail closed.
    let (plain_pnpr_url, plain_auth, _plain_storage) = start_pnpr().await;
    let mut plain_opts = resolve_opts.clone();
    plain_opts.authorization = Some(plain_auth);
    let plain_verify_opts =
        VerifyLockfileOptions::from_resolve_options(&plain_opts).expect("lockfile is present");
    assert!(
        PnprClient::new(plain_pnpr_url).verify_lockfile(plain_verify_opts).await.is_err(),
        "without an alias the gated entry's metadata fetch must fail closed",
    );
}

#[tokio::test]
async fn trust_lockfile_makes_the_server_skip_verification() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr().await;

    let client = PnprClient::new(pnpr_url);

    let first = client
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("first install");

    // Same policy that `rejects_an_input_lockfile_that_violates_the_clients_policy`
    // trips on, but with the client's `trustLockfile` opt-out set: the
    // server must skip the verify gate and resolve normally, matching the
    // local `--trust-lockfile` path.
    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
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
