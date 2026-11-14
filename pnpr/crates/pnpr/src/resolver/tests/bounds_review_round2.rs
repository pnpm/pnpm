use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::{Body, Bytes, to_bytes},
    http::StatusCode,
};
use pnpm_lockfile::Lockfile;
use pnpr_config::{IpNetwork, PublicRoute};
use pnpr_policy::Identity;
use pnpr_route::Footprint;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::Notify,
    time::Instant,
};

use super::{
    super::{
        Resolver,
        cargo::{IndexFetcher, solve::solve_cargo},
        dispatch::handle_resolve,
        limits::{CancelToken, GraphLimitTracker, RESOLVE_TIMEOUT_CODE, STREAM_CHANNEL_CAPACITY},
        protocol::{AppliedPolicies, ResolveRequest},
        request_tarball_router, serve_resolve_request,
        streaming::{
            StreamCacheTarget, StreamedResolve, StreamedResolveInputs, TerminalSender,
            finish_before_deadline,
        },
        wire::{StreamObserver, try_done_frame_until},
    },
    deps, registry_config,
};

fn caller(id: &str, name: &str) -> Identity {
    Identity::User {
        id: id.to_string(),
        username: name.to_string(),
        memberships: Vec::new().into(),
    }
}

fn test_resolver(origin: &str, dir: &TempDir) -> Resolver {
    let mut config = registry_config();
    config.storage.cache_dir = dir.path().to_path_buf();
    config.http.public_url = "http://example.test".into();
    config.routing.route_policy.public.push(PublicRoute {
        registry: Some(format!("{origin}/")),
        package: None,
    });
    config.routing.route_policy.allowed_private_networks =
        vec![IpNetwork::parse("127.0.0.0/8").unwrap()];
    Resolver::build(&config, None)
}

async fn read_http_path(socket: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if socket.read(&mut byte).await.unwrap_or(0) == 0 {
            break;
        }
        buf.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&buf);
    head.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string()
}

async fn write_http_ok(socket: &mut tokio::net::TcpStream, content_type: &str, body: &str) {
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let _ = socket.write_all(reply.as_bytes()).await;
}

#[tokio::test]
async fn http_resolve_sheds_excess_callers_before_reading_or_parsing_body() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let entered_srv = Arc::clone(&entered);
    let release_srv = Arc::clone(&release);
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let server_origin = server_origin.clone();
            let entered_srv = Arc::clone(&entered_srv);
            let release_srv = Arc::clone(&release_srv);
            tokio::spawn(async move {
                let _ = read_http_path(&mut socket).await;
                entered_srv.notify_waiters();
                release_srv.notified().await;
                let integrity = ssri::Integrity::from(b"shed-test").to_string();
                let body = json!({
                    "name": "slow",
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": {
                        "1.0.0": {
                            "name": "slow",
                            "version": "1.0.0",
                            "dist": {
                                "tarball": format!("{server_origin}/slow/-/slow-1.0.0.tgz"),
                                "integrity": integrity,
                            },
                        },
                    },
                })
                .to_string();
                write_http_ok(&mut socket, "application/json", &body).await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    resolver.limits_mut().set_concurrency(1, 1);
    resolver.limits_mut().set_admission(1, 1);

    let active_body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": { "slow": "1.0.0" },
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );
    let slow_entered = entered.notified();
    let active = handle_resolve(&resolver, caller("u-1", "one"), active_body).await;
    slow_entered.await;

    // Streamed HTTP request with an unparsable payload that tracks whether any chunk was read.
    let body_polls = Arc::new(AtomicUsize::new(0));
    let polls_stream = Arc::clone(&body_polls);
    let stream = futures_util::stream::once(async move {
        polls_stream.fetch_add(1, Ordering::Relaxed);
        Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{invalid json"))
    });
    let request = axum::extract::Request::new(Body::from_stream(stream));
    let shed_http = serve_resolve_request(&resolver, caller("u-2", "two"), request).await;
    assert_eq!(shed_http.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body_polls.load(Ordering::Relaxed),
        0,
        "shed HTTP requests must not poll or buffer the request body",
    );

    // Direct Bytes entry point also sheds before JSON deserialization (503, not 400).
    let shed_direct =
        handle_resolve(&resolver, caller("u-2", "two"), Bytes::from_static(b"{invalid")).await;
    assert_eq!(shed_direct.status(), StatusCode::SERVICE_UNAVAILABLE);

    release.notify_waiters();
    let _ = to_bytes(active.into_body(), usize::MAX).await.unwrap();
    upstream.abort();
}

