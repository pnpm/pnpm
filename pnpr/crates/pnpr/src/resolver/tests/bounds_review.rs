use std::{
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
    time::Instant,
};

use super::{
    super::{
        Resolver,
        dispatch::handle_resolve,
        limits::{GRAPH_TOO_LARGE_CODE, RESOLVE_TIMEOUT_CODE, request_timeout_response},
        streaming::{TerminalSender, send_before_deadline},
        wire::MAX_NDJSON_FRAME_BYTES,
    },
    cargo_done_frame, registry_config,
};

fn test_integrity() -> String {
    ssri::Integrity::from(b"pnpr-bounds-review-test-tarball").to_string()
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

fn caller(id: &str, name: &str) -> Identity {
    Identity::User {
        id: id.to_string(),
        username: name.to_string(),
        memberships: Vec::new().into(),
    }
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
async fn queue_admission_sheds_excess_requests_with_503_and_recovers() {
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
                let path = read_http_path(&mut socket).await;
                let name = path.trim_start_matches('/');
                if name == "slow" {
                    entered_srv.notify_waiters();
                    release_srv.notified().await;
                }
                let body = json!({
                    "name": name,
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": {
                        "1.0.0": {
                            "name": name,
                            "version": "1.0.0",
                            "dist": {
                                "tarball": format!("{server_origin}/{name}/-/{name}-1.0.0.tgz"),
                                "integrity": test_integrity(),
                            },
                        },
                    },
                })
                .to_string();
                write_json_ok(&mut socket, &body).await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    // 1 running slot, 2 total admitted (1 running + 1 queued), 2 per identity.
    resolver.limits_mut().set_concurrency(1, 1);
    resolver.limits_mut().set_admission(2, 2);
    let resolver = Arc::new(resolver);

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
    let active = handle_resolve(&resolver, caller("u-1", "one"), slow_body.clone()).await;
    slow_entered.await;

    let resolver_q = Arc::clone(&resolver);
    let queued = tokio::spawn(async move {
        handle_resolve(&resolver_q, caller("u-2", "two"), fast_body.clone()).await
    });
    tokio::task::yield_now().await;

    // 3rd concurrent request exceeds instance admission (2) and is shed immediately with 503.
    let shed = handle_resolve(&resolver, caller("u-3", "three"), slow_body).await;
    assert_eq!(shed.status(), StatusCode::SERVICE_UNAVAILABLE);

    release.notify_waiters();
    let (_, active_frames) = collect_frames(active).await;
    assert_eq!(active_frames.last().unwrap()["type"], "done");
    let (_, queued_frames) = collect_frames(queued.await.unwrap()).await;
    assert_eq!(queued_frames.last().unwrap()["type"], "done");

    // Once active and queued work settle, new requests are admitted again.
    let recovered = handle_resolve(
        &resolver,
        caller("u-3", "three"),
        Bytes::from(
            serde_json::to_vec(&json!({
                "dependencies": { "fast": "1.0.0" },
                "registry": format!("{origin}/"),
                "trustLockfile": true,
                "preferFrozenLockfile": false,
            }))
            .unwrap(),
        ),
    )
    .await;
    let (_, recovered_frames) = collect_frames(recovered).await;
    assert_eq!(recovered_frames.last().unwrap()["type"], "done");
    assert_eq!(resolver.limits.tracked_identities(), 0);

    upstream.abort();
}

#[tokio::test]
async fn queued_timeout_response_and_post_deadline_send_emit_timeout_code() {
    let response = request_timeout_response();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["code"], RESOLVE_TIMEOUT_CODE);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4);
    let terminal = TerminalSender::reserve(tx.clone());
    while tx.try_send(b"{\"type\":\"progress\"}\n".to_vec()).is_ok() {}
    drop(tx);
    let expired = Instant::now() - Duration::from_millis(1);
    send_before_deadline(terminal, expired, br#"{"type":"done"}"#.to_vec());

    let mut frames = Vec::new();
    while let Some(frame) = rx.recv().await {
        frames.push(serde_json::from_slice::<Value>(&frame).unwrap());
    }
    assert_eq!(frames.len(), 4, "3 hints + 1 reserved terminal frame");
    let parsed = frames.last().unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["code"], RESOLVE_TIMEOUT_CODE);
}

#[test]
fn oversized_terminal_frame_emits_graph_too_large_error() {
    let huge_lockfile = "x".repeat(MAX_NDJSON_FRAME_BYTES + 64);
    let frame = cargo_done_frame(&huge_lockfile);
    let compact = frame.len() < 1024;
    assert!(compact, "error frame must stay compact");
    let parsed: Value = serde_json::from_slice(&frame).unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["code"], GRAPH_TOO_LARGE_CODE);
}

