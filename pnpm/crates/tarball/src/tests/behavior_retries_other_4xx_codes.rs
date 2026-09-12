use super::{
    Arc, ArchiveStoreProjection, AuthHeaders, Duration, EndlessReader, FASTIFY_ERROR_INTEGRITY,
    FASTIFY_ERROR_TARBALL, HashMap, IngestTarballToStore, Integrity, MemCache, PrefetchedCasPaths,
    RetryOpts, STREAM_ENTRY_BUFFER_MAX, SharedVerifiedFilesCache, SilentReporter, StoreIndexWriter,
    TarballError, ThrottledClient, assert_eq, fast_retry_opts, fetch_and_extract_with_retry,
    integrity, store_index_key, tempdir_with_leaked_path, test_retry_opts, write_zip_entry_to_cas,
};

#[tokio::test]
async fn retries_other_4xx_codes() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(410)
        .expect(3) // retries: 2 + initial attempt = 3 total
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("non-401/403/404 4xx should exhaust the retry budget");
    match err {
        TarballError::HttpStatus(http) => assert_eq!(http.status, 410),
        other => panic!("expected HttpStatus(410), got: {other:?}"),
    }
    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn retry_exhaustion_returns_last_error() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/pkg.tgz").with_status(500).expect(3).create_async().await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("permanent 500s should exhaust the retry budget");
    match err {
        TarballError::HttpStatus(http) => assert_eq!(http.status, 500),
        other => panic!("expected HttpStatus(500), got: {other:?}"),
    }
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// Regression test for a `run_with_mem_cache` deadlock that hung
/// `pacquet install` on real-network workloads at high concurrency.
/// The if-let branch must not hold a `DashMap::Ref` (a synchronous
/// shard read guard) across an `.await` point: if it does, under
/// enough concurrency another task on the same worker calls
/// `mem_cache.insert` for a key hashing to the same shard, blocks
/// on the `parking_lot` write, and starves every worker.
///
/// To reproduce end-to-end:
/// * Mockito serves the real fastify-error tarball with a
///   per-request sleep so the `InProgress` window is wide enough to
///   schedule the contending task.
/// * Two concurrent calls for the same URL: one wins the else
///   branch, the other parks in the if-let branch.
/// * A third call for a different URL whose key hashes to the same
///   `DashMap` shard. Its else branch calls `mem_cache.insert`, which
///   needs a write guard on the same shard.
/// * Single-worker tokio runtime: with the bug, the only worker
///   blocks on `parking_lot`'s exclusive wait and nothing else can be
///   polled. The runtime is parked in a side OS thread so the test
///   asserts the deadlock as a wall-clock timeout instead of
///   hanging the test process forever.
#[test]
fn run_with_mem_cache_does_not_deadlock_on_dashmap_shard_contention() {
    use std::sync::mpsc;
    use std::thread;

    const RESPONSE_LATENCY: Duration = Duration::from_millis(300);
    const TEST_TIMEOUT: Duration = Duration::from_secs(30);

    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("tarball-deadlock-regression".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("build single-worker runtime");

            rt.block_on(async {
                let mut server = mockito::Server::new_async().await;
                let url1 = format!("{}/pkg.tgz", server.url());

                // `DashMap::default()` uses `RandomState`, whose seed is
                // per-instance — so we MUST probe the very cache the
                // runtime tasks will use. A separate "probe" map would
                // hash to different shards and silently defeat the
                // collision setup, hiding the regression.
                let mem_cache: &'static MemCache = Box::leak(Box::new(MemCache::default()));
                let target_shard = mem_cache.determine_map(&url1);
                let url2 = (0u32..10_000)
                    .map(|i| format!("{}/pkg-{i}.tgz", server.url()))
                    .find(|url| url != &url1 && mem_cache.determine_map(url) == target_shard)
                    .expect("no colliding URL within 10000 candidates");

                let path1 = url1.trim_start_matches(server.url().as_str()).to_string();
                let path2 = url2.trim_start_matches(server.url().as_str()).to_string();
                // Both endpoints are expected to be hit exactly once: A
                // for url1, C for url2. B uses the in-memory cache and
                // never reaches the network. Asserting hit counts guards
                // against a future short-circuit (e.g. a store-index
                // cache hit) that would let `run_with_mem_cache` return
                // before the contention window we want to exercise.
                let slow1 = server
                    .mock("GET", path1.as_str())
                    .with_status(200)
                    .expect(1)
                    .with_chunked_body(|writer| {
                        std::thread::sleep(RESPONSE_LATENCY);
                        writer.write_all(FASTIFY_ERROR_TARBALL)
                    })
                    .create_async()
                    .await;
                let slow2 = server
                    .mock("GET", path2.as_str())
                    .with_status(200)
                    .expect(1)
                    .with_chunked_body(|writer| {
                        std::thread::sleep(RESPONSE_LATENCY);
                        writer.write_all(FASTIFY_ERROR_TARBALL)
                    })
                    .create_async()
                    .await;

                // Leak everything spawned tasks need to borrow. The test
                // is single-shot so we don't bother reclaiming.
                let (_store_keep, store_path) = tempdir_with_leaked_path();
                let client: &'static ThrottledClient =
                    Box::leak(Box::new(ThrottledClient::default()));
                let pkg_integrity: &'static Integrity =
                    Box::leak(Box::new(integrity(FASTIFY_ERROR_INTEGRITY)));
                let url1: &'static str = Box::leak(url1.into_boxed_str());
                let url2: &'static str = Box::leak(url2.into_boxed_str());

                let auth_headers: &'static AuthHeaders =
                    Box::leak(Box::new(AuthHeaders::default()));
                let make_dts = |url: &'static str| IngestTarballToStore {
                    http_client: client,
                    store_dir: store_path,
                    store_index: None,
                    store_index_writer: None,
                    verify_store_integrity: true,
                    strict_store_pkg_content_check: true,
                    package_integrity: Some(pkg_integrity),
                    package_unpacked_size: None,
                    package_file_count: None,
                    package_url: url,
                    package_id: "fastify-error@3.3.0",
                    requester: "",
                    prefetched_cas_paths: None,
                    verified_files_cache: SharedVerifiedFilesCache::default(),
                    retry_opts: RetryOpts { retries: 0, ..RetryOpts::default() },
                    auth_headers,
                    ignore_file_pattern: None,
                    offline: false,
                    progress_reported: None,
                    store_projection: ArchiveStoreProjection::Package { append_manifest: None },
                };

                // Spawn each task and yield once before the next so the
                // single worker drains the just-spawned task to its first
                // suspension point. With one worker, `yield_now` is a
                // deterministic ordering primitive (FIFO local queue):
                // A reaches `run_without_mem_cache`'s HTTP await, B
                // reaches the if-let branch's `notified().await` (with
                // the bug, holding the DashMap shard guard), and only
                // then is C polled — its else branch's
                // `mem_cache.insert` is what blocks the worker pre-fix.
                let task_a =
                    tokio::spawn(make_dts(url1).run_with_mem_cache::<SilentReporter>(mem_cache));
                tokio::task::yield_now().await;
                let task_b =
                    tokio::spawn(make_dts(url1).run_with_mem_cache::<SilentReporter>(mem_cache));
                tokio::task::yield_now().await;
                let task_c =
                    tokio::spawn(make_dts(url2).run_with_mem_cache::<SilentReporter>(mem_cache));

                task_a.await.expect("task A panicked").expect("task A failed");
                task_b.await.expect("task B panicked").expect("task B failed");
                task_c.await.expect("task C panicked").expect("task C failed");

                // Confirm each tarball endpoint was actually hit; without
                // these the test would pass vacuously if `run_with_mem_cache`
                // ever short-circuits before the network call.
                slow1.assert_async().await;
                slow2.assert_async().await;
            });

            // Reaching here means the runtime drained all three tasks —
            // i.e. no deadlock.
            let _ = tx.send(());
        })
        .expect("spawn regression-test thread");

    rx.recv_timeout(TEST_TIMEOUT).expect(
        "run_with_mem_cache deadlocked on DashMap shard contention; \
         single-worker runtime did not finish within the timeout",
    );
}

