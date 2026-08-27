//! End-to-end tests for the pnpr client against a real pnpr server.
//!
//! Topology: a shared [`TestRegistry`] serves the package fixtures; a
//! per-test in-process `pnpr` hosts the `/-/pnpr` handshake +
//! `/-/pnpr/v0/resolve` endpoints. The client sends the registry it wants
//! resolved from (allowlisted on the server), proving resolution uses the
//! client-supplied registry. Public routes keep their upstream tarball URLs;
//! a private proxied route is returned as its upstream's `/~<name>/`
//! registry-endpoint URL.
//!
//! The client authenticates to pnpr with a bearer token but never
//! forwards its own upstream registry credentials. Private upstream
//! content resolves only when the pnpr server is configured with an
//! access-bearing upstream the caller is authorized to use.

use std::{
    collections::{BTreeMap, HashSet},
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use p256::{
    ecdsa::{SigningKey, signature::Signer as _},
    pkcs8::EncodePublicKey as _,
};
use pnpm_config::RegistryDeclaration;
use pnpm_pnpr_client::{
    ArtifactBlobRequest, ArtifactBlobUpload, ArtifactCandidate, ArtifactFile, ArtifactManifest,
    ArtifactPayload, BuilderProfile, CompatibilityConstraints, OwnerScope, PackageIdentity,
    PnprClient, PnprClientError, PublishArtifactRequest, ResolveArtifactsOptions, ResolveOptions,
    ResolveProject, ResolveProjectsOptions, SignedArtifactEnvelope, VerifyLockfileOptions,
};
use pnpm_testing_utils::registry::TestRegistry;
use sha2::{Digest as _, Sha512};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::Barrier,
};

/// Start an in-process pnpr with the fast-path endpoints, allowlisting
/// `registry_url` as a public route so the client may resolve against it
/// (off-allowlist registries are rejected at the request boundary). Returns
/// the base URL, the bearer `Authorization` for the registered `pnpr-client`
/// caller (pnpr only honors `_authToken` on requests — the resolver
/// endpoints reject Basic credentials), and the storage guard.
async fn start_pnpr(registry_url: &str) -> (String, String, TempDir) {
    start_pnpr_inner(None, Vec::new(), vec![registry_url.to_string()], false).await
}

/// Like [`start_pnpr`] but registers operator-managed access-bearing
/// upstreams, so the server can fetch private upstream content on behalf of
/// an authorized caller without the client forwarding any credential. A
/// upstream's origin is itself allowlisted, so no public route is needed.
async fn start_pnpr_with_upstreams(
    upstreams: Vec<(String, pnpr::UpstreamConfig)>,
) -> (String, String, TempDir) {
    start_pnpr_inner(None, upstreams, Vec::new(), false).await
}

/// Like [`start_pnpr_with_upstreams`] but pins `public_url` so a lockfile
/// produced by one instance can be verified by another fresh instance: a
/// `/~<name>/` endpoint URL is reversed to its upstream by matching the
/// verifying server's own `public_url`, which a real single-pnpr deployment
/// shares across resolve and verify.
async fn start_pnpr_with_upstreams_at(
    public_url: &str,
    upstreams: Vec<(String, pnpr::UpstreamConfig)>,
) -> (String, String, TempDir) {
    start_pnpr_inner(Some(public_url.to_string()), upstreams, Vec::new(), false).await
}

async fn start_pnpr_inner(
    public_url: Option<String>,
    upstreams: Vec<(String, pnpr::UpstreamConfig)>,
    public_registries: Vec<String>,
    artifacts_enabled: bool,
) -> (String, String, TempDir) {
    let storage = TempDir::new().expect("pnpr storage tempdir");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind pnpr");
    let addr = listener.local_addr().expect("pnpr addr");

    let mut config = pnpr::Config::proxy(addr, storage.path().to_path_buf());
    config.artifacts.enabled = artifacts_enabled;
    config.public_url = public_url.unwrap_or_else(|| format!("http://{addr}"));
    config.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;
    for (name, upstream) in upstreams {
        config.upstreams.insert(name, upstream);
    }
    for registry in public_registries {
        config
            .route_policy
            .public
            .push(pnpr::PublicRoute { registry: Some(registry), package: None });
    }

    tokio::spawn(async move {
        let _ = pnpr::serve_listener(config, listener).await;
    });

    wait_until_ready(addr).await;
    let base_url = format!("http://{addr}/");
    let token = register_token(&base_url, "pnpr-client").await;
    (base_url, format!("Bearer {token}"), storage)
}

