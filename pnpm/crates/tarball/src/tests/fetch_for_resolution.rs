use super::{
    ArchiveStoreProjection,
    AuthHeaders,
    FASTIFY_ERROR_INTEGRITY,
    FASTIFY_ERROR_TARBALL,
    FetchTarballForResolution,
    IngestTarballToStore,
    MAX_UNTRUSTED_PREALLOC_BYTES,
    MemCache,
    SharedVerifiedFilesCache,
    SilentReporter,
    TarballError,
    ThrottledClient,
    fast_retry_opts,
    integrity,
    tempdir_with_leaked_path,
    test_retry_opts,
};
use crate::{
    CacheValue,
    CachedTarball,
    package_mem_cache_key,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;

async fn wait_until_claimed(mem_cache: &MemCache, cache_key: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !mem_cache.contains_key(cache_key) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("prefetch did not claim {cache_key}"));
}

fn ingest<'a>(
    client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
    url: &'a str,
    integrity: &'a ssri::Integrity,
    store_path: &'static pnpm_store_dir::StoreDir,
) -> IngestTarballToStore<'a> {
    IngestTarballToStore {
        fetching: crate::ArchiveFetchOptions {
            http_client: client,
            auth_headers,
            retry_opts: test_retry_opts(),
            offline: false,
        },
        package: crate::TarballPackage {
            integrity: Some(integrity),
            unpacked_size: None,
            file_count: None,
            url,
            id: "@fastify/error@3.3.0",
        },
        store: crate::ArchiveStoreContext {
            dir: store_path,
            index: None,
            index_writer: None,
            verify_integrity: true,
            strict_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            prefetched_cas_paths: None,
        },
        requester: "/proj",
        ignore_file_pattern: None,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
}

fn resolution_read<'a>(
    client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
    url: &'a str,
    integrity: &'a ssri::Integrity,
    store_path: &'static pnpm_store_dir::StoreDir,
) -> FetchTarballForResolution<'a> {
    FetchTarballForResolution {
        http_client: client,
        store_dir: store_path,
        store_index_writer: None,
        package: crate::TarballPackage {
            integrity: Some(integrity),
            unpacked_size: None,
            file_count: None,
            url,
            id: "@fastify/error@3.3.0",
        },
        auth_headers,
        retry_opts: fast_retry_opts(),
        manifest_subdir: None,
        revision_addressed: false,
    }
}

/// <https://github.com/pnpm/pnpm/issues/15037>
#[tokio::test]
async fn pinned_resolution_read_reuses_a_settled_prefetch() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let mem_cache = MemCache::default();

    ingest(&client, &auth_headers, &url, &pkg_integrity, store_path)
        .run_with_mem_cache::<SilentReporter>(&mem_cache)
        .await
        .expect("prefetch extracts the archive");

    let resolved = resolution_read(&client, &auth_headers, &url, &pkg_integrity, store_path)
        .run::<SilentReporter>(Some(&mem_cache))
        .await
        .expect("the read takes the bundled manifest from the settled slot");

    let manifest = resolved.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("@fastify/error"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// <https://github.com/pnpm/pnpm/issues/15037>
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinned_resolution_read_parks_on_an_in_flight_prefetch() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .expect(1)
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(300));
            writer.write_all(FASTIFY_ERROR_TARBALL)
        })
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());
    let client: &'static ThrottledClient = Box::leak(Box::new(ThrottledClient::default()));
    let auth_headers: &'static AuthHeaders = Box::leak(Box::new(AuthHeaders::default()));
    let pkg_integrity: &'static ssri::Integrity =
        Box::leak(Box::new(integrity(FASTIFY_ERROR_INTEGRITY)));
    let url: &'static str = Box::leak(url.into_boxed_str());
    let mem_cache: &'static MemCache = Box::leak(Box::new(MemCache::default()));
    let cache_key = package_mem_cache_key(url, Some(pkg_integrity), false);

    let ingest_task = tokio::spawn(async move {
        ingest(client, auth_headers, url, pkg_integrity, store_path)
            .run_with_mem_cache::<SilentReporter>(mem_cache)
            .await
    });
    wait_until_claimed(mem_cache, &cache_key).await;

    let resolved = resolution_read(client, auth_headers, url, pkg_integrity, store_path)
        .run::<SilentReporter>(Some(mem_cache))
        .await
        .expect("the read parks on the in-flight extraction");
    ingest_task.await.expect("join ingest").expect("prefetch finishes");

    let manifest = resolved.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("@fastify/error"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// <https://github.com/pnpm/pnpm/issues/15037>
#[tokio::test]
async fn pinned_resolution_read_recovers_manifest_from_a_files_only_slot() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let mem_cache = MemCache::default();

    let cas_file = store_dir_keep.path().join("package.json");
    std::fs::write(
        &cas_file,
        r#"{"name":"@fastify/error","version":"3.3.0","dependencies":{"foo":"1.0.0"},"description":"dropped"}"#,
    )
    .expect("write cas package.json");
    let mut files = HashMap::new();
    files.insert("package.json".to_string(), cas_file);
    mem_cache.insert(
        package_mem_cache_key(&url, Some(&pkg_integrity), false),
        Arc::new(RwLock::new(CacheValue::Available(CachedTarball::from_files(files)))),
    );

    let resolved = resolution_read(&client, &auth_headers, &url, &pkg_integrity, store_path)
        .run::<SilentReporter>(Some(&mem_cache))
        .await
        .expect("the read takes package.json from the files-only slot");

    let manifest = resolved.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("@fastify/error"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("3.3.0"));
    assert_eq!(manifest["dependencies"]["foo"].as_str(), Some("1.0.0"));
    assert_eq!(manifest.get("description"), None);
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// <https://github.com/pnpm/pnpm/issues/15037>
#[tokio::test]
async fn files_only_slot_rejects_an_oversized_cached_package_json() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let mem_cache = MemCache::default();

    let cas_file = store_dir_keep.path().join("package.json");
    let file = std::fs::File::create(&cas_file).expect("create cas package.json");
    file.set_len(MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1)
        .expect("set oversized length");
    let mut files = HashMap::new();
    files.insert("package.json".to_string(), cas_file);
    mem_cache.insert(
        package_mem_cache_key(&url, Some(&pkg_integrity), false),
        Arc::new(RwLock::new(CacheValue::Available(CachedTarball::from_files(files)))),
    );

    let err = resolution_read(&client, &auth_headers, &url, &pkg_integrity, store_path)
        .run::<SilentReporter>(Some(&mem_cache))
        .await
        .expect_err("an oversized cached package.json is not a silent miss");
    assert!(matches!(err, TarballError::ReadTarballEntries(_)), "got {err:?}");
    mock.assert_async().await;
    drop(store_dir_keep);
}
