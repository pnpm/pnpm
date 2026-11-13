use std::{
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    body::{Bytes, to_bytes},
    http::StatusCode,
    response::Response,
};
use pnpr_config::{IpNetwork, PublicRoute};
use pnpr_policy::Identity;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::Notify,
};

use super::{
    super::{
        Resolver,
        dispatch::handle_resolve,
        limits::{
            GRAPH_TOO_LARGE_CODE, MAX_CONCURRENT_RESOLVES, MAX_DIRECT_DEPENDENCIES,
            MAX_WORKSPACE_PROJECTS, RESOLVE_TIMEOUT_CODE, STREAM_CHANNEL_CAPACITY,
        },
        verify_lockfile::handle_verify_lockfile,
    },
    package_lockfile, registry_config,
};

fn test_integrity() -> String {
    ssri::Integrity::from(b"pnpr-bounds-test-tarball").to_string()
}

fn test_resolver(origin: &str, dir: &TempDir) -> Resolver {
    let mut config = registry_config();
    config.storage.cache_dir = dir.path().to_path_buf();
    config.routing.route_policy.public.push(PublicRoute {
        registry: Some(format!("{origin}/")),
        package: None,
    });
    config.routing.route_policy.allowed_private_networks =
        vec![IpNetwork::parse("127.0.0.0/8").unwrap()];
    Resolver::build(&config, None)
}

fn identity_with_id(id: &str, name: &str) -> Identity {
    Identity::User {
        id: id.to_string(),
        username: name.to_string(),
        memberships: Vec::new().into(),
    }
}

fn packument_json(origin: &str, name: &str, deps: &Value) -> String {
    json!({
        "name": name,
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": name,
                "version": "1.0.0",
                "dependencies": deps,
                "dist": {
                    "tarball": format!("{origin}/{name}/-/{name}-1.0.0.tgz"),
                    "integrity": test_integrity(),
                },
            },
        },
    })
    .to_string()
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

async fn write_json_ok(socket: &mut tokio::net::TcpStream, body: &str) {
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let _ = socket.write_all(reply.as_bytes()).await;
}

async fn spawn_stalled_upstream(entered: Arc<Notify>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let handle = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let entered = Arc::clone(&entered);
            let server_origin = server_origin.clone();
            tokio::spawn(async move {
                let path = read_http_path(&mut socket).await;
                if path == "/first" {
                    let body =
                        packument_json(&server_origin, "first", &json!({ "stalled": "1.0.0" }));
                    write_json_ok(&mut socket, &body).await;
                } else {
                    entered.notify_waiters();
                    pending::<()>().await;
                }
            });
        }
    });
    (origin, handle)
}

async fn collect_frames(response: Response) -> (StatusCode, Vec<Value>) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let frames = bytes
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect();
    (status, frames)
}