#[tokio::test]
async fn reused_lockfile_packages_count_toward_live_graph_limit() {
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
                let body = json!({
                    "name": name,
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": {
                        "1.0.0": {
                            "name": name,
                            "version": "1.0.0",
                            "dist": {
                                "tarball": format!("{server_origin}/{name}/-/{name}-1.0.0.tgz"),
                                "integrity": test_integrity(),
                            },
                        },
                    },
                })
                .to_string();
                write_json_ok(&mut socket, &body).await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    // Input lockfile has 2 packages (`<= max_npm_packages`), so upfront check passes;
    // adding `fresh-c` brings the live total to 3 (`> 2`), which `on_reused` catches.
    resolver.limits_mut().max_npm_packages = 2;

    let prior_lockfile = json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": {
                "dependencies": {
                    "reused-a": { "specifier": "1.0.0", "version": "1.0.0" },
                    "reused-b": { "specifier": "1.0.0", "version": "1.0.0" },
                },
            },
        },
        "packages": {
            "reused-a@1.0.0": { "resolution": { "integrity": test_integrity() } },
            "reused-b@1.0.0": { "resolution": { "integrity": test_integrity() } },
        },
        "snapshots": {
            "reused-a@1.0.0": {},
            "reused-b@1.0.0": {},
        },
    });
    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "dependencies": {
                "reused-a": "1.0.0",
                "reused-b": "1.0.0",
                "fresh-c": "1.0.0",
            },
            "lockfile": prior_lockfile,
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let response = handle_resolve(&resolver, caller("u-1", "one"), body).await;
    let (status, frames) = collect_frames(response).await;
    assert_eq!(status, StatusCode::OK);
    let last = frames.last().expect("terminal frame");
    assert_eq!(last["type"], "error");
    assert_eq!(last["code"], GRAPH_TOO_LARGE_CODE);
    let fetched_count = fetched.load(Ordering::Relaxed);
    assert_eq!(
        fetched_count, 1,
        "reused-a and reused-b must be reused from the lockfile while still counting toward max_npm_packages",
    );

    upstream.abort();
}

fn peer_packument(origin: &str, name: &str) -> Value {
    match name {
        "leaf" => json!({
            "name": "leaf",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "leaf",
                    "version": "1.0.0",
                    "peerDependencies": { "p": "*", "q": "*", "r": "*" },
                    "dist": {
                        "tarball": format!("{origin}/leaf/-/leaf-1.0.0.tgz"),
                        "integrity": test_integrity(),
                    },
                },
            },
        }),
        "mid" => json!({
            "name": "mid",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "mid",
                    "version": "1.0.0",
                    "dependencies": { "leaf": "1.0.0" },
                    "dist": {
                        "tarball": format!("{origin}/mid/-/mid-1.0.0.tgz"),
                        "integrity": test_integrity(),
                    },
                },
            },
        }),
        _ => {
            let max_major = if name == "p" { 3 } else { 2 };
            let versions: serde_json::Map<String, Value> = (1..=max_major)
                .map(|major| {
                    let ver = format!("{major}.0.0");
                    let meta = json!({
                        "name": name,
                        "version": ver,
                        "dist": {
                            "tarball": format!("{origin}/{name}/-/{name}-{ver}.tgz"),
                            "integrity": test_integrity(),
                        },
                    });
                    (ver, meta)
                })
                .collect();
            json!({
                "name": name,
                "dist-tags": { "latest": "1.0.0" },
                "versions": versions,
            })
        }
    }
}

#[tokio::test]
async fn peer_variant_snapshots_exceeding_max_snapshots_fail_during_peer_walk() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let upstream = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let server_origin = server_origin.clone();
            tokio::spawn(async move {
                let path = read_http_path(&mut socket).await;
                let name = path.trim_start_matches('/');
                let body = peer_packument(&server_origin, name);
                write_json_ok(&mut socket, &body.to_string()).await;
            });
        }
    });

    let tmp = TempDir::new().unwrap();
    let mut resolver = test_resolver(&origin, &tmp);
    // 9 distinct packages (`p@1..3`, `q@1..2`, `r@1..2`, `leaf@1`, `mid@1`) fit within
    // `max_npm_packages = 9`, while 12 peer combinations produce 31 snapshots (`> 18`).
    resolver.limits_mut().max_npm_packages = 9;

    let projects: Vec<Value> = (0..12)
        .map(|idx| {
            let p_ver = format!("{}.0.0", (idx / 4) + 1);
            let q_ver = format!("{}.0.0", ((idx / 2) % 2) + 1);
            let r_ver = format!("{}.0.0", (idx % 2) + 1);
            json!({
                "dir": format!("packages/app-{idx}"),
                "dependencies": {
                    "p": p_ver,
                    "q": q_ver,
                    "r": r_ver,
                    "mid": "1.0.0",
                },
            })
        })
        .collect();
    let body = Bytes::from(
        serde_json::to_vec(&json!({
            "projects": projects,
            "registry": format!("{origin}/"),
            "trustLockfile": true,
            "preferFrozenLockfile": false,
        }))
        .unwrap(),
    );

    let response = handle_resolve(&resolver, caller("u-1", "one"), body).await;
    let (status, frames) = collect_frames(response).await;
    assert_eq!(status, StatusCode::OK);
    let last = frames.last().expect("terminal frame");
    assert_eq!(last["type"], "error");
    assert_eq!(last["code"], GRAPH_TOO_LARGE_CODE);
    let mentions_snapshots = last["message"]
        .as_str()
        .is_some_and(|msg| msg.contains("18 package snapshots"));
    assert!(mentions_snapshots, "expected live snapshot cap error, got {last:?}");

    upstream.abort();
}