async fn start_pnpr_artifacts() -> (String, String, TempDir) {
    start_pnpr_inner(None, Vec::new(), Vec::new(), true).await
}

/// An access-bearing upstream that serves `registry_url` with `token`, usable
/// by any authenticated pnpr caller (exposed at `/~test-registry/`).
fn registry_upstream(registry_url: &str, token: &str) -> (String, pnpr::UpstreamConfig) {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .expect("valid authorization header"),
    );
    (
        "test-registry".to_string(),
        pnpr::UpstreamConfig {
            url: registry_url.to_string(),
            headers,
            maxage: None,
            timeout: pnpr::UpstreamConfig::DEFAULT_TIMEOUT,
            max_fails: pnpr::UpstreamConfig::DEFAULT_MAX_FAILS,
            fail_timeout: pnpr::UpstreamConfig::DEFAULT_FAIL_TIMEOUT,
            cache: true,
            access: Some(pnpr::AccessList::from_tokens(["$authenticated"])),
            rules: pnpr::PackageRules::default(),
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

/// Accept one HTTP request, return its full raw bytes (headers + body), and
/// send the supplied raw HTTP response.
async fn capture_one_request_with_response(listener: TcpListener, response: String) -> String {
    let (mut socket, _) = listener.accept().await.expect("accept request");
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = socket.read(&mut chunk).await.expect("read request");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        // Stop once the full body (per Content-Length) has arrived. The
        // boundary is located in the raw bytes so the index stays aligned
        // with `buffer.len()` below: decoding first would rewrite any
        // non-UTF-8 byte as a longer replacement character and shift it.
        if let Some(headers_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buffer[..headers_end]);
            let content_length = headers
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
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
    String::from_utf8_lossy(&buffer).into_owned()
}

async fn capture_one_request(listener: TcpListener) -> String {
    capture_one_request_with_response(
        listener,
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\nstop"
            .to_string(),
    )
    .await
}

fn deps<const COUNT: usize>(entries: [(&str, &str); COUNT]) -> BTreeMap<String, String> {
    entries.into_iter().map(|(name, range)| (name.to_string(), range.to_string())).collect()
}

/// Register a user with an npm-compatible registry and return its bearer
/// token. The pnpr fixture authenticates its own caller with this token;
/// an access-bearing upstream uses one as its server-side credential.
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
        registries: BTreeMap::new(),
        authorization: Some(authorization.to_string()),
        overrides: None,
        patched_dependencies: None,
        package_extensions: None,
        allow_unused_patches: false,
        catalogs: None,
        auto_install_peers: None,
        dedupe_peers: None,
        exclude_links_from_lockfile: None,
        lockfile: None,
        frozen_lockfile: false,
        prefer_frozen_lockfile: None,
        update_patches: false,
        ignore_manifest_check: false,
        trust_lockfile: false,
        resolution_mode: pnpm_config::ResolutionMode::default(),
        minimum_release_age: None,
        minimum_release_age_exclude: None,
        minimum_release_age_ignore_missing_time: true,
        trust_policy: pnpm_config::TrustPolicy::Off,
        trust_policy_exclude: None,
        trust_policy_ignore_after: None,
    }
}

fn signed_artifact_fixture() -> (PublishArtifactRequest, Vec<u8>, Vec<u8>) {
    signed_artifact_fixture_with_builder_id("ci/main/42")
}

