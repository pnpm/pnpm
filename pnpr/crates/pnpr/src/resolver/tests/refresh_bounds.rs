use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use pnpm_package_manager::{ResolutionObserver, ResolvedPackageHint};
use pnpm_resolving_resolver_base::PackageVersionGuard;
use pnpr_config::{IpNetwork, PublicRoute};
use pnpr_policy::Identity;
use pnpr_route::Footprint;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    time::Instant,
};

use super::{
    super::{
        Resolver,
        limits::{CancelToken, GraphLimitTracker, RESOLVE_TIMEOUT_CODE, STREAM_CHANNEL_CAPACITY},
        protocol::ResolveRequest,
        request_tarball_router,
        streaming::{
            StreamCacheTarget, StreamedResolve, StreamedResolveInputs, TerminalSender,
            stream_resolution,
        },
        wire::StreamObserver,
    },
    deps, registry_config, tarball_router,
};

struct PostFetchCancelObserver {
    inner: StreamObserver,
    cancel: CancelToken,
    cancel_after_packages: usize,
    resolved_packages: AtomicUsize,
    observed_cancel: Arc<AtomicBool>,
}

impl ResolutionObserver for PostFetchCancelObserver {
    fn on_resolution_id(&self, id: &str) {
        self.inner.on_resolution_id(id);
    }

    fn on_resolved(&self, hint: ResolvedPackageHint<'_>) {
        self.inner.on_resolved(hint);
        if self.resolved_packages.fetch_add(1, Ordering::Relaxed) + 1 >= self.cancel_after_packages
        {
            self.cancel.cancel();
        }
    }

    fn package_version_guard(&self) -> Option<Arc<dyn PackageVersionGuard>> {
        self.inner.package_version_guard()
    }

    fn max_snapshots(&self) -> Option<usize> {
        self.inner.max_snapshots()
    }

    fn cancellation_check(&self) -> Option<Arc<dyn Fn() -> bool + Send + Sync>> {
        let inner_check = self.inner.cancellation_check()?;
        let observed_cancel = Arc::clone(&self.observed_cancel);
        Some(Arc::new(move || {
            let cancelled = inner_check();
            if cancelled {
                observed_cancel.store(true, Ordering::Relaxed);
            }
            cancelled
        }))
    }
}

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

async fn write_http_ok(socket: &mut tokio::net::TcpStream, body: &str) {
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let _ = socket.write_all(reply.as_bytes()).await;
}

fn spawn_peer_packument_server(
    listener: TcpListener,
    server_origin: String,
    http_fetches: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let server_origin = server_origin.clone();
            let http_fetches = Arc::clone(&http_fetches);
            tokio::spawn(async move {
                let path = read_http_path(&mut socket).await;
                let pkg = path.trim_start_matches('/');
                http_fetches.fetch_add(1, Ordering::Relaxed);
                let integrity = ssri::Integrity::from(pkg.as_bytes()).to_string();
                let peer_deps = match pkg {
                    "pkg-a" => json!({ "pkg-b": "^1.0.0" }),
                    "pkg-b" => json!({ "pkg-a": "^1.0.0" }),
                    _ => json!({}),
                };
                let body = json!({
                    "name": pkg,
                    "dist-tags": { "latest": "1.0.0" },
                    "versions": {
                        "1.0.0": {
                            "name": pkg,
                            "version": "1.0.0",
                            "peerDependencies": peer_deps,
                            "dist": {
                                "tarball": format!("{server_origin}/{pkg}/-/{pkg}-1.0.0.tgz"),
                                "integrity": integrity,
                            },
                        },
                    },
                })
                .to_string();
                write_http_ok(&mut socket, &body).await;
            });
        }
    })
}

fn build_post_fetch_cancel_task(
    resolver: &Resolver,
    origin: &str,
    permit: super::super::limits::ResolvePermit,
    observed_cancel: Arc<AtomicBool>,
) -> (StreamedResolve, tokio::sync::mpsc::Receiver<Vec<u8>>) {
    let request = ResolveRequest {
        dependencies: Some(deps(&[("pkg-a", "1.0.0"), ("pkg-b", "1.0.0")])),
        registry: Some(format!("{origin}/")),
        trust_lockfile: true,
        prefer_frozen_lockfile: Some(false),
        ..ResolveRequest::default()
    };
    let active = resolver
        .config_for(&request)
        .ok()
        .unwrap();
    let identity = caller("u-1", "one");
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let request_auth = resolver.hooked_auth(&request, &identity, &footprint);
    let router = request_tarball_router(resolver, &identity, &active);
    let deadline = Instant::now() + Duration::from_secs(30);
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(STREAM_CHANNEL_CAPACITY);
    let (terminal, cancel) = TerminalSender::reserve_until(tx.clone(), deadline);
    let graph_limit = GraphLimitTracker::new(resolver.limits.max_npm_packages);
    let observer: Arc<dyn ResolutionObserver> = Arc::new(PostFetchCancelObserver {
        inner: StreamObserver {
            tx,
            package_version_guard: None,
            tarball_router: router.clone(),
            graph_limit: Arc::clone(&graph_limit),
            cancel: cancel.clone(),
        },
        cancel,
        cancel_after_packages: 2,
        resolved_packages: AtomicUsize::new(0),
        observed_cancel,
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
            tarball_router: router,
            footprint,
            cache_key: Some("peer-cancel-key".to_string()),
            permit: Arc::new(permit),
            deadline,
        },
        max_npm_packages: resolver.limits.max_npm_packages,
    };
    (task, rx)
}

