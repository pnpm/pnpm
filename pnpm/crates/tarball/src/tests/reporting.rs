use super::{
    ArchiveStoreProjection, AuthHeaders, FASTIFY_ERROR_INTEGRITY, FASTIFY_ERROR_TARBALL,
    IngestTarballToStore, MemCache, SharedReportedProgressKeys, SharedVerifiedFilesCache,
    StoreIndex, ThrottledClient, assert_eq, fast_fail_client, integrity,
    seed_row_holding_another_package, store_index_key, tempdir_with_leaked_path, test_retry_opts,
};

#[cfg(not(target_os = "windows"))]
use super::SilentReporter;

/// A successful network download records its
/// `store_index_key(integrity, pkg_id)` in the supplied
/// [`SharedReportedProgressKeys`] set, so a later install pass can skip
/// a duplicate package-status event for the same key. Regression guard
/// for <https://github.com/pnpm/pnpm/issues/12235>.
#[tokio::test]
#[cfg(not(target_os = "windows"))]
async fn network_fetch_records_progress_key() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    let pkg_integrity = integrity(
        "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w==",
    );
    let pkg_id = "@fastify/error@3.3.0";
    let progress_reported = SharedReportedProgressKeys::default();

    IngestTarballToStore {
        http_client: &ThrottledClient::default(),
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: Some(16697),
        package_file_count: None,
        package_url: "https://registry.npmjs.org/@fastify/error/-/error-3.3.0.tgz",
        package_id: pkg_id,
        requester: "",
        prefetched_cas_paths: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: Some(SharedReportedProgressKeys::clone(&progress_reported)),
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .unwrap();

    let expected_key = store_index_key(&pkg_integrity.to_string(), pkg_id);
    assert!(
        progress_reported.contains(&expected_key),
        "network download must record its progress key; got {progress_reported:?}",
    );

    drop(store_dir);
}

/// `strictStorePkgContentCheck: false` downgrades the same
/// disagreement to a warning and installs from the row anyway.
#[tokio::test]
async fn store_row_holding_another_package_only_warns_when_not_strict() {
    use std::sync::Mutex;

    use pnpm_reporter::LogEvent;

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);
    seed_row_holding_another_package(store_path, &index_key);

    EVENTS.lock().unwrap().clear();
    let cas_paths = IngestTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        // The row's blob was never written to disk, so the reuse this
        // asserts is only reachable with verification off.
        verify_store_integrity: false,
        strict_store_pkg_content_check: false,
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: "http://127.0.0.1:1/unreachable.tgz",
        package_id: pkg_id,
        requester: "",
        prefetched_cas_paths: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<RecordingReporter>()
    .await
    .expect("without the strict check the row is still used");
    assert!(cas_paths.contains_key("package.json"));

    let warnings: Vec<String> = EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            LogEvent::Global(log) => Some(log.message.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0]
            .starts_with("Package name or version mismatch found while reading from the store."),
        "{warnings:?}",
    );

    drop(store_dir);
}

/// Without a shared progress-dedupe set, `run_with_mem_cache`'s
/// `Available` short-circuit emits `pnpm:progress found_in_store`
/// against the caller's reporter, regardless of who originally
/// populated the slot. This preserves the legacy install path where a
/// later caller still needs its own visible cache-hit event.
///
/// Drives two `run_with_mem_cache` calls for the same URL but
/// different `package_id`s. The first uses `SilentReporter`
/// (modelling the prefetcher). The second uses the recording
/// reporter (modelling the install pass) and hits the
/// immediate-`Available` branch — the only event captured must be
/// a single `found_in_store` for the install pass's `package_id`.
#[tokio::test]
async fn mem_cache_hit_emits_found_in_store_against_callers_reporter() {
    use std::sync::Mutex;

    use pnpm_reporter::{LogEvent, ProgressMessage};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        // exactly one network hit — the second requester must reuse
        // the in-memory cache without going to the network.
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let mem_cache = MemCache::default();
    let verified_files_cache = SharedVerifiedFilesCache::default();

    // First requester: silent legacy owner.
    IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: "first@1.0.0",
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_with_mem_cache::<pnpm_reporter::SilentReporter>(&mem_cache)
    .await
    .expect("first call should populate the mem cache");

    // Second requester: same URL, different `package_id`. Hits the
    // immediate-`Available` branch and emits one `found_in_store`
    // because no shared progress set says this package status was
    // already reported.
    EVENTS.lock().unwrap().clear();
    IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: "second@2.0.0",
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_with_mem_cache::<RecordingReporter>(&mem_cache)
    .await
    .expect("second call should reuse the mem cache");

    let captured = EVENTS.lock().unwrap();
    let found_in_store_events: Vec<_> = captured
        .iter()
        .filter(|e| {
            matches!(
                e,
                LogEvent::Progress(log)
                    if matches!(&log.message, ProgressMessage::FoundInStore { .. }),
            )
        })
        .collect();
    assert_eq!(
        found_in_store_events.len(),
        1,
        "exactly one found_in_store emit expected on Available short-circuit; got {captured:?}",
    );
    if let LogEvent::Progress(log) = found_in_store_events[0]
        && let ProgressMessage::FoundInStore { package_id, .. } = &log.message
    {
        assert_eq!(package_id, "second@2.0.0");
    } else {
        unreachable!("captured event filtered above");
    }
    assert!(
        !captured.iter().any(|e| matches!(
            e,
            LogEvent::Progress(log) if matches!(&log.message, ProgressMessage::Fetched { .. })
        )),
        "fetched must NOT fire on a mem-cache hit; got {captured:?}",
    );

    drop(store_dir_keep);
}

/// With a shared progress-dedupe set, the first owner reports the
/// package status and records the cache key. A later caller that hits
/// the in-memory cache for the same package key must not emit a second
/// `fetched` or `found_in_store`.
#[tokio::test]
async fn mem_cache_hit_skips_package_status_when_progress_already_reported() {
    use std::sync::Mutex;

    use pnpm_reporter::{LogEvent, ProgressMessage};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let mem_cache = MemCache::default();
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let progress_reported = SharedReportedProgressKeys::default();
    let pkg_id = "@fastify/error@3.3.0";

    EVENTS.lock().unwrap().clear();
    IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: pkg_id,
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: Some(SharedReportedProgressKeys::clone(&progress_reported)),
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_with_mem_cache::<RecordingReporter>(&mem_cache)
    .await
    .expect("first call should fetch and report");

    // Clone the events out rather than binding the `MutexGuard`: a
    // named guard lexically spans the second download's `.await` below
    // (clippy's `await_holding_lock` is scope-based and ignores an
    // explicit `drop`), even though the data is only read here.
    let first = EVENTS.lock().unwrap().clone();
    assert!(
        first.iter().any(|e| matches!(
            e,
            LogEvent::Progress(log) if matches!(&log.message, ProgressMessage::Fetched { .. })
        )),
        "first call must report fetched; got {first:?}",
    );

    EVENTS.lock().unwrap().clear();
    IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: pkg_id,
        requester: "/proj",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: Some(SharedReportedProgressKeys::clone(&progress_reported)),
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_with_mem_cache::<RecordingReporter>(&mem_cache)
    .await
    .expect("second call should reuse the mem cache");

    let second = EVENTS.lock().unwrap().clone();
    assert!(
        !second.iter().any(|e| matches!(
            e,
            LogEvent::Progress(log)
                if matches!(
                    &log.message,
                    ProgressMessage::Fetched { .. } | ProgressMessage::FoundInStore { .. }
                )
        )),
        "second call must not duplicate package status; got {second:?}",
    );

    drop(store_dir_keep);
}
