#[path = "server/registry_discovery.rs"]
mod registry_discovery;

#[path = "server/hosted_discovery.rs"]
mod hosted_discovery;

#[path = "server/hosted_reads.rs"]
mod hosted_reads;

#[path = "server/behavior.rs"]
mod behavior;

#[path = "server/surfaces.rs"]
mod surfaces;

#[path = "server/tarball_verification.rs"]
mod tarball_verification;

#[path = "server/revision_tarballs.rs"]
mod revision_tarballs;

#[path = "server/registry_routing.rs"]
mod registry_routing;

#[path = "server/streaming_cache.rs"]
mod streaming_cache;

#[path = "server/tarball_proxy.rs"]
mod tarball_proxy;

#[path = "server/packuments.rs"]
mod packuments;

#[path = "server/resolver_access.rs"]
mod resolver_access;

#[path = "common/registry_groups.rs"]
mod registry_groups;

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{HeaderValue, Request, StatusCode, header},
};
use flate2::read::GzDecoder;
use futures_util::stream;
use pnpm_crypto_hash::integrity_addressed_tarball_path;
use pnpr::{
    AccessList, AuthState, Config, Ecosystem, HostedConfig, MaxUsers, PackagePattern, PackageRule,
    PackageRules, PublicRoute, Registries, Registry, router, router_with_auth,
};
use serde_json::{Value, json};
use ssri::{Algorithm, IntegrityOpts};
use std::{
    convert::Infallible,
    fs,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

fn config_for(upstream: &str, storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").url = upstream.to_string();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    config
}

/// A hosted registry whose `packages:` map is empty (claims every name) with
/// the given registry-level default `access:`. Unpublish defaults to any
/// authenticated user, the registry-mock contract these fixtures always ran
/// under (the safe per-registry default would deny destructive writes).
fn hosted_with_access(org: &str, access: &str) -> HostedConfig {
    HostedConfig {
        org: org.to_string(),
        rules: PackageRules::new(Vec::new(), Some(AccessList::from_tokens([access])))
            .with_default_unpublish(AccessList::from_tokens(["$authenticated"])),
        teams: pnpr::Teams::default(),
    }
}

/// One `packages:` entry carrying only an `access` rule.
fn access_rule(pattern: &str, access: &str) -> PackageRule {
    PackageRule {
        pattern: PackagePattern::parse(pattern, Ecosystem::Npm).expect("test pattern parses"),
        access: Some(AccessList::from_tokens([access])),
        publish: None,
        unpublish: None,
    }
}

/// A public upstream registry caches under `.pnpr-cache/~public/<digest>/<pkg>/`.
/// The proxy tests use a single `npmjs` registry, so there is one digest dir; this
/// resolves the per-package cache dir under it. When nothing is cached yet it
/// returns a path that does not exist, so existence assertions read naturally.
fn public_cache_pkg(cache_root: &Path, pkg: &str) -> PathBuf {
    let public = cache_root.join(".pnpr-cache").join("~public");
    // The public cache holds one digest directory per public registry. These tests
    // configure exactly one, so require at most one *directory* (ignoring stray
    // files and `read_dir` order) instead of trusting the first entry.
    let mut digest_dirs: Vec<PathBuf> = std::fs::read_dir(&public)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    digest_dirs.sort();
    assert!(
        digest_dirs.len() <= 1,
        "expected at most one ~public registry namespace, found {digest_dirs:?}",
    );
    digest_dirs.first().cloned().unwrap_or_else(|| public.join("__none__")).join(pkg)
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

fn git_resolve_request(repo_url: &str, authorization: Option<&str>) -> Request<Body> {
    let body = json!({
        "dependencies": {
            "git-dependency": format!("git+{repo_url}#main"),
        },
        // The built-in npmjs route is allowlisted; this resolve only touches a
        // git dependency, so the registry is validated but never fetched.
        "registry": "https://registry.npmjs.org/",
        "trustLockfile": true,
        "preferFrozenLockfile": false,
    });
    let mut request =
        Request::post("/-/pnpr/v0/resolve").header("content-type", "application/json");
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    request.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
}

fn verify_lockfile_request(registry_url: &str, authorization: Option<&str>) -> Request<Body> {
    let body = json!({
        "registry": registry_url,
        "lockfile": {
            "lockfileVersion": "9.0",
            "settings": {
                "autoInstallPeers": true,
                "excludeLinksFromLockfile": false,
            },
            "importers": {
                ".": {
                    "dependencies": {
                        "probe-pkg": {
                            "specifier": "1.0.0",
                            "version": "1.0.0",
                        },
                    },
                },
            },
            "packages": {
                "probe-pkg@1.0.0": {
                    "resolution": {
                        "integrity": "sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==",
                    },
                },
            },
            "snapshots": {
                "probe-pkg@1.0.0": {},
            },
        },
        "minimumReleaseAge": 1,
        "minimumReleaseAgeIgnoreMissingTime": false,
    });
    let mut request =
        Request::post("/-/pnpr/v0/verify-lockfile").header("content-type", "application/json");
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    request.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
}

async fn drain_resolve_response(response: axum::response::Response) -> (StatusCode, Vec<u8>) {
    let status = response.status();
    let body = tokio::time::timeout(Duration::from_secs(10), body_bytes(response.into_body()))
        .await
        .expect("resolver response should finish within 10 seconds");
    (status, body)
}

async fn spawn_git_probe() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let request_count = Arc::new(AtomicUsize::new(0));
    let probe_count = Arc::clone(&request_count);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            probe_count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\n\
                          Content-Length: 0\r\n\
                          Connection: close\r\n\
                          \r\n",
                    )
                    .await;
            });
        }
    });
    (format!("http://{addr}/repo.git"), request_count)
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