fn signed_artifact_fixture_with_builder_id(
    builder_id: &str,
) -> (PublishArtifactRequest, Vec<u8>, Vec<u8>) {
    let blob = b"native-addon".to_vec();
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(&blob)));
    let payload = ArtifactPayload {
        kind: "dependency-side-effects:v1".to_string(),
        package: PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
        source_integrity: "sha512-source".to_string(),
        input_key: "dependency-side-effects:v1:deps=abc".to_string(),
        owner: OwnerScope::organization("pnpr-client"),
        builder_id: builder_id.to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::from([("CFLAGS".to_string(), "-O2".to_string())]),
        },
        compatibility: CompatibilityConstraints::Tagged {
            tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
        },
        manifest: ArtifactManifest {
            added: vec![ArtifactFile {
                path: "build/addon.node".to_string(),
                integrity: integrity.clone(),
                mode: 0o755,
                size: blob.len() as u64,
            }],
            deleted: vec!["src/intermediate.o".to_string()],
        },
    };
    let payload_bytes = serde_json::to_vec(&payload).expect("serialize signed payload");
    let private_key = SigningKey::from_slice(&[7; 32]).expect("fixture private key");
    let signature: p256::ecdsa::Signature = private_key.sign(&payload_bytes);
    let public_key = p256::PublicKey::from(private_key.verifying_key())
        .to_public_key_der()
        .expect("encode fixture public key")
        .as_bytes()
        .to_vec();
    let envelope = SignedArtifactEnvelope {
        algorithm: "ecdsa-p256-sha256".to_string(),
        key_id: "acme-2026".to_string(),
        payload: BASE64.encode(payload_bytes),
        signature: BASE64.encode(signature.to_der().as_bytes()),
    };
    (
        PublishArtifactRequest {
            key: payload.input_key,
            envelope,
            blobs: vec![ArtifactBlobUpload { integrity, data: BASE64.encode(&blob) }],
        },
        public_key,
        blob,
    )
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
    for field in ["autoInstallPeers", "dedupePeers", "excludeLinksFromLockfile"] {
        assert!(
            request.contains(&format!(r#""{field}":null"#)),
            "an unsent {field} must stay unset rather than turn into `false`, got:\n{request}",
        );
    }
}

#[tokio::test]
async fn multi_project_request_sends_every_workspace_project_without_a_synthetic_root() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind capture");
    let addr = listener.local_addr().expect("capture addr");
    let done = serde_json::json!({
        "type": "done",
        "lockfile": {
            "lockfileVersion": "9.0",
            "importers": {
                "packages/app": {},
                "packages/lib": {},
            },
        },
        "stats": { "totalPackages": 0 },
    });
    let body = format!("{done}\n");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let capture = tokio::spawn(capture_one_request_with_response(listener, response));

    let client = PnprClient::new(format!("http://{addr}/"));
    let mut opts: ResolveProjectsOptions =
        options("https://registry.example.test/", "Bearer token", BTreeMap::new()).into();
    opts.projects = vec![
        ResolveProject {
            dir: "packages/app".to_string(),
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
            dependencies: deps([("app-dependency", "1.0.0")]),
            dev_dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
        },
        ResolveProject {
            dir: "packages/lib".to_string(),
            name: Some("lib".to_string()),
            version: Some("2.0.0".to_string()),
            dependencies: BTreeMap::new(),
            dev_dependencies: deps([("lib-tool", "2.0.0")]),
            optional_dependencies: BTreeMap::new(),
        },
    ];

    let outcome = client.resolve_projects(opts).await.expect("multi-project response should parse");
    let mut importer_ids = outcome.lockfile.importers.keys().cloned().collect::<Vec<_>>();
    importer_ids.sort();
    assert_eq!(importer_ids, ["packages/app".to_string(), "packages/lib".to_string()]);

    let request = capture.await.expect("capture task");
    let (_, body) = request.split_once("\r\n\r\n").expect("captured HTTP request has a body");
    let body: serde_json::Value = serde_json::from_str(body).expect("request body is JSON");
    assert_eq!(
        body["projects"],
        serde_json::json!([
            {
                "dir": "packages/app",
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "app-dependency": "1.0.0" },
                "devDependencies": {},
                "optionalDependencies": {},
            },
            {
                "dir": "packages/lib",
                "name": "lib",
                "version": "2.0.0",
                "dependencies": {},
                "devDependencies": { "lib-tool": "2.0.0" },
                "optionalDependencies": {},
            },
        ]),
    );
}

