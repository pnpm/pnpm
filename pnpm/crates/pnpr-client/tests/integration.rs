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
    ArtifactPayload, ArtifactSubject, BuilderProfile, CompatibilityConstraints, OwnerScope,
    PackageIdentity, PnprClient, PnprClientError, PublishArtifactRequest, ResolveArtifactsOptions,
    ResolveOptions, ResolveProject, ResolveProjectsOptions, SignedArtifactEnvelope,
    VerifyLockfileOptions,
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
            search: false,
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
        let Some(headers_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
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
    signed_artifact_fixture_for(builder_id, "pnpm:v1:linux-x64-node22-glibc2.17")
}

/// One input key admits one artifact per set of compatibility constraints, so a
/// test wanting several of them for one dependency varies the platform — which
/// is the only reason a second artifact for one input is legitimate.
fn signed_artifact_fixture_for_platform(
    index: usize,
) -> (PublishArtifactRequest, Vec<u8>, Vec<u8>) {
    // Node major, not the glibc floor: two floors for one architecture and Node
    // major both apply to a consumer meeting the higher one, so the registry
    // refuses the second as an overlapping publish.
    signed_artifact_fixture_for(
        &format!("ci/concurrent/{index}"),
        &format!("pnpm:v1:linux-x64-node{}-glibc2.17", index + 1),
    )
}

fn signed_artifact_fixture_for(
    builder_id: &str,
    tag: &str,
) -> (PublishArtifactRequest, Vec<u8>, Vec<u8>) {
    let blob = b"native-addon".to_vec();
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(&blob)));
    let payload = ArtifactPayload {
        kind: "dependency-side-effects:v1".to_string(),
        subject: ArtifactSubject::dependency_side_effects(
            PackageIdentity { name: "native-addon".to_string(), version: "1.0.0".to_string() },
            "sha512-source",
        ),
        input_key: "dependency-side-effects:v1:deps=abc".to_string(),
        owner: OwnerScope::organization("pnpr-client"),
        builder_id: builder_id.to_string(),
        builder_profile: BuilderProfile {
            image_digest: Some("sha256:image".to_string()),
            architecture_baseline: "x86-64-v2".to_string(),
            environment: BTreeMap::from([("CFLAGS".to_string(), "-O2".to_string())]),
        },
        compatibility: CompatibilityConstraints::Tagged { tags: vec![tag.to_string()] },
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

#[path = "integration/authorization.rs"]
mod authorization;

#[path = "integration/workspace_settings.rs"]
mod workspace_settings;

#[path = "integration/security.rs"]
mod security;

#[path = "integration/streaming.rs"]
mod streaming;

#[path = "integration/behavior.rs"]
mod behavior;

#[path = "integration/lockfile.rs"]
mod lockfile;

#[path = "integration/dependencies.rs"]
mod dependencies;

#[path = "integration/integrity.rs"]
mod integrity;