/// `retries: 0` (the value the existing fall-through tests use)
/// must produce exactly one network attempt — no extra request,
/// no backoff sleep. Guards against a future refactor that
/// off-by-ones the loop and turns `retries: 0` into "1 retry".
#[tokio::test]
async fn zero_retries_makes_a_single_attempt() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/pkg.tgz").with_status(500).expect(1).create_async().await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let opts = RetryOpts { retries: 0, ..fast_retry_opts() };

    fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        opts,
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("retries=0 must surface the first failure");
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// `run_with_mem_cache` must not deadlock when the *owning* fetch
/// errors. The owner must set the slot to `CacheValue::Failed`,
/// remove the entry from `mem_cache`, and notify waiters — otherwise
/// a second requester parks on `Notify::notified` forever. Both
/// requesters surface a `TarballError`.
///
/// Two concurrent `run_with_mem_cache` calls for the same URL,
/// pointing at a 404 endpoint with `retries: 0` so the failure is
/// fast. With a 30 s wall-clock cap, the test asserts the deadlock
/// regression by demanding both calls complete (rather than hanging
/// the whole runtime).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_with_mem_cache_recovers_from_owning_fetch_error() {
    use pnpm_reporter::SilentReporter;

    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/pkg.tgz")
        // 404 makes `is_transient_error` return false, so the retry
        // loop fails fast — perfect for forcing the owner-error
        // branch deterministically.
        .with_status(404)
        // Both concurrent requesters dedup on the URL, so only one
        // network call should land. `expect_at_least(1)` covers
        // either: `mem_cache` dedup (1 hit) or a no-op race (still
        // 1 hit since the 404 is fast).
        .expect_at_least(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    // Leak the inputs so concurrent tasks can each construct a
    // borrow-style `IngestTarballToStore` without lifetime
    // gymnastics on the spawned futures. The test scope is short and
    // the leak is negligible.
    let client: &'static ThrottledClient = Box::leak(Box::new(ThrottledClient::default()));
    let pkg_integrity: &'static Integrity = Box::leak(Box::new(integrity(FASTIFY_ERROR_INTEGRITY)));
    let url: &'static str = Box::leak(url.into_boxed_str());
    let mem_cache: &'static MemCache = Box::leak(Box::new(MemCache::default()));
    let auth_headers: &'static AuthHeaders = Box::leak(Box::<AuthHeaders>::default());

    let make_dts = || IngestTarballToStore {
        http_client: client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: url,
        package_id: "deadlock@1.0.0",
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers,
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    };

    // Drive both calls concurrently. One hits the `else` branch and
    // goes through the network; the other waits on `Notify`. The
    // owner notifies after setting `Failed`, so the waiter wakes up,
    // observes `Failed`, and surfaces `SiblingFetchFailed` (or its
    // own attempt's error).
    let task_a =
        tokio::spawn(
            async move { make_dts().run_with_mem_cache::<SilentReporter>(mem_cache).await },
        );
    let task_b =
        tokio::spawn(
            async move { make_dts().run_with_mem_cache::<SilentReporter>(mem_cache).await },
        );

    // 30s is a paranoid cap; the actual runtime should be a few
    // hundred ms (one mockito 404 + the retry-loop's no-retry
    // path). If `notify_waiters` regresses, this would otherwise
    // hang until nextest's per-test timeout.
    let join = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        futures_util::future::join(task_a, task_b),
    )
    .await
    .expect("run_with_mem_cache deadlocked on owner-error path");

    let (a_result, b_result) = join;
    let result_a = a_result.expect("task_a join");
    let result_b = b_result.expect("task_b join");

    // Both must surface an error — exact variant depends on which
    // task drove the network fetch (gets HttpStatus 404) and which
    // parked on Notify (gets SiblingFetchFailed). Pin only the
    // "both errored, neither hung" invariant.
    assert!(result_a.is_err(), "task_a must surface the 404 (or sibling failure)");
    assert!(result_b.is_err(), "task_b must surface the 404 (or sibling failure)");

    drop(store_dir_keep);
}