/// The resolved lockfile is merged into the caller's `pnpm-lock.yaml`, so
/// a server that answers with an importer nobody asked about would be
/// writing dependencies into an unrelated project. The response is
/// rejected rather than merged.
#[tokio::test]
async fn an_importer_outside_the_request_is_rejected() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind capture");
    let addr = listener.local_addr().expect("capture addr");
    let done = serde_json::json!({
        "type": "done",
        "lockfile": {
            "lockfileVersion": "9.0",
            "importers": {
                "packages/app": {},
                "packages/attacker": {},
            },
        },
        "stats": { "totalPackages": 0 },
    });
    let body = format!("{done}\n");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let capture = tokio::spawn(capture_one_request_with_response(listener, response));

    let client = PnprClient::new(format!("http://{addr}/"));
    let mut opts: ResolveProjectsOptions =
        options("https://registry.example.test/", "Bearer token", BTreeMap::new()).into();
    opts.projects = vec![ResolveProject {
        dir: "packages/app".to_string(),
        name: Some("app".to_string()),
        version: Some("1.0.0".to_string()),
        dependencies: BTreeMap::new(),
        dev_dependencies: BTreeMap::new(),
        optional_dependencies: BTreeMap::new(),
    }];

    let error = match client.resolve_projects(opts).await {
        Ok(_) => panic!("an unrequested importer must not be accepted"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("packages/attacker"), "unexpected error: {error}");

    capture.await.expect("capture task");
}

/// End-to-end: the test registry gates `@pnpm.e2e/needs-auth` behind
/// `$authenticated`. The client never forwards its own credentials, so
/// resolving it works only when the pnpr server is configured with an
/// upstream for that registry that the caller is
/// authorized to use.
#[tokio::test]
async fn an_upstream_resolves_a_private_package() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-forwarder").await;
    let (pnpr_url, pnpr_auth, _storage) =
        start_pnpr_with_upstreams(vec![registry_upstream(&registry.url(), &token)]).await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), &pnpr_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let outcome = client.resolve(opts).await.expect("the upstream should resolve it");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@pnpm.e2e/needs-auth@1.0.0")),
        "lockfile should contain the authed package, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

/// The same install against a server with no matching upstream
/// fails: the client forwards no credential and pnpr has none to select,
/// so the gated packument can only be fetched anonymously — which the
/// registry refuses.
#[tokio::test]
async fn a_private_package_fails_without_an_upstream() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let client = PnprClient::new(pnpr_url);

    let opts = options(&registry.url(), &pnpr_auth, deps([("@pnpm.e2e/needs-auth", "1.0.0")]));
    let Err(PnprClientError::Server(message)) = client.resolve(opts).await else {
        panic!("expected the gated install to fail with a server error");
    };
    assert!(message.contains("401"), "expected an auth denial without an upstream, got: {message}");
}

#[tokio::test]
async fn resolves_a_package() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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

/// An unknown route (no upstream, no public rule) has no managed credential,
/// so it was resolved anonymously and pnpr mints no gateway URL: the
/// resolution keeps its registry resolution, and the client fetches the
/// tarball directly from the upstream the same way pnpr did.
#[tokio::test]
async fn unknown_route_keeps_its_upstream_tarball_url() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let outcome = PnprClient::new(pnpr_url)
        .resolve(options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")])))
        .await
        .expect("install should succeed");
    let lockfile = serde_json::to_value(&outcome.lockfile).expect("lockfile serializes");
    let resolution = &lockfile["packages"]["@foo/no-deps@1.0.0"]["resolution"];

    // No pnpr gateway URL is minted; the entry stays integrity-only (its URL
    // is reconstructed from the client's configured registry).
    assert!(
        resolution.get("tarball").is_none(),
        "unknown route should stay integrity-only, got: {resolution}",
    );

    // The tarball is fetchable directly from the upstream registry.
    let direct = reqwest::get(format!("{}@foo/no-deps/-/no-deps-1.0.0.tgz", registry.url()))
        .await
        .expect("direct tarball request");
    assert!(direct.status().is_success(), "registry returned {}", direct.status());
}