#[tokio::test]
async fn disconnect_aborts_streaming_resolve_and_releases_permits() {
    let entered = Arc::new(Notify::new());
    let (origin, upstream) = spawn_stalled_upstream(Arc::clone(&entered)).await;
    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    resolver.limits_mut().set_concurrency(2, 1);

    let caller = identity_with_id("u-1", "caller-one");
    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": { "first": "1.0.0" },
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let waited = entered.notified();
    let response = handle_resolve(&resolver, caller.clone(), body).await;
    assert_eq!(response.status(), StatusCode::OK);
    waited.await;

    assert_eq!(resolver.limits.available_instance_permits(), 1);
    assert_eq!(resolver.limits.tracked_identities(), 1);

    // Dropping the response body simulates the HTTP client disconnecting mid-stream.
    drop(response);

    tokio::time::timeout(Duration::from_secs(1), async {
        while resolver.limits.available_instance_permits() != 2
            || resolver.limits.tracked_identities() != 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("aborting the disconnected stream must promptly release both permits");

    upstream.abort();
}

#[tokio::test]
async fn overall_deadline_aborts_streaming_resolve_after_headers_are_sent() {
    let entered = Arc::new(Notify::new());
    let (origin, upstream) = spawn_stalled_upstream(Arc::clone(&entered)).await;
    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    resolver.limits_mut().deadline = Duration::from_secs(60);

    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": { "first": "1.0.0" },
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let stalled = entered.notified();
    let response = handle_resolve(&resolver, identity_with_id("u-1", "caller-one"), body).await;
    tokio::time::timeout(Duration::from_secs(30), stalled).await
        .expect("resolution must reach the stalled dependency before advancing the deadline");
    tokio::time::pause();
    tokio::time::advance(resolver.limits.deadline).await;
    let (status, frames) = tokio::time::timeout(Duration::from_secs(2), collect_frames(response))
        .await
        .expect("overall deadline must terminate the streamed body");

    assert_eq!(status, StatusCode::OK);
    assert!(
        frames
            .iter()
            .any(|frame| frame["type"] == "package" && frame["name"] == "first"),
        "expected streamed package frame before stalled wave: {frames:?}",
    );
    let last = frames.last().expect("terminal frame");
    assert_eq!(last["type"], "error");
    assert_eq!(last["code"], RESOLVE_TIMEOUT_CODE);
    assert_eq!(resolver.limits.available_instance_permits(), MAX_CONCURRENT_RESOLVES);
    assert_eq!(resolver.limits.tracked_identities(), 0);

    upstream.abort();
}

#[tokio::test]
async fn per_identity_semaphore_prevents_one_caller_from_starving_others() {
    let entered = Arc::new(Notify::new());
    let release_slow = Arc::new(Notify::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let entered_srv = Arc::clone(&entered);
    let release_srv = Arc::clone(&release_slow);
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let server_origin = server_origin.clone();
            let entered_srv = Arc::clone(&entered_srv);
            let release_srv = Arc::clone(&release_srv);
            tokio::spawn(async move {
                let path = read_http_path(&mut socket).await;
                if path == "/slow" {
                    entered_srv.notify_waiters();
                    release_srv.notified().await;
                    write_json_ok(&mut socket, &packument_json(&server_origin, "slow", &json!({})))
                        .await;
                } else {
                    write_json_ok(&mut socket, &packument_json(&server_origin, "fast", &json!({})))
                        .await;
                }
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    // 2 instance permits, 1 per identity: noisy caller can hold at most 1 of the 2.
    resolver.limits_mut().set_concurrency(2, 1);
    let resolver = Arc::new(resolver);

    let noisy = identity_with_id("u-noisy", "noisy");
    let peer = identity_with_id("u-peer", "peer");
    let slow_body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": { "slow": "1.0.0" },
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );
    let fast_body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": { "fast": "1.0.0" },
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let slow_entered = entered.notified();
    let first_slow = handle_resolve(&resolver, noisy.clone(), slow_body.clone()).await;
    slow_entered.await;

    // Second request from `noisy` queues on `noisy`'s per-identity permit.
    let resolver_queued = Arc::clone(&resolver);
    let noisy_queued = noisy.clone();
    let queued =
        tokio::spawn(
            async move { handle_resolve(&resolver_queued, noisy_queued, slow_body).await },
        );
    tokio::task::yield_now().await;

    // `peer` still acquires the second instance permit and resolves immediately.
    let peer_response =
        tokio::time::timeout(Duration::from_secs(2), handle_resolve(&resolver, peer, fast_body))
            .await
            .expect("peer identity must not be blocked behind noisy caller");
    let (_, peer_frames) = collect_frames(peer_response).await;
    assert_eq!(peer_frames.last().unwrap()["type"], "done");

    release_slow.notify_waiters();
    let (_, first_frames) = collect_frames(first_slow).await;
    assert_eq!(first_frames.last().unwrap()["type"], "done");
    let (_, second_frames) = collect_frames(queued.await.unwrap()).await;
    assert_eq!(second_frames.last().unwrap()["type"], "done");
    assert_eq!(resolver.limits.tracked_identities(), 0);

    upstream.abort();
}

#[tokio::test]
async fn live_graph_limit_aborts_optional_dependencies_walk() {
    let fetched = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let fetched_srv = Arc::clone(&fetched);
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let server_origin = server_origin.clone();
            let fetched_srv = Arc::clone(&fetched_srv);
            tokio::spawn(async move {
                let path = read_http_path(&mut socket).await;
                let name = path.trim_start_matches('/');
                fetched_srv.fetch_add(1, Ordering::Relaxed);
                write_json_ok(&mut socket, &packument_json(&server_origin, name, &json!({}))).await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    resolver.limits_mut().max_npm_packages = 2;

    // `optionalDependencies` swallow individual `Resolver::resolve` errors in
    // the tree walker, so only task-level cancellation via `GraphLimitTracker`
    // prevents resolving and returning a partial graph.
    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "optionalDependencies": {
                "opt-one": "1.0.0",
                "opt-two": "1.0.0",
                "opt-three": "1.0.0",
                "opt-four": "1.0.0",
            },
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let response = handle_resolve(&resolver, identity_with_id("u-1", "caller-one"), body).await;
    let (status, frames) = collect_frames(response).await;
    assert_eq!(status, StatusCode::OK);
    let last = frames.last().expect("terminal frame");
    assert_eq!(last["type"], "error");
    assert_eq!(last["code"], GRAPH_TOO_LARGE_CODE);
    assert!(
        frames
            .iter()
            .filter(|frame| frame["type"] == "package")
            .count()
            <= 2,
        "must not emit package frames past max_npm_packages: {frames:?}",
    );

    upstream.abort();
}

#[tokio::test]
async fn upfront_graph_limit_rejects_oversized_input_lockfile_on_resolve_and_verify() {
    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver("http://127.0.0.1:1", &tmp);
    resolver.limits_mut().max_npm_packages = 1;

    let mut oversized = package_lockfile("pkg-a", "1.0.0");
    let extra = package_lockfile("pkg-b", "1.0.0");
    oversized.packages
        .as_mut()
        .unwrap()
        .extend(extra.packages.unwrap());

    let payload = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": { "pkg-a": "^1.0.0" },
            "registry": "https://registry.npmjs.org/",
            "lockfile": oversized,
        }))
        .unwrap(),
    );
    let caller = identity_with_id("u-1", "caller-one");

    let resolve_resp = handle_resolve(&resolver, caller.clone(), payload.clone()).await;
    assert_eq!(resolve_resp.status(), StatusCode::BAD_REQUEST);
    let resolve_err: Value =
        serde_json::from_slice(&to_bytes(resolve_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(resolve_err["code"], GRAPH_TOO_LARGE_CODE);

    let verify_resp = handle_verify_lockfile(&resolver, caller, payload).await;
    assert_eq!(verify_resp.status(), StatusCode::BAD_REQUEST);
    let verify_err: Value =
        serde_json::from_slice(&to_bytes(verify_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(verify_err["code"], GRAPH_TOO_LARGE_CODE);
}

#[tokio::test]
async fn bounded_channel_completes_without_deadlock_when_packages_exceed_capacity() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let server_origin = server_origin.clone();
            tokio::spawn(async move {
                let path = read_http_path(&mut socket).await;
                let name = path.trim_start_matches('/');
                write_json_ok(&mut socket, &packument_json(&server_origin, name, &json!({}))).await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let resolver = test_resolver(&origin, &tmp);

    let count = STREAM_CHANNEL_CAPACITY + 16;
    let deps: serde_json::Map<String, Value> = (0..count)
        .map(|idx| (format!("leaf-{idx}"), Value::String("1.0.0".to_string())))
        .collect();
    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": deps,
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let response = handle_resolve(&resolver, identity_with_id("u-1", "caller-one"), body).await;
    // Pause before draining so `StreamObserver` fills the bounded channel
    // while `stream_resolution` runs in the background.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (status, frames) = tokio::time::timeout(Duration::from_secs(10), collect_frames(response))
        .await
        .expect("bounded channel must deliver terminal done frame without deadlocking");

    assert_eq!(status, StatusCode::OK);
    let last = frames.last().expect("terminal frame");
    assert_eq!(last["type"], "done");
    assert_eq!(last["stats"]["totalPackages"], count);

    upstream.abort();
}

#[tokio::test]
async fn upfront_graph_limit_rejects_oversized_projects_and_empty_projects_bypass() {
    let tmp = TempDir::new().unwrap();
    let resolver = test_resolver("http://127.0.0.1:1", &tmp);
    let caller = identity_with_id("u-1", "caller-one");

    let too_many_projects: Vec<Value> = (0..=MAX_WORKSPACE_PROJECTS)
        .map(|idx| json!({ "dir": format!("packages/p-{idx}") }))
        .collect();
    let projects_payload = Bytes::from(
        serde_json::to_vec(&json!({
            "projects": too_many_projects,
            "registry": "https://registry.npmjs.org/",
        }))
        .unwrap(),
    );
    let projects_resp = handle_resolve(&resolver, caller.clone(), projects_payload).await;
    assert_eq!(projects_resp.status(), StatusCode::BAD_REQUEST);
    let projects_err: Value =
        serde_json::from_slice(&to_bytes(projects_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(projects_err["code"], GRAPH_TOO_LARGE_CODE);

    // `"projects": []` normalizes to the top-level dependency maps; ensure an
    // empty `projects` array cannot bypass `MAX_DIRECT_DEPENDENCIES`.
    let oversized_deps: serde_json::Map<String, Value> = (0..=MAX_DIRECT_DEPENDENCIES)
        .map(|idx| (format!("dep-{idx}"), Value::String("1.0.0".to_string())))
        .collect();
    let bypass_payload = Bytes::from(
        serde_json::to_vec(&json!({
            "projects": [],
            "dependencies": oversized_deps,
            "registry": "https://registry.npmjs.org/",
        }))
        .unwrap(),
    );
    let bypass_resp = handle_resolve(&resolver, caller, bypass_payload).await;
    assert_eq!(bypass_resp.status(), StatusCode::BAD_REQUEST);
    let bypass_err: Value =
        serde_json::from_slice(&to_bytes(bypass_resp.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(bypass_err["code"], GRAPH_TOO_LARGE_CODE);
}