#[tokio::test]
async fn npm_stream_emits_timeout_error_frame_when_deadline_expires_during_peer_resolution() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let http_fetches = Arc::new(AtomicUsize::new(0));
    let upstream = spawn_peer_packument_server(listener, origin.clone(), Arc::clone(&http_fetches));

    let tmp = TempDir::new().unwrap();
    let resolver = test_resolver(&origin, &tmp);
    let identity = caller("u-1", "one");
    let permit = resolver.limits
        .admit(&identity)
        .ok()
        .unwrap()
        .acquire(Instant::now() + Duration::from_secs(5))
        .await
        .ok()
        .unwrap();
    let observed_cancel = Arc::new(AtomicBool::new(false));
    let (task, mut rx) =
        build_post_fetch_cancel_task(&resolver, &origin, permit, Arc::clone(&observed_cancel));

    // Outer deadline is 30s in the future so `tokio::select!` cannot fire; only
    // synchronous `cancellation_check` polling inside peer resolution can abort.
    stream_resolution(task).await;
    assert!(
        observed_cancel.load(Ordering::Relaxed),
        "peer resolution must poll cancellation_check and observe the cancelled token",
    );

    let mut frames = Vec::new();
    while let Some(raw) = rx.recv().await {
        let trimmed = raw.strip_suffix(b"\n").unwrap_or(&raw);
        frames.push(serde_json::from_slice::<Value>(trimmed).unwrap());
    }
    assert_eq!(
        http_fetches.load(Ordering::Relaxed),
        2,
        "both packument fetches must complete before synchronous peer resolution cancels",
    );
    let package_frames = frames
        .iter()
        .filter(|frame| frame["type"] == "package")
        .count();
    assert_eq!(package_frames, 2, "expected 2 package frames before cancellation, got: {frames:?}");
    let last = frames.last().expect("stream must emit a terminal frame");
    assert_eq!(last["type"], "error", "expected terminal error frame, got: {last:?}");
    assert_eq!(last["code"], RESOLVE_TIMEOUT_CODE, "expected timeout code, got: {last:?}");
    assert!(
        resolver.cache.entries
            .lock()
            .unwrap()
            .is_empty(),
        "cancelled resolution must not populate the resolution cache",
    );

    upstream.abort();
}

#[tokio::test]
async fn stream_observer_cancellation_check_trips_on_deadline_disconnect_and_graph_limit() {
    let reg_cfg = registry_config();
    let router = tarball_router(&reg_cfg, Identity::Anonymous);

    // 1. Active token + open receiver + under limit => not cancelled.
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let graph_limit = GraphLimitTracker::new(1);
    let (cancel1, _guard1) = CancelToken::new(Instant::now() + Duration::from_secs(30));
    let observer = StreamObserver {
        tx,
        package_version_guard: None,
        tarball_router: router.clone(),
        graph_limit: Arc::clone(&graph_limit),
        cancel: cancel1,
    };
    let check = observer.cancellation_check().expect("stream observer exposes cancellation check");
    assert!(!check(), "fresh stream observer must not start cancelled");

    // 2. Exceeding graph limit trips synchronous cancellation immediately.
    for id in ["a@1.0.0", "b@1.0.0"] {
        observer.on_resolution_id(id);
    }
    assert!(check(), "exceeding graph limit must trip cancellation check");

    // 3. Receiver drop trips synchronous cancellation immediately.
    let (tx2, rx2) = tokio::sync::mpsc::channel(4);
    let (cancel2, _guard2) = CancelToken::new(Instant::now() + Duration::from_secs(30));
    let observer2 = StreamObserver {
        tx: tx2,
        package_version_guard: None,
        tarball_router: router.clone(),
        graph_limit: GraphLimitTracker::new(16),
        cancel: cancel2,
    };
    let check2 = observer2.cancellation_check().unwrap();
    assert!(!check2(), "open channel must not report cancellation");
    drop(rx2);
    assert!(check2(), "dropping receiver must trip cancellation check");
    drop(rx);

    // 4. Expired deadline trips synchronous cancellation immediately without yielding.
    let (tx3, _rx3) = tokio::sync::mpsc::channel(4);
    let (cancel3, _guard3) = CancelToken::new(Instant::now() - Duration::from_millis(1));
    let observer3 = StreamObserver {
        tx: tx3,
        package_version_guard: None,
        tarball_router: router,
        graph_limit: GraphLimitTracker::new(16),
        cancel: cancel3,
    };
    let check3 = observer3.cancellation_check().unwrap();
    assert!(check3(), "expired deadline must trip cancellation check without yielding");
}