/// The streaming API surfaces each resolved tarball as a `package`
/// frame *before* the terminal `done` frame carrying the lockfile, and
/// every streamed package appears in the final lockfile. This is the
/// overlap lever: the caller can begin fetching each tarball the moment
/// its frame arrives, while the server is still resolving.
#[tokio::test]
async fn streams_resolved_packages_before_the_lockfile() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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
/// package verifies only when the pnpr server has an upstream for the
/// registry — and fails closed against a server without one. Each verify
/// targets a fresh pnpr so neither the whole-lockfile verdict cache nor the
/// metadata mirror warmed by an earlier call can satisfy it without
/// exercising the upstream. The resolve and aliased-verify instances share a
/// `public_url` so the verifier can reverse the lockfile's `/~<name>/`
/// tarball URLs back to upstream — what a real single-pnpr deployment does.
#[tokio::test]
async fn verify_lockfile_endpoint_uses_upstreams() {
    let registry = TestRegistry::start();
    let token = register_token(&registry.url(), "needs-auth-verifier").await;
    let shared_public_url = "http://pnpr.verify.test";

    let (resolve_pnpr_url, resolve_auth, _resolve_storage) = start_pnpr_with_upstreams_at(
        shared_public_url,
        vec![registry_upstream(&registry.url(), &token)],
    )
    .await;
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

    // A fresh pnpr that carries the upstream verifies the gated entry.
    let (aliased_pnpr_url, aliased_auth, _aliased_storage) = start_pnpr_with_upstreams_at(
        shared_public_url,
        vec![registry_upstream(&registry.url(), &token)],
    )
    .await;
    let mut aliased_opts = resolve_opts.clone();
    aliased_opts.authorization = Some(aliased_auth);
    let verify_opts =
        VerifyLockfileOptions::from_resolve_options(&aliased_opts).expect("lockfile is present");
    PnprClient::new(aliased_pnpr_url)
        .verify_lockfile(verify_opts)
        .await
        .expect("the upstream should let the gated entry verify");

    // A pnpr without the upstream has no credential to select, so the gated
    // entry's metadata fetch must fail closed.
    let (plain_pnpr_url, plain_auth, _plain_storage) = start_pnpr(&registry.url()).await;
    let mut plain_opts = resolve_opts.clone();
    plain_opts.authorization = Some(plain_auth);
    let plain_verify_opts =
        VerifyLockfileOptions::from_resolve_options(&plain_opts).expect("lockfile is present");
    assert!(
        PnprClient::new(plain_pnpr_url).verify_lockfile(plain_verify_opts).await.is_err(),
        "without an upstream the gated entry's metadata fetch must fail closed",
    );
}

#[tokio::test]
async fn trust_lockfile_makes_the_server_skip_verification() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

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

#[tokio::test]
async fn artifact_capability_is_disabled_by_default() {
    let (pnpr_url, _pnpr_auth, _storage) =
        start_pnpr_inner(None, Vec::new(), Vec::new(), false).await;
    let client = PnprClient::new(pnpr_url);
    client.handshake().await.expect("resolver capability");
    let error = client
        .handshake_artifacts()
        .await
        .expect_err("artifact capability must require an explicit opt-in");
    assert!(error.to_string().contains("does not advertise shared artifact protocol"));
}

#[tokio::test]
async fn artifact_handshake_is_independent_from_the_resolver_protocol() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"versions":[],"artifacts":[0]}}"#)
        .create_async()
        .await;

    PnprClient::new(server.url())
        .handshake_artifacts()
        .await
        .expect("artifact-only capability is supported");
    mock.assert_async().await;
}

#[tokio::test]
async fn artifact_blob_download_rejects_bytes_that_do_not_match_the_integrity() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/-/pnpr/v0/artifacts/blob")
        .with_status(200)
        .with_body("poisoned blob")
        .create_async()
        .await;
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(b"expected blob")));

    let error = PnprClient::new(server.url())
        .download_artifact_blob(
            &ArtifactBlobRequest { owner: OwnerScope::organization("acme"), integrity },
            None,
        )
        .await
        .expect_err("a corrupt artifact blob must be rejected");

    assert!(matches!(error, PnprClientError::Protocol(_)), "got: {error}");
    mock.assert_async().await;
}