#[tokio::test]
async fn cargo_deadline_expiry_inside_resolve_returns_504_timeout() {
    let tmp = TempDir::new().unwrap();
    let resolver = test_resolver("http://127.0.0.1:9", &tmp);
    let identity = caller("u-1", "one");
    let permit = resolver.limits
        .admit(&identity)
        .ok()
        .unwrap()
        .acquire(Instant::now() + Duration::from_secs(5))
        .await
        .ok()
        .unwrap();
    // Empty dependency list skips `index.fetch_for` network work and reaches
    // `resolve_lockfile_until` inside `spawn_blocking` with an expired deadline.
    let empty_metadata = json!({
        "packages": [{
            "id": "demo 0.1.0",
            "name": "demo",
            "version": "0.1.0",
            "features": {},
            "dependencies": [],
        }],
        "workspace_members": ["demo 0.1.0"],
    })
    .to_string();
    let registry = "http://127.0.0.1:9".to_string();
    let source = pnpm_cargo_resolver::registry_source(&registry);
    let index = IndexFetcher::new(&resolver, identity.clone(), registry);
    let expired = Instant::now() - Duration::from_millis(1);
    let direct = solve_cargo(
        index,
        empty_metadata,
        source,
        std::collections::BTreeMap::new(),
        permit,
        expired,
    )
    .await;
    assert_eq!(direct.status(), StatusCode::GATEWAY_TIMEOUT);
    let direct_body: Value =
        serde_json::from_slice(&to_bytes(direct.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(direct_body["code"], RESOLVE_TIMEOUT_CODE);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let _path = read_http_path(&mut socket).await;
                std::future::pending::<()>().await;
            });
        }
    });

    let mut slow_resolver = test_resolver(&origin, &tmp);
    slow_resolver.limits_mut().deadline = Duration::from_millis(30);
    let serde_metadata = json!({
        "packages": [{
            "id": "demo 0.1.0",
            "name": "demo",
            "version": "0.1.0",
            "features": {},
            "dependencies": [{
                "name": "serde",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "req": "^1.0",
                "kind": null,
                "optional": false,
                "uses_default_features": true,
                "features": [],
            }],
        }],
        "workspace_members": ["demo 0.1.0"],
    })
    .to_string();
    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "ecosystem": "cargo",
            "registry": format!("{origin}/"),
            "metadata": serde_metadata,
        }))
        .unwrap(),
    );

    let response = handle_resolve(&slow_resolver, caller("u-2", "two"), body).await;
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let payload: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["code"], RESOLVE_TIMEOUT_CODE);

    upstream.abort();
}

fn synthetic_lockfile(package_count: usize) -> Lockfile {
    let integrity = ssri::Integrity::from(b"synthetic-lockfile-entry").to_string();
    let mut packages = serde_json::Map::with_capacity(package_count);
    for idx in 0..package_count {
        packages.insert(
            format!("pkg-{idx}@1.0.0"),
            json!({ "resolution": { "integrity": integrity } }),
        );
    }
    serde_json::from_value(json!({
        "lockfileVersion": "9.0",
        "importers": { ".": {} },
        "packages": packages,
    }))
    .expect("synthetic lockfile parses")
}

#[test]
fn try_done_frame_until_stops_mid_serialization_when_cancelled() {
    let lockfile = synthetic_lockfile(2_500);
    let applied = AppliedPolicies::from_request(&ResolveRequest::default());
    let checks = AtomicUsize::new(0);
    let cancelled = try_done_frame_until(&lockfile, &applied, &|| {
        // Allow entry check to pass, then cancel on the first 64 KiB writer checkpoint.
        checks.fetch_add(1, Ordering::Relaxed) >= 2
    });
    assert!(cancelled.is_none());
    assert!(checks.load(Ordering::Relaxed) >= 2);
}

#[tokio::test]
async fn finish_before_deadline_drops_sender_for_immediate_eof_and_skips_cache_on_timeout() {
    let tmp = TempDir::new().unwrap();
    let resolver = test_resolver("http://127.0.0.1:9", &tmp);
    let request =
        ResolveRequest { dependencies: Some(deps(&[("pkg-0", "1.0.0")])), ..Default::default() };
    let active = resolver
        .config_for(&request)
        .ok()
        .unwrap();
    let identity = caller("u-1", "one");
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = resolver.hooked_auth(&request, &identity, &footprint);
    let tarball_router = request_tarball_router(&resolver, &identity, &active);
    let permit_deadline = Instant::now() + Duration::from_secs(5);
    let permit = resolver.limits
        .admit(&identity)
        .ok()
        .unwrap()
        .acquire(permit_deadline)
        .await
        .ok()
        .unwrap();

    let expired_deadline = Instant::now() - Duration::from_millis(1);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(STREAM_CHANNEL_CAPACITY);
    let terminal = TerminalSender::reserve(tx.clone());
    let graph_limit = GraphLimitTracker::new(resolver.limits.max_npm_packages);
    let observer: Arc<dyn pnpm_package_manager::ResolutionObserver> = Arc::new(StreamObserver {
        tx,
        package_version_guard: None,
        tarball_router: tarball_router.clone(),
        graph_limit: Arc::clone(&graph_limit),
        cancel: CancelToken::never(),
    });
    let task = StreamedResolve {
        client: Arc::clone(&resolver.client),
        observer,
        graph_limit,
        osv_index: None,
        cache: StreamCacheTarget {
            entries: Arc::clone(&resolver.cache.entries),
            ttl: resolver.cache.ttl,
            secret: Arc::clone(&resolver.cache.secret),
        },
        terminal,
        inputs: StreamedResolveInputs {
            config: active,
            request,
            request_auth,
            tarball_router,
            footprint,
            cache_key: Some("timed-out-key".to_string()),
            permit: Arc::new(permit),
            deadline: expired_deadline,
        },
        max_npm_packages: resolver.limits.max_npm_packages,
    };

    finish_before_deadline(task, Ok(synthetic_lockfile(512))).await;

    let frame = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await
        .expect("timeout frame should arrive promptly")
        .expect("timeout frame");
    let parsed: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["code"], RESOLVE_TIMEOUT_CODE);

    let eof = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await
        .expect("response stream must close immediately after timeout frame");
    assert!(eof.is_none());
    assert!(
        resolver.cache.entries
            .lock()
            .unwrap()
            .is_empty(),
    );
}
