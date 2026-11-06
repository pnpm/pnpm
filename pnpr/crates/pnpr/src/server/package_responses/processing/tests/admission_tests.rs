use super::{
    AppState, Arc, AtomicBool, CanonicalPackageName, Config, MockConfigPresets, Ordering,
    PackumentProcessing, TempDir, Workers, document,
};

#[tokio::test]
async fn admission_precedes_polling_the_source_loader() {
    use std::{future::Future, task::Poll};
    let processing = PackumentProcessing {
        admissions: super::super::admission::Admissions::new(1),
        ..PackumentProcessing::default()
    };
    let permit = processing.admissions.stored.acquire().await.unwrap();
    let loaded = AtomicBool::new(false);
    let source = async {
        loaded.store(true, Ordering::SeqCst);
        std::future::pending::<axum::response::Response>().await
    };
    let mut response = Box::pin(processing.admissions.serve(
        super::super::admission::DocumentSource::Stored,
        source,
    ));
    std::future::poll_fn(|context| {
        let result = response.as_mut().poll(context);
        assert!(result.is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(!loaded.load(Ordering::SeqCst));
    drop(permit);
    std::future::poll_fn(|context| {
        let result = response.as_mut().poll(context);
        assert!(result.is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(loaded.load(Ordering::SeqCst));
    drop(response);
    assert_eq!(processing.admissions.stored.available_permits(), 1);
}

#[tokio::test]
async fn slow_origins_do_not_hold_stored_or_other_upstream_admission() {
    let admissions = super::super::admission::Admissions::new(1);
    let slow = admissions.upstream("slow-example");
    let permit = slow.acquire().await.unwrap();
    let stored =
        admissions.serve(super::super::admission::DocumentSource::Stored, async { "stored" });
    let stored = tokio::time::timeout(std::time::Duration::from_secs(5), stored)
        .await
        .unwrap();
    assert_eq!(stored, "stored");
    let other = admissions.serve(
        super::super::admission::DocumentSource::Upstream("other-example"),
        async { "other" },
    );
    let other = tokio::time::timeout(std::time::Duration::from_secs(5), other)
        .await
        .unwrap();
    assert_eq!(other, "other");
    drop(permit);
}

#[tokio::test]
async fn slow_upstream_misses_do_not_block_a_fresh_cached_package() {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let upstream = Router::new()
        .route(
            "/{*name}",
            get(move || {
                let started = started_tx.clone();
                async move {
                    started.send(()).unwrap();
                    std::future::pending::<StatusCode>().await
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let tmp = TempDir::new().unwrap();
    let mut config = Config::proxy("127.0.0.1:0".parse().unwrap(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().url = format!("http://{address}");
    let (state, app) =
        crate::server::routing::state_and_router(config, pnpr_auth::AuthState::in_memory(), None)
            .unwrap();
    let namespace = crate::server::upstream_cache_namespace(&state, "npmjs");
    let name =
        CanonicalPackageName::parse("example-widget", pnpr_package_name::Ecosystem::Npm).unwrap();
    state.inner.storage
        .write_upstream_document(&namespace, &name, &document())
        .await
        .unwrap();
    let admissions = &state.inner.packuments.admissions;
    let slots = admissions.upstream(&namespace);
    let mut requests = Vec::new();
    for index in 0..admissions.capacity {
        let app = app.clone();
        requests.push(tokio::spawn(async move {
            let request = Request::get(format!("/npm/slow-example-{index}"))
                .body(Body::empty())
                .unwrap();
            app.oneshot(request).await.unwrap()
        }));
    }
    started_rx.recv().await.unwrap();
    let wait_for_saturation = async {
        while slots.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), wait_for_saturation)
        .await
        .unwrap();
    let request = Request::get("/npm/example-widget")
        .body(Body::empty())
        .unwrap();
    let cached = tokio::time::timeout(std::time::Duration::from_secs(5), app.oneshot(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cached.status(), StatusCode::OK);
    for request in requests {
        request.abort();
        let result = request.await;
        assert!(matches!(result, Err(error) if error.is_cancelled()));
    }
    origin.abort();
    let result = origin.await;
    assert!(matches!(result, Err(error) if error.is_cancelled()));
}

#[tokio::test]
async fn upstream_admission_has_an_instance_wide_bound() {
    use super::super::admission::{Admissions, DocumentSource};
    use std::future::Future;

    let admissions = Admissions::new(2);
    let (started, mut received) = tokio::sync::mpsc::unbounded_channel();
    let first = admissions.serve(DocumentSource::Upstream("first-example"), async {
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    let second = admissions.serve(DocumentSource::Upstream("second-example"), async {
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    let occupied = async {
        tokio::join!(first, second);
    };
    tokio::pin!(occupied);
    tokio::select! {
        () = &mut occupied => panic!("upstreams should remain pending"),
        () = async { received.recv().await.unwrap(); received.recv().await.unwrap(); } => {}
    }
    let polled = std::sync::atomic::AtomicBool::new(false);
    let third = admissions.serve(DocumentSource::Upstream("third-example"), async {
        polled.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    tokio::pin!(third);
    std::future::poll_fn(|context| {
        assert!(
            third
                .as_mut()
                .poll(context)
                .is_pending(),
        );
        std::task::Poll::Ready(())
    })
    .await;
    assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
    admissions.serve(DocumentSource::Stored, async {}).await;
}

#[tokio::test]
async fn shared_upstream_readers_admit_sources_before_waiting_for_workers() {
    let mut upstream = controlled_upstream(true, 2).await;
    let state = upstream.state.clone();
    let namespace = crate::server::upstream_cache_namespace(&state, "npmjs");
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_state = state.clone();
    let worker = tokio::spawn(async move {
        worker_state.inner.packuments.workers
            .run(move || {
                started_tx.send(()).unwrap();
                // Sender teardown also releases the worker if an assertion fails.
                release_rx.recv().unwrap_or_default();
            })
            .await
            .unwrap();
    });
    started_rx.await.unwrap();
    let mut requests = Vec::new();
    for index in 0..2 {
        let state = state.clone();
        let namespace = namespace.clone();
        requests.push(tokio::spawn(async move {
            let name = CanonicalPackageName::parse(
                &format!("example-widget-{index}"),
                pnpr_package_name::Ecosystem::Npm,
            )
            .unwrap();
            crate::server::load_upstream_packument(
                &state,
                &namespace,
                state.inner.proxy.upstreams.get("npmjs").unwrap(),
                &name,
                std::time::Duration::from_mins(5),
            )
            .await
            .unwrap()
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), upstream.fetched.recv())
        .await
        .unwrap()
        .unwrap();
    let misses = format!("pnpr_cache_misses_total {}", requests.len());
    let readers_queued = async {
        while !state.inner.metrics
            .render_prometheus()
            .lines()
            .any(|line| line == misses)
        {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), readers_queued)
        .await
        .unwrap();
    let slots = state.inner.packuments.admissions.upstream(&namespace);
    assert_eq!(slots.available_permits(), 0);
    assert!(upstream.fetched.try_recv().is_err());
    for request in requests {
        request.abort();
        let result = request.await;
        assert!(matches!(result, Err(error) if error.is_cancelled()));
    }
    release_tx.send(()).unwrap();
    worker.await.unwrap();
    drop(state);
    stop_upstream(upstream).await;
}

struct ControlledUpstream {
    state: AppState,
    fetched: tokio::sync::mpsc::UnboundedReceiver<String>,
    release: Arc<tokio::sync::Notify>,
    origin: tokio::task::JoinHandle<()>,
    _storage: TempDir,
}

async fn controlled_upstream(cache: bool, capacity: usize) -> ControlledUpstream {
    use axum::{Json, Router, extract::Path, routing::get};

    let release = Arc::new(tokio::sync::Notify::new());
    let origin_release = Arc::clone(&release);
    let (fetched_tx, fetched) = tokio::sync::mpsc::unbounded_channel();
    let upstream = Router::new()
        .route(
            "/{*name}",
            get(move |Path(name): Path<String>| {
                let fetched = fetched_tx.clone();
                let release = Arc::clone(&origin_release);
                async move {
                    fetched.send(name.clone()).unwrap();
                    if name == "held-example" {
                        release.notified().await;
                    }
                    Json(serde_json::json!({"name": name, "versions": {}}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });
    let storage = TempDir::new().unwrap();
    let mut config = Config::proxy("127.0.0.1:0".parse().unwrap(), storage.path().to_path_buf());
    let upstream = config.routing.upstreams.get_mut("npmjs").unwrap();
    upstream.url = format!("http://{address}");
    upstream.cache = cache;
    let mut inner =
        crate::server::state::AppInner::new(config, pnpr_auth::AuthState::in_memory(), None)
            .unwrap();
    inner.packuments = PackumentProcessing {
        workers: Workers::new(1),
        admissions: super::super::admission::Admissions::new(capacity),
        ..PackumentProcessing::default()
    };
    ControlledUpstream {
        state: AppState { inner: Arc::new(inner) },
        fetched,
        release,
        origin,
        _storage: storage,
    }
}

async fn read_upstream(state: &AppState, name: &str) -> Option<Vec<u8>> {
    let namespace = crate::server::upstream_cache_namespace(state, "npmjs");
    let name = CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm).unwrap();
    crate::server::load_upstream_packument(
        state,
        &namespace,
        state.inner.proxy.upstreams.get("npmjs").unwrap(),
        &name,
        std::time::Duration::from_mins(5),
    )
    .await
    .unwrap()
}

async fn stop_upstream(upstream: ControlledUpstream) {
    upstream.origin.abort();
    let result = upstream.origin.await;
    assert!(matches!(result, Err(error) if error.is_cancelled()));
}

#[tokio::test]
async fn duplicate_fetch_followers_leave_admission_for_other_packages() {
    use std::future::Future;

    let mut upstream = controlled_upstream(false, 4).await;
    let state = upstream.state.clone();
    let first = tokio::spawn(async move { read_upstream(&state, "held-example").await });
    let fetched = tokio::time::timeout(std::time::Duration::from_secs(5), upstream.fetched.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched, "held-example");
    {
        let follower = read_upstream(&upstream.state, "held-example");
        let other_follower = read_upstream(&upstream.state, "held-example");
        tokio::pin!(follower, other_follower);
        std::future::poll_fn(|context| {
            assert!(
                follower
                    .as_mut()
                    .poll(context)
                    .is_pending(),
            );
            assert!(
                other_follower
                    .as_mut()
                    .poll(context)
                    .is_pending(),
            );
            std::task::Poll::Ready(())
        })
        .await;
        let namespace = crate::server::upstream_cache_namespace(&upstream.state, "npmjs");
        let slots = upstream.state.inner.packuments.admissions.upstream(&namespace);
        assert_eq!(slots.available_permits(), 1);
        let other = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_upstream(&upstream.state, "other-example"),
        )
        .await
        .unwrap();
        assert!(other.is_some());
    }
    upstream.release.notify_one();
    let first = first.await.unwrap();
    assert!(first.is_some());
    stop_upstream(upstream).await;
}

#[tokio::test]
async fn uncached_upstream_reads_do_not_wait_for_stored_admission() {
    let upstream = controlled_upstream(false, 4).await;
    let stored = upstream.state.inner.packuments.admissions.stored.acquire_many(4).await.unwrap();
    let document = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        read_upstream(&upstream.state, "other-example"),
    )
    .await
    .unwrap();
    assert!(document.is_some());
    drop(stored);
    stop_upstream(upstream).await;
}

#[tokio::test]
async fn rendered_cache_and_response_bytes_do_not_retain_fetch_admission() {
    let upstream = controlled_upstream(true, 4).await;
    let name =
        CanonicalPackageName::parse("other-example", pnpr_package_name::Ecosystem::Npm).unwrap();
    let response = crate::server::serve_packument_via_upstream(
        &upstream.state,
        &crate::server::Identity::Anonymous,
        &super::HeaderMap::new(),
        "npmjs",
        &name,
        "https://registry.example",
        super::view(),
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let namespace = crate::server::upstream_cache_namespace(&upstream.state, "npmjs");
    let slots = upstream.state.inner.packuments.admissions.upstream(&namespace);
    assert_eq!(slots.available_permits(), 2);
    drop(response);
    stop_upstream(upstream).await;
}

#[tokio::test]
async fn fetched_responses_wait_for_stored_capacity_before_rendering() {
    let mut upstream = controlled_upstream(false, 4).await;
    let stored = upstream.state.inner.packuments.admissions.stored.acquire_many(4).await.unwrap();
    let state = upstream.state.clone();
    let mut request = tokio::spawn(async move {
        let name = CanonicalPackageName::parse("other-example", pnpr_package_name::Ecosystem::Npm)
            .unwrap();
        crate::server::serve_packument_via_upstream(
            &state,
            &crate::server::Identity::Anonymous,
            &super::HeaderMap::new(),
            "npmjs",
            &name,
            "https://registry.example",
            super::view(),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), upstream.fetched.recv())
        .await
        .unwrap()
        .unwrap();
    let early_response =
        tokio::time::timeout(std::time::Duration::from_secs(1), &mut request).await;
    assert!(early_response.is_err(), "fetched document rendering bypassed stored admission");
    drop(stored);
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    drop(response);
    stop_upstream(upstream).await;
}
