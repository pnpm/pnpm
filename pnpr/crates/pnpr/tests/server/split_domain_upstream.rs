use super::{
    AccessList, Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, Router,
    ServiceExt, StatusCode, TempDir, body_bytes, config_for, drain_resolve_response, json,
    package_tar_and_tgz, router_with_auth, sha512_integrity,
};
use crate::tokens::{issue_token, seeded_auth};
use axum::http::header;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const CORP_BEARER: &str = "Bearer corp-upstream-secret";
const PRESIGNED_QUERY: &str = "X-Amz-Signature=EXAMPLE_PRESIGNED_TOKEN";

struct SplitDomainHarness {
    app: Router,
    reader_token: String,
    outsider_token: String,
    upstream_url: String,
    cdn_authority: String,
    tgz: Vec<u8>,
    cdn_hits: Arc<AtomicUsize>,
}

async fn bind_loopback_preferring(ip: &str) -> TcpListener {
    match TcpListener::bind(format!("{ip}:0")).await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind("127.0.0.1:0").await.unwrap(),
    }
}

async fn spawn_presigned_cdn(tgz: Vec<u8>) -> (String, Arc<AtomicUsize>) {
    let listener = bind_loopback_preferring("127.0.0.3").await;
    let authority = listener
        .local_addr()
        .unwrap()
        .to_string();
    let hits = Arc::new(AtomicUsize::new(0));
    let served_hits = Arc::clone(&hits);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let tgz = tgz.clone();
            let hits = Arc::clone(&served_hits);
            tokio::spawn(async move {
                serve_cdn_connection(&mut socket, &tgz, &hits).await;
            });
        }
    });
    (authority, hits)
}

const MAX_TEST_HEADER_BYTES: usize = 16 * 1024;

async fn read_http_headers(socket: &mut tokio::net::TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(pos) = buf
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            let end = pos + 4;
            return (end <= MAX_TEST_HEADER_BYTES).then(|| {
                String::from_utf8_lossy(&buf[..end]).into_owned()
            });
        }
        if buf.len() >= MAX_TEST_HEADER_BYTES {
            return None;
        }
    }
}

async fn serve_cdn_connection(socket: &mut tokio::net::TcpStream, tgz: &[u8], hits: &AtomicUsize) {
    let Some(req) = read_http_headers(socket).await else {
        return;
    };
    let expected_line = format!("GET /@corp/split/split-1.0.0.tgz?{PRESIGNED_QUERY} HTTP/");
    let leaked_auth = req
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("authorization:"));
    if !req.starts_with(&expected_line) || leaked_auth {
        let _ = socket.write_all(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
        return;
    }
    hits.fetch_add(1, Ordering::SeqCst);
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        tgz.len(),
    );
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(tgz).await;
}

async fn spawn_split_packument_upstream(packument: String) -> String {
    let listener = bind_loopback_preferring("127.0.0.2").await;
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let packument = packument.clone();
            tokio::spawn(async move {
                serve_packument_connection(&mut socket, &packument).await;
            });
        }
    });
    url
}

async fn serve_packument_connection(socket: &mut tokio::net::TcpStream, packument: &str) {
    let Some(req) = read_http_headers(socket).await else {
        return;
    };
    let is_split_get = req.starts_with("GET /@corp%2fsplit HTTP/")
        || req.starts_with("GET /@corp%2Fsplit HTTP/")
        || req.starts_with("GET /@corp/split HTTP/");
    let authorized = req
        .lines()
        .any(|line| {
            line.split_once(':')
                .is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization") && value.trim() == CORP_BEARER
                })
        });
    if !is_split_get || !authorized {
        let _ = socket.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
        return;
    }
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{packument}",
        packument.len(),
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

fn split_packument(cdn_authority: &str, tgz: &[u8]) -> String {
    json!({
        "name": "@corp/split",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2024-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "@corp/split",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!(
                        "http://{cdn_authority}/@corp/split/split-1.0.0.tgz?{PRESIGNED_QUERY}",
                    ),
                    "integrity": sha512_integrity(tgz),
                    "unpackedSize": 512,
                }
            }
        }
    })
    .to_string()
}