fn sha512_integrity(bytes: &[u8]) -> String {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

fn hosted_publish_request(
    url: &str,
    package: &str,
    version: &str,
    tarball: &[u8],
    token: &str,
) -> Request<Body> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let basename = package.rsplit('/').next().unwrap();
    let attachment = format!("{package}-{version}.tgz");
    let body = json!({
        "name": package,
        "dist-tags": { "latest": version },
        "versions": { (version): { "name": package, "version": version, "dist": {
            "tarball": format!("http://example.test/{package}/-/{basename}-{version}.tgz"),
            "integrity": sha512_integrity(tarball),
        } } },
        "_attachments": { (attachment): {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(tarball),
            "length": tarball.len(),
        } },
    });
    Request::put(url)
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

/// The 40-char hex SHA-1 the way pre-2017 npm publishes carry it in the
/// legacy `dist.shasum` field.
fn sha1_hex_of(bytes: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha1);
    opts.input(bytes);
    let integrity = opts.result();
    let digest = BASE64.decode(&integrity.hashes[0].digest).unwrap();
    digest.iter().fold(String::with_capacity(40), |mut acc, byte| {
        use std::fmt::Write as _;
        write!(acc, "{byte:02x}").unwrap();
        acc
    })
}

fn osv_database(package: &str, versions: &[&str]) -> TempDir {
    let dir = TempDir::new().unwrap();
    let versions: Vec<Value> = versions.iter().map(|version| json!(version)).collect();
    let advisory = json!({
        "id": "GHSA-registry",
        "affected": [{
            "package": { "ecosystem": "npm", "name": package },
            "versions": versions,
        }],
    });
    fs::write(dir.path().join("GHSA-registry.json"), advisory.to_string()).unwrap();
    dir
}

fn enable_osv(config: &mut Config, path: &Path) {
    config.osv.enabled = true;
    config.osv.path = Some(path.to_path_buf());
}

async fn mock_packument_for_tarball(
    upstream: &mut mockito::ServerGuard,
    package: &str,
    version: &str,
    expected_bytes: &[u8],
) -> mockito::Mock {
    let basename = package.rsplit('/').next().expect("package has a basename");
    let mut versions = serde_json::Map::new();
    versions.insert(
        version.to_string(),
        json!({
            "name": package,
            "version": version,
            "dist": {
                "tarball": format!("{}/{package}/-/{basename}-{version}.tgz", upstream.url()),
                "integrity": sha512_integrity(expected_bytes),
            },
        }),
    );
    let packument = json!({
        "name": package,
        "dist-tags": { "latest": version },
        "versions": versions,
    });
    let path = format!("/{package}");
    upstream
        .mock("GET", path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        // At least once: a `cache: true` registry fetches the packument a single
        // time (then caches); a `cache: false` registry refetches per request.
        .expect_at_least(1)
        .create_async()
        .await
}

/// A proxy config whose default `npmjs` upstream is promoted to an
/// access-bearing private-route upstream, reachable at `/~npmjs/`.
fn upstream_endpoint_config(upstream_url: &str, storage: PathBuf, access: &str) -> Config {
    let mut config = config_for(upstream_url, storage);
    config.public_url = "http://example.test".to_string();
    config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream").access =
        Some(AccessList::from_tokens([access]));
    config
}

/// Spawn a TCP listener that serves a valid packument but truncates the
/// matching tarball body. Mockito cannot simulate a mid-body disconnect.
async fn spawn_truncated_upstream(expected_integrity: String) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let packument = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("http://{addr}/foo/-/foo-1.0.0.tgz"),
                    "integrity": expected_integrity,
                },
            },
        },
    })
    .to_string();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let packument = packument.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let bytes_read = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..bytes_read]);
                if request.starts_with("GET /foo HTTP/") {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Length: {}\r\n\
                         Content-Type: application/json\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {packument}",
                        packument.len(),
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    return;
                }
                if request.starts_with("GET /foo/-/foo-1.0.0.tgz HTTP/") {
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\n\
                          Content-Length: 1048576\r\n\
                          Content-Type: application/octet-stream\r\n\
                          Connection: close\r\n\
                          \r\n",
                        )
                        .await;
                    let _ = socket.write_all(&[0xAA; 100]).await;
                    return;
                }
                let _ =
                    socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
            });
        }
    });
    addr
}