#[tokio::test]
async fn publishes_resolves_and_verifies_an_organization_artifact() {
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let client = PnprClient::new(pnpr_url);
    client.handshake_artifacts().await.expect("artifact capability");

    let (publish, public_key, expected_blob) = signed_artifact_fixture();
    client.publish_artifact(&publish, Some(&pnpr_auth)).await.expect("publish signed artifact");

    let candidate = ArtifactCandidate {
        key: publish.key.clone(),
        package: PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
        source_integrity: "sha512-source".to_string(),
        owner: OwnerScope::organization("pnpr-client"),
    };
    let untrusted_key = SigningKey::from_slice(&[8; 32]).expect("alternate fixture private key");
    let untrusted_public_key = p256::PublicKey::from(untrusted_key.verifying_key())
        .to_public_key_der()
        .expect("encode alternate fixture public key")
        .as_bytes()
        .to_vec();
    let untrusted = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([candidate.package.name.clone()]),
            allowed_builds: HashSet::from([candidate.package.name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), untrusted_public_key)]),
            pinned_envelope_digests: BTreeMap::new(),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("an invalid signature is a cache miss");
    assert!(untrusted.is_empty());

    let mut mismatched_candidate = candidate.clone();
    mismatched_candidate.package.version = "2.0.0".to_string();
    let mismatched = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![mismatched_candidate],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([candidate.package.name.clone()]),
            allowed_builds: HashSet::from([candidate.package.name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            pinned_envelope_digests: BTreeMap::new(),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("a mismatched signed package identity is a cache miss");
    assert!(mismatched.is_empty());

    let selected = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([candidate.package.name.clone()]),
            allowed_builds: HashSet::from([candidate.package.name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            pinned_envelope_digests: BTreeMap::new(),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("resolve signed artifact");
    let artifact = selected.get(&publish.key).expect("trusted compatible variant selected");
    assert_eq!(artifact.payload.owner, OwnerScope::organization("pnpr-client"));
    assert_eq!(artifact.envelope_digest.len(), 64);
    let pinned_digest = artifact.envelope_digest.clone();
    let quarantined = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([candidate.package.name.clone()]),
            allowed_builds: HashSet::from([candidate.package.name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            pinned_envelope_digests: BTreeMap::new(),
            quarantined_envelope_digests: BTreeMap::from([(
                candidate.key.clone(),
                HashSet::from([pinned_digest.clone()]),
            )]),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("a quarantined variant is a cache miss");
    assert!(quarantined.is_empty());
    let pinned = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([candidate.package.name.clone()]),
            allowed_builds: HashSet::from([candidate.package.name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key.clone())]),
            pinned_envelope_digests: BTreeMap::from([(
                candidate.key.clone(),
                pinned_digest.clone(),
            )]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("an exact pin should select the artifact");
    assert_eq!(
        pinned.get(&candidate.key).map(|artifact| &artifact.envelope_digest),
        Some(&pinned_digest),
    );
    let pinned = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![candidate.clone()],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from([candidate.package.name.clone()]),
            allowed_builds: HashSet::from([candidate.package.name.clone()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key)]),
            pinned_envelope_digests: BTreeMap::from([(candidate.key.clone(), "0".repeat(64))]),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth.clone()),
        })
        .await
        .expect("a missing pin is a cache miss");
    assert!(pinned.is_empty());

    let bytes = client
        .download_artifact_blob(
            &ArtifactBlobRequest {
                owner: candidate.owner,
                integrity: artifact.payload.manifest.added[0].integrity.clone(),
            },
            Some(&pnpr_auth),
        )
        .await
        .expect("download verified blob");
    assert_eq!(bytes, expected_blob);
}

#[tokio::test]
async fn artifact_lookup_preserves_script_eligibility_and_allow_build_policy() {
    let (publish, public_key, _) = signed_artifact_fixture();
    let candidate = ArtifactCandidate {
        key: publish.key,
        package: PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
        source_integrity: "sha512-source".to_string(),
        owner: OwnerScope::organization("pnpr-client"),
    };
    let supported_tags = vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()];
    let trusted_keys = BTreeMap::from([("acme-2026".to_string(), public_key)]);
    let client = PnprClient::new("http://127.0.0.1:9/");

    for (ignore_scripts, eligible_packages, allowed_builds) in [
        (
            true,
            HashSet::from([candidate.package.name.clone()]),
            HashSet::from([candidate.package.name.clone()]),
        ),
        (false, HashSet::new(), HashSet::from([candidate.package.name.clone()])),
        (false, HashSet::from([candidate.package.name.clone()]), HashSet::new()),
    ] {
        let selected = client
            .resolve_artifacts(ResolveArtifactsOptions {
                candidates: vec![candidate.clone()],
                supported_tags: supported_tags.clone(),
                eligible_packages,
                allowed_builds,
                ignore_scripts,
                trusted_keys: trusted_keys.clone(),
                pinned_envelope_digests: BTreeMap::new(),
                quarantined_envelope_digests: BTreeMap::new(),
                on_rejected_artifact: None,
                authorization: None,
            })
            .await
            .expect("a denied remote build must not contact pnpr");
        assert!(selected.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn concurrent_artifact_publications_apply_the_variant_limit_at_read_time() {
    const PUBLICATIONS: usize = 16;

    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let (fixture, _, _) = signed_artifact_fixture_with_builder_id("ci/concurrent/0");
    let (payload, _) = fixture.envelope.decode_payload().expect("decode fixture payload");
    let candidate = ArtifactCandidate {
        key: fixture.key,
        package: payload.package,
        source_integrity: payload.source_integrity,
        owner: payload.owner,
    };
    let barrier = Arc::new(Barrier::new(PUBLICATIONS + 1));
    let mut publications = Vec::with_capacity(PUBLICATIONS);
    for index in 0..PUBLICATIONS {
        let barrier = Arc::clone(&barrier);
        let pnpr_url = pnpr_url.clone();
        let pnpr_auth = pnpr_auth.clone();
        let (publish, _, _) =
            signed_artifact_fixture_with_builder_id(&format!("ci/concurrent/{index}"));
        publications.push(tokio::spawn(async move {
            barrier.wait().await;
            PnprClient::new(pnpr_url).publish_artifact(&publish, Some(&pnpr_auth)).await
        }));
    }
    barrier.wait().await;

    for publication in publications {
        publication.await.expect("publication task").expect("publish artifact variant");
    }

    let response = reqwest::Client::new()
        .post(format!("{pnpr_url}-/pnpr/v0/artifacts/resolve"))
        .header(reqwest::header::AUTHORIZATION, pnpr_auth)
        .json(&pnpm_pnpr_client::ResolveArtifactsRequest { candidates: vec![candidate] })
        .send()
        .await
        .expect("resolve artifacts response")
        .error_for_status()
        .expect("successful artifact resolve")
        .json::<serde_json::Value>()
        .await
        .expect("artifact resolve JSON");
    let variants =
        response["artifacts"][0]["variants"].as_array().expect("artifact variants array");
    assert_eq!(variants.len(), pnpm_shared_artifact_protocol::MAX_VARIANTS_PER_CANDIDATE);
}

#[tokio::test]
async fn artifact_blob_misses_and_errors_are_caller_scoped() {
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let http = reqwest::Client::new();
    let missing_integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(b"missing")));
    let missing = http
        .post(format!("{pnpr_url}-/pnpr/v0/artifacts/blob"))
        .header(reqwest::header::AUTHORIZATION, &pnpr_auth)
        .json(&ArtifactBlobRequest {
            owner: OwnerScope::organization("pnpr-client"),
            integrity: missing_integrity,
        })
        .send()
        .await
        .expect("missing blob response");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    assert_eq!(missing.headers()[reqwest::header::CACHE_CONTROL], "private, no-store");
    assert_eq!(missing.headers()[reqwest::header::VARY], "Authorization");

    let invalid = http
        .post(format!("{pnpr_url}-/pnpr/v0/artifacts/blob"))
        .header(reqwest::header::AUTHORIZATION, &pnpr_auth)
        .json(&serde_json::json!({
            "owner": { "type": "organization", "name": "pnpr-client" },
            "integrity": "not-an-integrity",
        }))
        .send()
        .await
        .expect("invalid blob response");
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(invalid.headers()[reqwest::header::CACHE_CONTROL], "private, no-store");
    assert_eq!(invalid.headers()[reqwest::header::VARY], "Authorization");
}

#[tokio::test]
async fn organization_artifact_existence_is_not_exposed_to_another_owner() {
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr_artifacts().await;
    let client = PnprClient::new(pnpr_url);
    let (publish, public_key, _) = signed_artifact_fixture();
    client.publish_artifact(&publish, Some(&pnpr_auth)).await.expect("publish artifact");

    let selected = client
        .resolve_artifacts(ResolveArtifactsOptions {
            candidates: vec![ArtifactCandidate {
                key: publish.key,
                package: PackageIdentity {
                    name: "native-addon".to_string(),
                    version: "1.0.0".to_string(),
                },
                source_integrity: "sha512-source".to_string(),
                owner: OwnerScope::organization("another-owner"),
            }],
            supported_tags: vec!["pnpm:v1:linux-x64-node22-glibc2.17".to_string()],
            eligible_packages: HashSet::from(["native-addon".to_string()]),
            allowed_builds: HashSet::from(["native-addon".to_string()]),
            ignore_scripts: false,
            trusted_keys: BTreeMap::from([("acme-2026".to_string(), public_key)]),
            pinned_envelope_digests: BTreeMap::new(),
            quarantined_envelope_digests: BTreeMap::new(),
            on_rejected_artifact: None,
            authorization: Some(pnpr_auth),
        })
        .await
        .expect("cross-owner lookup is a masked miss");
    assert!(selected.is_empty());
}

/// The client describes its registries to the server the way its own
/// `registries` setting does, so a scope the client routes elsewhere resolves
/// from that registry and not from the request's default one.
///
/// This is also the contract test for the two ends of the protocol: the
/// server reads the declarations under the key the client writes them.
#[tokio::test]
async fn resolves_a_scope_from_the_registry_declared_for_it() {
    let registry = TestRegistry::start();
    // A default registry that is allowlisted but serves nothing: reaching it
    // for the scoped package is the failure this test is looking for.
    let dead_default = "http://127.0.0.1:9/";
    let (pnpr_url, pnpr_auth, _storage) =
        start_pnpr_inner(None, Vec::new(), vec![registry.url(), dead_default.to_string()], false)
            .await;

    let mut opts = options(dead_default, &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.registries = BTreeMap::from([(
        registry.url(),
        RegistryDeclaration {
            scopes: Some(vec!["@foo".to_string()]),
            ..RegistryDeclaration::default()
        },
    )]);

    let outcome = PnprClient::new(pnpr_url).resolve(opts).await.expect("install should succeed");

    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(
        packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")),
        "the declared registry should have served the scope, got: {:?}",
        packages.keys().map(ToString::to_string).collect::<Vec<_>>(),
    );
}

/// A client describes its whole configuration, including scopes a given
/// resolve never reaches. Declaring a registry this pnpr does not serve is
/// therefore not an error by itself — only fetching from one is.
#[tokio::test]
async fn a_declared_registry_the_resolve_never_reaches_is_not_rejected() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.registries = BTreeMap::from([(
        "http://169.254.169.254/".to_string(),
        RegistryDeclaration {
            scopes: Some(vec!["@never-resolved".to_string()]),
            ..RegistryDeclaration::default()
        },
    )]);

    let outcome = PnprClient::new(pnpr_url).resolve(opts).await.expect("install should succeed");
    let packages = outcome.lockfile.packages.as_ref().expect("lockfile has packages");
    assert!(packages.keys().any(|key| key.to_string().starts_with("@foo/no-deps@1.0.0")));
}

/// The SSRF boundary still holds where it matters: a scope the resolve *does*
/// reach is refused before the request leaves the server.
#[tokio::test]
async fn a_declared_registry_the_resolve_reaches_is_refused() {
    let registry = TestRegistry::start();
    let (pnpr_url, pnpr_auth, _storage) = start_pnpr(&registry.url()).await;

    let mut opts = options(&registry.url(), &pnpr_auth, deps([("@foo/no-deps", "1.0.0")]));
    opts.registries = BTreeMap::from([(
        "http://169.254.169.254/".to_string(),
        RegistryDeclaration {
            scopes: Some(vec!["@foo".to_string()]),
            ..RegistryDeclaration::default()
        },
    )]);

    let Err(error) = PnprClient::new(pnpr_url).resolve(opts).await else {
        panic!("an off-allowlist registry the resolve reaches must be refused")
    };
    let error = error.to_string();
    assert!(error.contains("is not allowed by this pnpr server"), "{error}");
    assert!(error.contains("169.254.169.254"), "the refused origin is named: {error}");
}