fn build_split_config(npmjs_url: &str, corp_url: &str, storage: std::path::PathBuf) -> Config {
    let mut config = config_for(npmjs_url, storage);
    let mut corp = config.routing.upstreams
        .get("npmjs")
        .expect("default npmjs upstream")
        .clone();
    corp.url = corp_url.to_string();
    corp.public = false;
    corp.set_access(AccessList::from_tokens(["alice"]));
    corp.headers.insert(header::AUTHORIZATION, CORP_BEARER.parse().unwrap());
    config.routing.upstreams.insert("corp".to_string(), corp);
    config.routing.registries = Registries::new(
        [
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
        ]
        .into_iter()
        .collect(),
        Some("main".to_string()),
    );
    config
}

async fn setup_split_domain(tmp: &TempDir) -> SplitDomainHarness {
    let (_, tgz) = package_tar_and_tgz(r#"{"name":"@corp/split","version":"1.0.0"}"#);
    let (cdn_authority, cdn_hits) = spawn_presigned_cdn(tgz.clone()).await;
    let upstream_url = spawn_split_packument_upstream(split_packument(&cdn_authority, &tgz)).await;
    let config = build_split_config("http://127.0.0.1:1", &upstream_url, tmp.path().to_path_buf());
    let auth = seeded_auth(&config, &[("alice", "password"), ("outsider", "password")]);
    let reader_token = issue_token(&auth, "alice").await;
    let outsider_token = issue_token(&auth, "outsider").await;
    SplitDomainHarness {
        app: router_with_auth(config, auth),
        reader_token,
        outsider_token,
        upstream_url,
        cdn_authority,
        tgz,
        cdn_hits,
    }
}

async fn post_ndjson(app: &Router, path: &str, token: &str, body: Value) -> Vec<Value> {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, raw) = drain_resolve_response(response).await;
    let text = String::from_utf8(raw).unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn assert_no_cdn_or_credential_leak(frames: &[Value], cdn_authority: &str) {
    let serialized = serde_json::to_string(frames).unwrap();
    assert!(!serialized.contains(cdn_authority), "leaked CDN host: {serialized}");
    assert!(!serialized.contains("X-Amz-Signature"), "leaked query key: {serialized}");
    assert!(
        !serialized.contains("EXAMPLE_PRESIGNED_TOKEN"),
        "leaked presigned token: {serialized}",
    );
}

fn corp_scope_registries(upstream_url: &str) -> Value {
    json!({
        upstream_url: { "scopes": ["@corp"] },
    })
}

#[tokio::test]
async fn split_domain_private_upstream_routes_resolve_frozen_verify_and_tarball_download() {
    let tmp = TempDir::new().unwrap();
    let harness = setup_split_domain(&tmp).await;
    let expected_url = "http://example.test/npm/~corp/@corp/split/-/split-1.0.0.tgz";

    let fresh_frames = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/resolve",
        &harness.reader_token,
        json!({
            "dependencies": { "@corp/split": "1.0.0" },
            "registries": corp_scope_registries(&harness.upstream_url),
            "preferFrozenLockfile": false,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&fresh_frames, &harness.cdn_authority);
    let pkg_frame = fresh_frames
        .iter()
        .find(|f| f["type"] == "package")
        .unwrap_or_else(|| panic!("missing package frame: {fresh_frames:?}"));
    assert_eq!(pkg_frame["tarball"], expected_url);
    let done = fresh_frames
        .iter()
        .find(|f| f["type"] == "done")
        .unwrap_or_else(|| panic!("missing done frame: {fresh_frames:?}"));
    let lockfile = done["lockfile"].clone();
    assert_eq!(lockfile["packages"]["@corp/split@1.0.0"]["resolution"]["tarball"], expected_url);

    assert_cached_resolve_and_outsider_isolation(&harness, expected_url).await;
    assert_frozen_and_verify_round_trip(&harness, &lockfile, expected_url).await;
    assert_tamper_rejection(&harness, &lockfile).await;
    assert_tarball_proxy_download(&harness).await;
}

async fn assert_cached_resolve_and_outsider_isolation(
    harness: &SplitDomainHarness,
    expected_url: &str,
) {
    let cached_frames = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/resolve",
        &harness.reader_token,
        json!({
            "dependencies": { "@corp/split": "1.0.0" },
            "registries": corp_scope_registries(&harness.upstream_url),
            "preferFrozenLockfile": false,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&cached_frames, &harness.cdn_authority);
    let cached_done = cached_frames
        .iter()
        .find(|f| f["type"] == "done")
        .expect("cached done");
    assert_eq!(
        cached_done["lockfile"]["packages"]["@corp/split@1.0.0"]["resolution"]["tarball"],
        expected_url,
    );

    let outsider_frames = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/resolve",
        &harness.outsider_token,
        json!({
            "dependencies": { "@corp/split": "1.0.0" },
            "registries": corp_scope_registries(&harness.upstream_url),
            "preferFrozenLockfile": false,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&outsider_frames, &harness.cdn_authority);
    assert_eq!(outsider_frames[0]["type"], "error", "{outsider_frames:?}");
}

async fn assert_frozen_and_verify_round_trip(
    harness: &SplitDomainHarness,
    lockfile: &Value,
    expected_url: &str,
) {
    let frozen_frames = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/resolve",
        &harness.reader_token,
        json!({
            "dependencies": { "@corp/split": "1.0.0" },
            "registries": corp_scope_registries(&harness.upstream_url),
            "frozenLockfile": true,
            "trustLockfile": false,
            "minimumReleaseAge": 0,
            "lockfile": lockfile,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&frozen_frames, &harness.cdn_authority);
    let frozen_pkg = frozen_frames
        .iter()
        .find(|f| f["type"] == "package")
        .expect("frozen pkg");
    assert_eq!(frozen_pkg["tarball"], expected_url);
    let frozen_done = frozen_frames
        .iter()
        .find(|f| f["type"] == "done")
        .expect("frozen done");
    assert_eq!(
        frozen_done["lockfile"]["packages"]["@corp/split@1.0.0"]["resolution"]["tarball"],
        expected_url,
    );

    let verified = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/verify-lockfile",
        &harness.reader_token,
        json!({
            "registries": corp_scope_registries(&harness.upstream_url),
            "lockfile": lockfile,
        }),
    )
    .await;
    assert_eq!(verified.len(), 1, "{verified:?}");
    assert_eq!(verified[0]["type"], "done", "{verified:?}");
}

async fn assert_tamper_rejection(harness: &SplitDomainHarness, lockfile: &Value) {
    let mut tampered = lockfile.clone();
    tampered["packages"]["@corp/split@1.0.0"]["resolution"]["tarball"] =
        json!("http://example.test/npm/~corp/@corp/split/-/tampered-1.0.0.tgz");
    let violations = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/verify-lockfile",
        &harness.reader_token,
        json!({
            "registries": corp_scope_registries(&harness.upstream_url),
            "lockfile": tampered,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&violations, &harness.cdn_authority);
    assert_eq!(violations[0]["type"], "violations", "{violations:?}");

    // Directly supplying an unpublished synthetic `{upstream}/@corp/split/-/split-1.0.0.tgz`
    // URL (rather than the `/~corp/` route) must also be rejected, even after
    // the valid `/~corp/` lockfile was recorded in `verdict_cache`.
    let mut synthetic = lockfile.clone();
    synthetic["packages"]["@corp/split@1.0.0"]["resolution"]["tarball"] = json!(format!(
        "{}/@corp/split/-/split-1.0.0.tgz",
        harness.upstream_url.trim_end_matches('/'),
    ));
    let synthetic_violations = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/verify-lockfile",
        &harness.reader_token,
        json!({
            "registries": corp_scope_registries(&harness.upstream_url),
            "lockfile": synthetic,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&synthetic_violations, &harness.cdn_authority);
    assert_eq!(synthetic_violations[0]["type"], "violations", "{synthetic_violations:?}");

    // A routed peer-dependency variant in the same lockfile must not vouch for
    // a sibling entry that smuggles an unrouted synthetic upstream URL.
    let mut mixed_peers = lockfile.clone();
    let valid_entry = mixed_peers["packages"]["@corp/split@1.0.0"].clone();
    let mut forged_peer = valid_entry.clone();
    forged_peer["resolution"]["tarball"] = json!(format!(
        "{}/@corp/split/-/split-1.0.0.tgz",
        harness.upstream_url.trim_end_matches('/'),
    ));
    mixed_peers["packages"]["@corp/split@1.0.0(peer@1.0.0)"] = valid_entry;
    mixed_peers["packages"]["@corp/split@1.0.0(peer@2.0.0)"] = forged_peer;
    let mixed_violations = post_ndjson(
        &harness.app,
        "/-/pnpr/v0/verify-lockfile",
        &harness.reader_token,
        json!({
            "registries": corp_scope_registries(&harness.upstream_url),
            "lockfile": mixed_peers,
        }),
    )
    .await;
    assert_no_cdn_or_credential_leak(&mixed_violations, &harness.cdn_authority);
    assert_eq!(mixed_violations[0]["type"], "violations", "{mixed_violations:?}");

    let outsider_verify = harness.app
        .clone()
        .oneshot(
            Request::post("/-/pnpr/v0/verify-lockfile")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", harness.outsider_token))
                .body(Body::from(
                    json!({
                        "registries": corp_scope_registries(&harness.upstream_url),
                        "lockfile": lockfile,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(outsider_verify.status(), StatusCode::BAD_GATEWAY);
}

async fn assert_tarball_proxy_download(harness: &SplitDomainHarness) {
    let forbidden = harness.app
        .clone()
        .oneshot(
            Request::get("/npm/~corp/@corp/split/-/split-1.0.0.tgz")
                .header(header::AUTHORIZATION, format!("Bearer {}", harness.outsider_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    for path in ["/npm/~corp/@corp/split/-/split-1.0.0.tgz", "/npm/~corp/@corp/split@1.0.0"] {
        let downloaded = harness.app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, format!("Bearer {}", harness.reader_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(downloaded.status(), StatusCode::OK, "{path}");
        let bytes = body_bytes(downloaded.into_body()).await;
        assert_eq!(bytes, harness.tgz, "{path}");
    }
    assert_eq!(harness.cdn_hits.load(Ordering::SeqCst), 1);
}

async fn spawn_rotating_version_cdn(tgz: Vec<u8>) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = bind_loopback_preferring("127.0.0.3").await;
    let authority = listener
        .local_addr()
        .unwrap()
        .to_string();
    let expired_hits = Arc::new(AtomicUsize::new(0));
    let fresh_hits = Arc::new(AtomicUsize::new(0));
    let expired = Arc::clone(&expired_hits);
    let fresh = Arc::clone(&fresh_hits);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let tgz = tgz.clone();
            let expired = Arc::clone(&expired);
            let fresh = Arc::clone(&fresh);
            tokio::spawn(async move {
                serve_rotating_cdn_connection(&mut socket, &tgz, &expired, &fresh).await;
            });
        }
    });
    (authority, expired_hits, fresh_hits)
}

async fn serve_rotating_cdn_connection(
    socket: &mut tokio::net::TcpStream,
    tgz: &[u8],
    expired: &AtomicUsize,
    fresh: &AtomicUsize,
) {
    let Some(req) = read_http_headers(socket).await else {
        return;
    };
    if req.starts_with("GET /@corp/split@1.0.0?X-Amz-Signature=EXPIRED_TOKEN HTTP/") {
        expired.fetch_add(1, Ordering::SeqCst);
        let _ = socket.write_all(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
        return;
    }
    if !req.starts_with("GET /@corp/split@1.0.0?X-Amz-Signature=FRESH_TOKEN HTTP/") {
        let _ = socket.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await;
        return;
    }
    fresh.fetch_add(1, Ordering::SeqCst);
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        tgz.len(),
    );
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(tgz).await;
}

async fn spawn_rotating_packument_upstream(
    cdn_authority: String,
    tgz: Vec<u8>,
    rotates: bool,
) -> (String, Arc<AtomicUsize>) {
    let listener = bind_loopback_preferring("127.0.0.2").await;
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetches_for_task = Arc::clone(&fetches);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let step = fetches_for_task.fetch_add(1, Ordering::SeqCst);
            let sig = if !rotates || step == 0 { "EXPIRED_TOKEN" } else { "FRESH_TOKEN" };
            let doc = json!({
                "name": "@corp/split",
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "name": "@corp/split",
                        "version": "1.0.0",
                        "dist": {
                            "tarball": format!("http://{cdn_authority}/@corp/split@1.0.0?X-Amz-Signature={sig}"),
                            "integrity": sha512_integrity(&tgz),
                        }
                    }
                }
            })
            .to_string();
            tokio::spawn(async move {
                serve_packument_connection(&mut socket, &doc).await;
            });
        }
    });
    (url, fetches)
}

#[tokio::test]
async fn split_domain_tarball_proxy_recovers_expired_presigned_version_route_url() {
    let tmp = TempDir::new().unwrap();
    let (_, tgz) = package_tar_and_tgz(r#"{"name":"@corp/split","version":"1.0.0"}"#);
    let (cdn_authority, expired_hits, fresh_hits) = spawn_rotating_version_cdn(tgz.clone()).await;
    let (upstream_url, packument_fetches) =
        spawn_rotating_packument_upstream(cdn_authority, tgz.clone(), true).await;
    let config = build_split_config("http://127.0.0.1:1", &upstream_url, tmp.path().to_path_buf());
    let auth = seeded_auth(&config, &[("alice", "password")]);
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config, auth);

    // Warm the cached packument with the first (`EXPIRED_TOKEN`) signed URL.
    let packument_res = app
        .clone()
        .oneshot(
            Request::get("/npm/~corp/@corp/split")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(packument_res.status(), StatusCode::OK);
    assert_eq!(packument_fetches.load(Ordering::SeqCst), 1);

    // Downloading `@1.0.0` hits the cached `EXPIRED_TOKEN` URL (403), refreshes
    // the packument to `FRESH_TOKEN`, and succeeds on the same request.
    let tarball_res = app
        .oneshot(
            Request::get("/npm/~corp/@corp/split@1.0.0")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tarball_res.status(), StatusCode::OK);
    assert_eq!(body_bytes(tarball_res.into_body()).await, tgz);
    assert_eq!(expired_hits.load(Ordering::SeqCst), 1);
    assert_eq!(fresh_hits.load(Ordering::SeqCst), 1);
    assert_eq!(packument_fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn split_domain_tarball_proxy_throttles_packument_refetches_on_persistent_forbidden_url() {
    let tmp = TempDir::new().unwrap();
    let (_, tgz) = package_tar_and_tgz(r#"{"name":"@corp/split","version":"1.0.0"}"#);
    let (cdn_authority, expired_hits, _) = spawn_rotating_version_cdn(tgz.clone()).await;
    let (upstream_url, packument_fetches) =
        spawn_rotating_packument_upstream(cdn_authority, tgz, false).await;
    let config = build_split_config("http://127.0.0.1:1", &upstream_url, tmp.path().to_path_buf());
    let auth = seeded_auth(&config, &[("alice", "password")]);
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config, auth);

    let packument_res = app
        .clone()
        .oneshot(
            Request::get("/npm/~corp/@corp/split")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(packument_res.status(), StatusCode::OK);
    assert_eq!(packument_fetches.load(Ordering::SeqCst), 1);

    for path in [
        "/npm/~corp/@corp/split@1.0.0",
        "/npm/~corp/@corp/split@1.0.0",
        "/npm/~corp/@corp/split/-/split-1.0.0.tgz",
    ] {
        let res = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY, "{path}");
    }
    // 1 initial packument load + 1 throttled forced refresh across 3 failed downloads.
    assert_eq!(packument_fetches.load(Ordering::SeqCst), 2);
    assert_eq!(expired_hits.load(Ordering::SeqCst), 3);
}
