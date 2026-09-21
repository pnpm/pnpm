use super::{
    ArchiveStoreProjection, AuthHeaders, FASTIFY_ERROR_INTEGRITY, FASTIFY_ERROR_TARBALL,
    FetchTarballForResolution, IngestTarballToStore, MemCache, SharedVerifiedFilesCache,
    SilentReporter, ThrottledClient, fast_retry_opts, integrity, tempdir_with_leaked_path,
    test_retry_opts,
};
use crate::package_mem_cache_key;
use std::time::Duration;

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
    while !mem_cache.contains_key(&cache_key) {
        tokio::task::yield_now().await;
    }

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