fn is_tarball_tmp(name: &str) -> bool {
    name.split_once(".tgz.tmp.").is_some_and(|(_, suffix)| !suffix.is_empty())
}

fn tarball_cache_entries(dir: &std::path::Path) -> Vec<String> {
    let mut entries = dir
        .read_dir()
        .map(|iter| {
            iter.filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".tgz") || is_tarball_tmp(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

async fn await_no_tgz(dir: &std::path::Path, budget: Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    loop {
        if tarball_cache_entries(dir).is_empty() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn foo_packument(upstream_url: &str) -> Value {
    json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{upstream_url}/foo/-/foo-1.0.0.tgz"),
                    "shasum": "deadbeef",
                },
            },
        },
    })
}

// --------------------------------------------------------------------
// Registry routing (RFC: registries for pnpr).
// --------------------------------------------------------------------

/// Mock a one-version packument plus its tarball for `pkg` on `server`,
/// returning the tarball bytes.
async fn mock_package(server: &mut mockito::Server, pkg: &str, marker: &str) -> Vec<u8> {
    let body = format!("tarball-{marker}").into_bytes();
    // The canonical tarball basename strips any scope: `@corp/secret` →
    // `secret-1.0.0.tgz`.
    let bare = pkg.rsplit('/').next().unwrap();
    let basename = format!("{bare}-1.0.0.tgz");
    let packument = json!({
        "name": pkg,
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": pkg, "version": "1.0.0", "dist": {
            "tarball": format!("{}/{pkg}/-/{basename}", server.url()),
            "integrity": sha512_integrity(&body),
        } } },
    });
    server
        .mock("GET", format!("/{pkg}").as_str())
        .with_body(packument.to_string())
        .create_async()
        .await;
    server
        .mock("GET", format!("/{pkg}/-/{basename}").as_str())
        .with_header("content-type", "application/octet-stream")
        .with_body(&body)
        .create_async()
        .await;
    body
}

/// Build a config with two public upstream registries — `corp` claiming `@corp/*`
/// and a pattern-less `npmjs` catch-all — and a `main` router over them,
/// aliased by the path-less base.
fn router_config(npmjs_url: &str, corp_url: &str, storage: PathBuf) -> Config {
    let mut config = config_for(npmjs_url, storage);
    let mut corp = config.upstreams.get("npmjs").expect("default `npmjs` upstream").clone();
    corp.url = corp_url.to_string();
    config.upstreams.insert("corp".to_string(), corp);
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "corp".to_string(),
            Registry::Upstream {
                patterns: vec![PackagePattern::parse("@corp/*", Ecosystem::Npm).unwrap()],
            },
        ),
        (
            "main".to_string(),
            Registry::Router { sources: vec!["corp".to_string(), "npmjs".to_string()] },
        ),
    ];
    let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    registries.validate().expect("router config is valid");
    config.registries = registries;
    config
}

/// Seed a hosted package directly into the hosted store (root == `storage`),
/// as a publish would have.
fn seed_hosted(storage: &Path, pkg: &str) {
    let packument = json!({
        "name": pkg,
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": pkg, "version": "1.0.0", "dist": {
            "tarball": format!("http://example.test/{pkg}/-/widget-1.0.0.tgz"),
            "shasum": "abc",
        } } },
    });
    std::fs::create_dir_all(storage.join(pkg)).unwrap();
    std::fs::write(storage.join(pkg).join("package.json"), packument.to_string()).unwrap();
}

fn seed_hosted_with_maintainer(storage: &Path, pkg: &str, maintainer: &str) {
    seed_hosted(storage, pkg);
    let path = storage.join(pkg).join("package.json");
    let mut packument: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    packument["maintainers"] = json!([{ "username": maintainer }]);
    std::fs::write(path, serde_json::to_vec(&packument).unwrap()).unwrap();
}

/// The grouped config declares `internal` and the `main` router over it in
/// every ecosystem; a caller who cannot see them gets neither, and no default.
fn assert_grouped_ecosystem(directory: &Value, ecosystem: Ecosystem, visible: bool) {
    assert_eq!(
        directory["defaultRegistries"][ecosystem.as_str()],
        if visible { json!("main") } else { Value::Null },
    );
    let entries: Vec<_> = directory["registries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["ecosystem"] == ecosystem.as_str())
        .collect();
    assert_eq!(entries.len(), if visible { 2 } else { 0 }, "{ecosystem}");
    if !visible {
        return;
    }
    assert_eq!(entries[0]["name"], "internal");
    assert_eq!(entries[1]["name"], "main");
    assert_eq!(entries[1]["sources"], json!(["internal"]));
}

/// The registry directory as one caller sees it.
async fn read_registry_directory(app: &Router, token: Option<&String>) -> Value {
    let mut request = Request::get("/-/pnpr/v0/registries");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap()
}