/// `pnpm:fetching-progress started` must fire *before* `send().await`,
/// not after. Connection-level failures (DNS / connect / timeout)
/// surface from `send().await` — emitting `started` after that point
/// would silently skip those attempts even though the retry loop
/// still iterates over them. Drives the failure path with an
/// unreachable URL and asserts `started` fired anyway.
#[tokio::test]
async fn started_fires_for_connection_level_failures() {
    use std::sync::Mutex;

    use pnpm_reporter::{FetchingProgressLog, FetchingProgressMessage, LogEvent};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    // Reserved-for-documentation TLD per RFC 6761; resolves nowhere
    // and reqwest's connect step bails before any response. The
    // tarball pipeline surfaces this as `TarballError::FetchTarball`
    // — a transient error that the retry loop *would* keep retrying
    // if we let it, so cap with `retries: 0` for determinism.
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);

    EVENTS.lock().unwrap().clear();
    let _ = fetch_and_extract_with_retry::<RecordingReporter>(
        &client,
        "http://127.0.0.1:1/pkg.tgz", // port 1 is reserved → connect-refused
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "/proj",
        store_path,
        RetryOpts { retries: 0, ..fast_retry_opts() },
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("connect-refused must surface as a TarballError");

    let captured = EVENTS.lock().unwrap();
    let started: Vec<Option<u64>> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::FetchingProgress(FetchingProgressLog {
                message: FetchingProgressMessage::Started { size, .. },
                ..
            }) => Some(*size),
            _ => None,
        })
        .collect();
    assert_eq!(
        started.len(),
        1,
        "started must fire for the attempt even when send() fails before headers; got {captured:?}",
    );
    // No response head ever arrived, so `size` is the truthful
    // "we don't know" — JSON `null` per pnpm's `size: number | null`.
    // Pinning this here so a future refactor that synthesizes a
    // bogus `size` for the error path can't sneak past review.
    assert_eq!(
        started[0], None,
        "size must be None when send() fails before headers; got {:?}",
        started[0],
    );

    drop(store_dir_keep);
}

/// `pnpm:progress found_in_store` fires from the cache-hit early
/// returns in `run_without_mem_cache` — both the prefetched-cas
/// branch and the `load_cached_cas_paths` fallback. Use the latter
/// (writing a v11 store row + the underlying CAFS files, then a
/// fresh-call `run_without_mem_cache`) so the test exercises the
/// same path a warm install would.
#[tokio::test]
async fn found_in_store_event_fires_on_cache_hit() {
    use std::sync::Mutex;

    use pnpm_reporter::{LogEvent, ProgressMessage};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    // First-pass install populates the v11 store + index. Use a
    // mockito server that serves the real fastify-error tarball; the
    // store_dir is the integration boundary.
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1) // exactly one network hit — second call must reuse the cache
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);

    let (writer, writer_task) = StoreIndexWriter::spawn(store_path);
    let verified_files_cache = SharedVerifiedFilesCache::default();

    IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: Some(Arc::clone(&writer)),
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: "@fastify/error@3.3.0",
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect("first download should populate the store");

    // Drain the writer so the index row is durably persisted before
    // the second call attempts to read it back.
    drop(writer);
    writer_task.await.expect("writer task").expect("writer flushed");

    // Second pass — same (integrity, package_id) pair. Recording
    // reporter sees the `found_in_store` emit; the mockito mock must
    // not be hit again (`expect(1)` above).
    let store_index = tokio::task::spawn_blocking(move || {
        pnpm_store_dir::StoreIndex::shared_readonly_in(store_path)
    })
    .await
    .expect("spawn_blocking")
    .expect("index opens after the first install");

    EVENTS.lock().unwrap().clear();
    IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: Some(store_index),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: "@fastify/error@3.3.0",
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<RecordingReporter>()
    .await
    .expect("second call should hit the store cache");

    let captured = EVENTS.lock().unwrap();
    assert!(
        captured.iter().any(|e| matches!(
            e,
            LogEvent::Progress(log)
                if matches!(
                    &log.message,
                    ProgressMessage::FoundInStore { package_id, requester }
                        if package_id == "@fastify/error@3.3.0" && requester == "/proj",
                )
        )),
        "found_in_store must fire on cache hit; got {captured:?}",
    );
    assert!(
        !captured.iter().any(|e| matches!(
            e,
            LogEvent::Progress(log) if matches!(&log.message, ProgressMessage::Fetched { .. })
        )),
        "fetched must NOT fire on cache hit; got {captured:?}",
    );

    drop(store_dir_keep);
}

/// `pnpm:request-retry` fires before each backoff sleep — once per
/// failed-and-being-retried attempt — and never on the final
/// successful or final failed attempt. With one transient 503
/// followed by a 200, the retry loop emits exactly one event:
/// `attempt: 1` (one-indexed, matching pnpm's wire shape) carrying
/// the response status as `httpStatusCode`.
#[tokio::test]
async fn request_retry_event_fires_per_retried_attempt() {
    use std::sync::Mutex;

    use pnpm_reporter::{LogEvent, RequestRetryLog};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let fail = server.mock("GET", "/pkg.tgz").with_status(503).expect(1).create_async().await;
    let ok = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);

    EVENTS.lock().unwrap().clear();

    fetch_and_extract_with_retry::<RecordingReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect("transient 503 should be followed by a successful retry");

    fail.assert_async().await;
    ok.assert_async().await;

    let captured = EVENTS.lock().unwrap();
    let retries: Vec<&RequestRetryLog> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::RequestRetry(log) => Some(log),
            _ => None,
        })
        .collect();
    assert_eq!(retries.len(), 1, "exactly one retry emit expected; got {captured:?}");

    let retry = retries[0];
    // attempt is one-indexed (the failed attempt). With one transient
    // 503 and the retry succeeding, the only retry-emit is for
    // attempt 1.
    assert_eq!(retry.attempt, 1, "attempt must be one-indexed");
    assert_eq!(retry.max_retries, fast_retry_opts().retries);
    assert_eq!(retry.method, "GET");
    assert_eq!(retry.url, url);
    // `fast_retry_opts` collapses the backoff to 1 ms, so `timeout`
    // must reflect the actual retry-loop sleep (not pnpm's
    // production 10 s default) — guard against an off-by-one that
    // emits the wrong attempt's delay.
    assert_eq!(retry.timeout, 1, "timeout must mirror RetryOpts::delay_for");
    // The 503 surfaces as `TarballError::HttpStatus`, so the
    // wire-shape carries `httpStatusCode: "503"` and the JS
    // reporter's `??` chain dispatches on it before falling
    // through to the placeholder `code`.
    assert_eq!(retry.error.http_status_code.as_deref(), Some("503"));
    assert!(
        retry.error.code.is_none(),
        "HTTP failures must skip the placeholder code so the JS reporter dispatches on httpStatusCode",
    );

    drop(store_dir_keep);
}

/// A zip entry's decompressed size is a claim in the central directory,
/// not a limit the deflate stream behind it respects. An entry that
/// keeps producing bytes past what it declared is the zip bomb: it must
/// be rejected, and the read must stop rather than following the stream
/// to wherever it ends — on the buffered and the direct-to-store branch
/// alike.
#[test]
fn write_zip_entry_to_cas_stops_reading_an_entry_longer_than_it_claims() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    for declared_size in [16, STREAM_ENTRY_BUFFER_MAX + 1] {
        let mut liar = EndlessReader { bytes_read: 0, cap: declared_size * 8 };
        let err = write_zip_entry_to_cas(
            &mut liar,
            declared_size,
            "https://example.test/bomb.zip",
            "big.bin",
            store_path,
            false,
        )
        .expect_err("an entry that outruns its declared size must be rejected");
        assert!(
            matches!(err, TarballError::ReadZipEntries { .. }),
            "expected ReadZipEntries for declared_size {declared_size}, got {err:?}",
        );
        assert!(
            liar.bytes_read <= declared_size + 1,
            "the read must stop just past the declared {declared_size} bytes, took {}",
            liar.bytes_read,
        );
    }

    drop(tempdir);
}

/// `offline: true` short-circuits the fetcher before any network
/// request when the package isn't in the local store. Mocks a server
/// with `.expect(0)` so the assertion fires *only* if the fetcher
/// ever calls the mocked URL; the offline gate must keep it from
/// ever reaching `fetch_and_extract_with_retry`.
#[tokio::test]
async fn offline_mode_skips_network_on_cache_miss() {
    use pnpm_diagnostics::miette::Diagnostic;

    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    // `.expect(0)` — if the fetcher attempts the network at all,
    // mockito's drop checker fails the test on the `.assert_async`
    // call below.
    let must_not_fire =
        server.mock("GET", "/pkg.tgz").with_status(200).expect(0).create_async().await;

    let url = format!("{}/pkg.tgz", server.url());
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let pkg_id = "@fastify/error@3.3.0";

    let err = IngestTarballToStore {
        http_client: &ThrottledClient::default(),
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: pkg_id,
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("offline + cache miss must error before reaching the network");

    // Variant shape + diagnostic code together. The `code` check
    // pins the user-facing surface — `ERR_PNPM_NO_OFFLINE_TARBALL`
    // is part of the CLI contract, like pnpm's
    // `ERR_PNPM_NO_OFFLINE_META`.
    let TarballError::NoOfflineTarball { package_id, url: errored_url } = &err else {
        panic!("expected NoOfflineTarball, got {err:?}");
    };
    assert_eq!(package_id, pkg_id);
    assert_eq!(errored_url, &url);
    let code = err.code().map(|c| c.to_string()).unwrap_or_default();
    assert_eq!(
        code, "ERR_PNPM_NO_OFFLINE_TARBALL",
        "diagnostic code is part of the user-facing surface; must stay stable",
    );

    // No network call was made — confirms the gate fired before any
    // attempt at `fetch_and_extract_with_retry`.
    must_not_fire.assert_async().await;

    drop(store_dir_keep);
}

/// `offline: true` is *not* consulted when the local store already
/// has the file: the prefetched-CAS-paths branch should still
/// short-circuit happily, regardless of the offline flag. Without
/// this guard, a regression that bumped the offline check above the
/// prefetch lookup would break warm installs under `--offline`.
#[tokio::test]
async fn offline_mode_still_uses_prefetched_cache() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    // Server with `.expect(0)` — the prefetched-CAS-paths branch must
    // short-circuit before any HTTP call.
    let mut server = mockito::Server::new_async().await;
    let must_not_fire =
        server.mock("GET", "/pkg.tgz").with_status(200).expect(0).create_async().await;

    let url = format!("{}/pkg.tgz", server.url());
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let pkg_id = "@fastify/error@3.3.0";

    // Seed the prefetched cache with a placeholder entry for our
    // (integrity, pkg_id) — value content doesn't matter; the gate
    // we're exercising only checks key presence. `PrefetchedCasPaths`
    // is a `HashMap` type alias, so a struct literal works directly.
    let cache_key = store_index_key(&pkg_integrity.to_string(), pkg_id);
    let mut prefetched: PrefetchedCasPaths = HashMap::new();
    prefetched.insert(cache_key, Arc::new(HashMap::new()));

    let cas_paths = IngestTarballToStore {
        http_client: &ThrottledClient::default(),
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: pkg_id,
        requester: "",
        prefetched_cas_paths: Some(&prefetched),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect("warm install under --offline must succeed when the package is prefetched");

    // Prefetched seed used a placeholder empty map; the return must
    // surface that empty map (the offline gate didn't fire, the
    // prefetch lookup did).
    assert!(cas_paths.is_empty(), "got the prefetched-empty map back: {cas_paths:?}");
    must_not_fire.assert_async().await;

    drop(store_dir_keep);
}
