use super::{
    FetchTarballForResolution, MAX_UNTRUSTED_PREALLOC_BYTES, MemCache, RetryOpts,
    SharedReportedProgressKeys, auth_header_for_package_download,
    download::{
        DownloadTarballToStore, download_priority, fetch_and_extract_with_retry,
        is_transient_error, slow_download_warning,
    },
    error::{HttpStatusError, NetworkError, TarballError, VerifyChecksumError},
    extract::{
        STREAM_ENTRY_BUFFER_MAX, STREAM_EXTRACT_COMPRESSED_THRESHOLD, allocate_tarball_buffer,
        apply_append_manifest, apply_placeholder_manifest, bounded_gzip_size_hint, decompress_gzip,
        extract_gzipped_tarball, extract_tarball_entries, gzip_isize_hint,
        is_eager_decode_limit_exceeded, normalize_bundled_manifest, should_stream_extract,
        stream_extract_gzipped_tarball,
    },
    local_tarball::{
        allocate_local_tarball_buffer, local_file_tarball_path, open_local_tarball,
        read_local_tarball_buffer, read_local_tarball_metadata,
    },
    prefetch::{PrefetchedCasPaths, prefetch_cas_paths},
    zip_archive::{extract_zip_entries, write_zip_entry_to_cas},
};
use pipe_trait::Pipe;
use pnpm_network::{AuthHeaders, MAX_THROUGHPUT_PRIORITY, ThrottledClient, UNPRIORITIZED};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
    StoreIndexWriter, store_index_key,
};
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::{
    collections::HashMap,
    io::{Cursor, ErrorKind, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tempfile::{TempDir, tempdir};

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
}

#[test]
fn node_runtime_downloads_do_not_send_auth_over_remote_http() {
    let auth_headers = AuthHeaders::from_creds_map([(
        "//mirror.example/".to_string(),
        "Bearer mirror-token".to_string(),
    )]);
    assert_eq!(
        auth_header_for_package_download(
            &auth_headers,
            "http://mirror.example/node.tar.gz",
            "node@runtime:22.0.0",
        ),
        None,
    );
    assert_eq!(
        auth_header_for_package_download(
            &auth_headers,
            "https://mirror.example/node.tar.gz",
            "node@runtime:22.0.0",
        )
        .as_deref(),
        Some("Bearer mirror-token"),
    );
}

#[test]
fn formats_warning_for_slow_tarball_download() {
    assert_eq!(
        slow_download_warning(
            40 * 1024,
            Duration::from_millis(2_001),
            50,
            "https://user:pass@registry.example.test/pkg.tgz?token=secret#fragment\u{1b}",
        ),
        Some(
            "Tarball download average speed 19 KiB/s (size 40 KiB) is below 50 KiB/s: https://registry.example.test/pkg.tgz (GET)"
                .to_string(),
        ),
    );
}

#[test]
fn does_not_warn_for_short_or_fast_tarball_download() {
    assert_eq!(
        slow_download_warning(1, Duration::from_secs(1), 50, "https://example.test/pkg.tgz"),
        None,
    );
    assert_eq!(
        slow_download_warning(
            100 * 1024,
            Duration::from_secs(2),
            50,
            "https://example.test/pkg.tgz",
        ),
        None,
    );
}

#[test]
fn gzip_size_hint_enforces_untrusted_preallocation_limit() {
    assert_eq!(bounded_gzip_size_hint(None), None);
    assert_eq!(bounded_gzip_size_hint(Some(1)), Some(1));
    assert_eq!(
        bounded_gzip_size_hint(Some(MAX_UNTRUSTED_PREALLOC_BYTES)),
        Some(MAX_UNTRUSTED_PREALLOC_BYTES),
    );
    assert_eq!(
        bounded_gzip_size_hint(Some(MAX_UNTRUSTED_PREALLOC_BYTES + 1)),
        Some(MAX_UNTRUSTED_PREALLOC_BYTES),
    );
    assert_eq!(bounded_gzip_size_hint(Some(usize::MAX)), Some(MAX_UNTRUSTED_PREALLOC_BYTES));
}

/// Covers the wiring rather than the bound itself: nothing in
/// [`bounded_gzip_size_hint`]'s own test catches `decompress_gzip`
/// handing the raw hint to zune-inflate, which aborts the process
/// instead of failing.
#[test]
fn decompress_gzip_bounds_oversized_unpacked_size() {
    let payload = b"decompressed tar payload";
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut encoder, payload).expect("gzip payload");
    let gz_data = encoder.finish().expect("finish gzip");

    let decompressed = decompress_gzip(&gz_data, Some(usize::MAX)).expect("decompress gzip");

    assert_eq!(decompressed, payload);
}

/// Absent `Content-Length` (chunked transfer) returns an empty
/// growable buffer. The stream loop extends it as chunks arrive.
#[test]
fn allocate_tarball_buffer_returns_empty_when_content_length_is_absent() {
    let buf = allocate_tarball_buffer(None, "https://example.test/pkg.tgz")
        .expect("no content-length is a valid chunked-transfer response");
    assert_eq!(buf.len(), 0);
}

/// Reasonable `Content-Length` pre-sizes the buffer so no
/// realloc happens during the stream loop. `try_reserve_exact`
/// succeeds; we don't assert `buf.capacity() == size` because
/// allocators are allowed to round up, only that it's at least
/// what we asked for.
#[test]
fn allocate_tarball_buffer_presizes_for_reasonable_content_length() {
    let buf = allocate_tarball_buffer(Some(1024 * 1024), "https://example.test/pkg.tgz")
        .expect("1 MiB pre-allocation should succeed on any dev / CI box");
    assert!(buf.capacity() >= 1024 * 1024, "capacity = {}", buf.capacity());
    assert_eq!(buf.len(), 0);
}

/// A maliciously or buggily huge `Content-Length` must not be
/// passed through to the infallible `Vec::with_capacity` — that
/// would abort the process on allocation failure. `try_reserve_exact`
/// surfaces the failure as `TarballTooLarge` so the install can
/// reject this one package and continue.
#[test]
fn allocate_tarball_buffer_rejects_absurd_content_length() {
    let url = "https://example.test/evil.tgz";
    let err = allocate_tarball_buffer(Some(u64::MAX), url)
        .expect_err("u64::MAX cannot actually be reserved");
    match err {
        TarballError::TarballTooLarge { url: got_url, advertised_size } => {
            assert_eq!(got_url, url);
            assert_eq!(advertised_size, u64::MAX);
        }
        other => panic!("expected TarballTooLarge, got {other:?}"),
    }
}

/// HTTP client for the fall-through tests. A default `ThrottledClient`
/// uses `Client::new()` with no connect / request timeout, so on a
/// firewalled runner the unreachable `http://127.0.0.1:1/...` URL
/// could stall for minutes of TCP retry. One-second bounds are
/// plenty for loopback and keep the failure mode deterministic.
fn fast_fail_client() -> ThrottledClient {
    let build = |redirect| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(1))
            .timeout(std::time::Duration::from_secs(1))
            .redirect(redirect)
            .build()
            .expect("build reqwest client")
    };
    ThrottledClient::from_clients(
        build(reqwest::redirect::Policy::limited(10)),
        build(reqwest::redirect::Policy::none()),
    )
}

/// Pin `walk_reqwest_chain`'s contract: a `NetworkError` formed
/// from a real reqwest connect failure must surface the leaf
/// reason (e.g. `Connection refused`) appended to the wrapper
/// message, not stop at reqwest's `error sending request for url
/// (URL)`. Without the helper, the user sees only the wrapper —
/// which is what triggered the original "what's actually failing?"
/// debugging round on this branch.
///
/// Uses `127.0.0.1:1` and [`fast_fail_client`]'s 1 s bounds. A
/// firewalled runner may time out instead of refusing the connection.
#[tokio::test]
async fn network_error_display_includes_reqwest_inner_chain() {
    let url = "http://127.0.0.1:1/ssl-package.tgz";
    let client = fast_fail_client();
    let err =
        client.acquire().await.get(url).send().await.expect_err("connecting to port 1 must fail");
    let expected_code = if err.is_timeout() { "ETIMEDOUT" } else { "ECONNREFUSED" };
    let net_err = NetworkError { url: url.to_string(), error: err };

    let rendered = net_err.to_string();
    assert!(
        rendered.starts_with("Failed to fetch http://127.0.0.1:1/"),
        "wrapper prefix missing, got: {rendered:?}",
    );

    // Reqwest's wrapper already includes the URL in `(...)`; the
    // leaf reason appears after the wrapper, separated by `: `.
    // Assert there *is* a non-empty frame after that — without
    // `walk_reqwest_chain`, this is exactly what got dropped.
    let leaf_section = rendered
        .split_once("error sending request for url (")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(_, after_paren)| after_paren)
        .expect("rendered output should include reqwest's wrapper");
    assert!(
        !leaf_section.trim().is_empty(),
        "expected leaf cause appended after reqwest wrapper, got: {rendered:?}",
    );
    assert!(
        leaf_section.starts_with(": "),
        "leaf should be joined with `: ` per walk_reqwest_chain, got: {rendered:?}",
    );

    // Structural form for completeness — `#[error(source)]` should
    // expose the reqwest::Error so miette / `Error::source` can
    // walk into it independently of our flattened Display.
    assert!(
        std::error::Error::source(&net_err).is_some(),
        "NetworkError should expose its reqwest::Error as source",
    );
    let details = TarballError::FetchTarball(net_err).fetch_error_details();
    assert_eq!(details.code.as_deref(), Some(expected_code));
    assert_eq!(details.status, None);
}

/// Default `RetryOpts` for unit tests. We don't want the suite to
/// sit through pnpm's 10 s + 60 s production backoff just to assert
/// that an unreachable URL eventually fails — every test that
/// exercises a network call here either short-circuits to a cache
/// hit or expects the failure path. `retries: 0` keeps the failure
/// path deterministic and bounded by [`fast_fail_client`]'s 1 s
/// timeouts; tests that specifically want to *prove* the retry
/// loop runs should construct their own [`RetryOpts`].
fn test_retry_opts() -> RetryOpts {
    RetryOpts { retries: 0, ..RetryOpts::default() }
}

/// **Problem:**
/// The tested function requires `'static` paths, leaking would prevent
/// temporary files from being cleaned up.
///
/// **Solution:**
/// Create [`TempDir`] as a temporary variable (which can be dropped)
/// but provide its path as `'static`.
///
/// **Side effect:**
/// The `'static` path becomes dangling outside the scope of [`TempDir`].
fn tempdir_with_leaked_path() -> (TempDir, &'static StoreDir) {
    let tempdir = tempdir().unwrap();
    let leaked_path =
        tempdir.path().to_path_buf().pipe(StoreDir::from).pipe(Box::new).pipe(Box::leak);
    (tempdir, leaked_path)
}

#[tokio::test]
#[cfg(not(target_os = "windows"))]
async fn packages_under_orgs_should_work() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    let cas_files = DownloadTarballToStore {
        http_client: &ThrottledClient::default(),
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_integrity: Some(&integrity("sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w==")),
        package_unpacked_size: Some(16697),
        package_file_count: None,
        package_url: "https://registry.npmjs.org/@fastify/error/-/error-3.3.0.tgz",
        package_id: "@fastify/error@3.3.0",
        requester: "",
        prefetched_cas_paths: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .unwrap();

    let mut filenames = cas_files.keys().collect::<Vec<_>>();
    filenames.sort();
    assert_eq!(
        filenames,
        vec![
            ".github/dependabot.yml",
            ".github/workflows/ci.yml",
            ".taprc",
            "LICENSE",
            "README.md",
            "benchmarks/create.js",
            "benchmarks/instantiate.js",
            "benchmarks/no-stack.js",
            "benchmarks/toString.js",
            "index.js",
            "package.json",
            "test/index.test.js",
            "types/index.d.ts",
            "types/index.test-d.ts",
        ],
    );

    drop(store_dir);
}

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

    DownloadTarballToStore {
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
        append_manifest: None,
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

#[tokio::test]
async fn should_throw_error_on_checksum_mismatch() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    DownloadTarballToStore {
        http_client: &ThrottledClient::default(),
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_integrity: Some(&integrity("sha512-aaaan1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w==")),
        package_unpacked_size: Some(16697),
        package_file_count: None,
        package_url: "https://registry.npmjs.org/@fastify/error/-/error-3.3.0.tgz",
        package_id: "@fastify/error@3.3.0",
        requester: "",
        prefetched_cas_paths: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("checksum mismatch");

    drop(store_dir);
}

/// When the `SQLite` index already has an entry for this
/// `(integrity, pkg_id)` pair and every referenced CAFS file is on
/// disk, `run_without_mem_cache` must return the cached layout
/// without issuing an HTTP request. We prove the "no network"
/// property by pointing `package_url` at an address that would
/// fail-fast if dialed.
#[tokio::test]
async fn reuses_cached_cas_paths_when_index_entry_is_live() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let (pkg_json_path, pkg_json_hash) =
        store_path.write_cas_file(b"{\"name\":\"fake\"}", false).unwrap();
    let (bin_path, bin_hash) =
        store_path.write_cas_file(b"#!/usr/bin/env node\nconsole.log('hi');\n", true).unwrap();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            digest: format!("{pkg_json_hash:x}"),
            mode: 0o644,
            size: 15,
            checked_at: None,
        },
    );
    files.insert(
        "bin/cli.js".to_string(),
        CafsFileInfo { digest: format!("{bin_hash:x}"), mode: 0o755, size: 39, checked_at: None },
    );

    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };

    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    // A cache hit also emits package-status progress, so it records the
    // key to prevent a later warm/cold pass from counting the same
    // package status again.
    let progress_reported = SharedReportedProgressKeys::default();
    let download = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        // Any request that reaches the network here would fail the
        // test; the cache lookup must short-circuit before we get
        // near it. `fast_fail_client` caps that at 1 s per side in
        // case a firewalled runner drops the packet silently.
        package_url: "http://127.0.0.1:1/unreachable.tgz",
        package_id: pkg_id,
        requester: "",
        prefetched_cas_paths: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: Some(SharedReportedProgressKeys::clone(&progress_reported)),
        append_manifest: None,
    };
    let cas_paths = download
        .run_without_mem_cache::<SilentReporter>()
        .await
        .expect("cache hit should succeed without network");

    assert_eq!(cas_paths.len(), 2);
    assert_eq!(cas_paths.get("package.json"), Some(&pkg_json_path));
    assert_eq!(cas_paths.get("bin/cli.js"), Some(&bin_path));
    assert!(
        progress_reported.contains(&index_key),
        "a store cache hit must record its progress key; got {progress_reported:?}",
    );
    let error = download
        .fetch_and_extract::<SilentReporter>()
        .await
        .expect_err("an explicit fetch must read the requested URL despite the cached digest");
    assert!(matches!(error, TarballError::FetchTarball(_)), "{error}");

    drop(store_dir);
}

/// When `prefetched_cas_paths` already covers the requested
/// `(integrity, pkg_id)`, `run_without_mem_cache` must short-circuit
/// to the prefetched map and never touch the `SQLite` index or the
/// network. `store_index: None` proves it doesn't fall through to
/// the per-snapshot `SQLite` lookup, and the unreachable
/// `package_url` proves the network path is also bypassed.
#[tokio::test]
async fn reuses_prefetched_cas_paths_when_provided() {
    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let cache_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    // Synthetic cas-path map — its values just need to be returned
    // verbatim by the prefetched short-circuit. They don't need to
    // resolve to anything on disk because no integrity check runs
    // on this path.
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    files.insert("package.json".to_string(), PathBuf::from("/synthetic/package.json"));
    files.insert("bin/cli.js".to_string(), PathBuf::from("/synthetic/bin/cli.js"));
    let mut prefetched: PrefetchedCasPaths = HashMap::new();
    prefetched.insert(cache_key, Arc::new(files.clone()));

    // Use a leaked tempdir for `store_dir` so the helper has
    // somewhere to point even though we never read it.
    let (_keep, store_path) = tempdir_with_leaked_path();

    let cas_paths = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        // No SQLite handle: any fall-through to the per-snapshot
        // SQLite lookup would just miss, so a network attempt
        // would follow — and that would fail against the
        // unreachable URL below, failing the test.
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_integrity: Some(&pkg_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: "http://127.0.0.1:1/unreachable.tgz",
        package_id: pkg_id,
        requester: "",
        prefetched_cas_paths: Some(&prefetched),
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect("prefetched short-circuit should succeed without network");

    assert_eq!(cas_paths.len(), 2);
    assert_eq!(cas_paths.get("package.json"), files.get("package.json"));
    assert_eq!(cas_paths.get("bin/cli.js"), files.get("bin/cli.js"));
}

/// `prefetch_cas_paths` against an index row whose CAFS blobs
/// exist on disk and verify cleanly must return a hit for the
/// requested key. Mirrors the warm-cache install shape: we
/// pre-write a row, then ask the prefetch to look it up.
#[tokio::test]
async fn prefetch_cas_paths_returns_hits_for_live_index_rows() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let (pkg_json_path, pkg_json_hash) =
        store_path.write_cas_file(b"{\"name\":\"fake\"}", false).unwrap();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            digest: format!("{pkg_json_hash:x}"),
            mode: 0o644,
            size: 15,
            checked_at: None,
        },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: Some(true),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let prefetched = prefetch_cas_paths(
        StoreIndex::shared_readonly_in(store_path),
        store_path,
        vec![index_key.clone()],
        true,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    let map = prefetched.cas_paths.get(&index_key).expect("hit");
    assert_eq!(map.get("package.json"), Some(&pkg_json_path));
    assert_eq!(prefetched.requires_build.get(&index_key), Some(&false));
    assert_eq!(prefetched.requires_prepare.get(&index_key), Some(&true));
    drop(store_dir);
}

#[tokio::test]
async fn prefetch_cas_paths_recomputes_requires_build_for_legacy_rows() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let manifest_bytes = br#"{"name":"fake","scripts":{"postinstall":"node build.js"}}"#;
    let (pkg_json_path, pkg_json_hash) = store_path.write_cas_file(manifest_bytes, false).unwrap();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            digest: format!("{pkg_json_hash:x}"),
            mode: 0o644,
            size: manifest_bytes.len() as u64,
            checked_at: None,
        },
    );
    let entry = PackageFilesIndex {
        manifest: Some(serde_json::from_slice(manifest_bytes).unwrap()),
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let prefetched = prefetch_cas_paths(
        StoreIndex::shared_readonly_in(store_path),
        store_path,
        vec![index_key.clone()],
        true,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    let map = prefetched.cas_paths.get(&index_key).expect("hit");
    assert_eq!(map.get("package.json"), Some(&pkg_json_path));
    assert_eq!(prefetched.requires_build.get(&index_key), Some(&true));
    drop(store_dir);
}

/// `prefetch_cas_paths` must omit entries whose integrity check
/// fails — same policy as the per-snapshot `load_cached_cas_paths`
/// path. We seed an index row that points at a digest no file on
/// disk matches; the prefetch should drop the row from its result
/// rather than return a half-populated map (which would mislead
/// the warm-batch path into thinking the package was ready).
#[tokio::test]
async fn prefetch_cas_paths_omits_failed_integrity_entries() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            // Digest of a file that was never written to disk.
            digest: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            mode: 0o644,
            size: 15,
            checked_at: None,
        },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let prefetched = prefetch_cas_paths(
        StoreIndex::shared_readonly_in(store_path),
        store_path,
        vec![index_key.clone()],
        // Verification on: the missing CAFS blob trips
        // `check_pkg_files_integrity`'s "scrub & re-fetch" path,
        // which turns the row into a miss.
        true,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    assert!(
        !prefetched.cas_paths.contains_key(&index_key),
        "row that fails integrity must not appear in prefetch result",
    );
    drop(store_dir);
}

/// With `verify_store_integrity = false`, `prefetch_cas_paths`
/// goes through `build_file_maps_from_index` instead of
/// `check_pkg_files_integrity` — the index row is trusted and
/// no `fs::metadata` syscalls run per file. The result must
/// still surface an entry for the requested key, even when no
/// CAFS blob exists on disk; correctness is left to the caller's
/// downstream import step (matches pnpm's behaviour with
/// `verify-store-integrity: false`).
#[tokio::test]
async fn prefetch_cas_paths_skips_filesystem_checks_when_verify_disabled() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            // Digest matches no on-disk file, but with
            // `verify_store_integrity = false` we never check.
            digest: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".to_string(),
            mode: 0o644,
            size: 15,
            checked_at: None,
        },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let prefetched = prefetch_cas_paths(
        StoreIndex::shared_readonly_in(store_path),
        store_path,
        vec![index_key.clone()],
        false,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    let map = prefetched.cas_paths.get(&index_key).expect(
        "verify=false should trust the index row and surface the entry without checking disk",
    );
    assert!(map.contains_key("package.json"));
    drop(store_dir);
}

/// If the index row points at a CAFS blob that no longer exists on
/// disk (pruned out-of-band, say), the cache lookup must reject the
/// entry and fall through to a download. We don't want to do the
/// download for real in a unit test, so assert that we got a
/// `FetchTarball` error from the unreachable URL rather than the
/// cache-hit's `Ok`.
#[tokio::test]
async fn falls_through_when_cafs_file_missing() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    // A digest that matches no file on disk. `load_cached_cas_paths`
    // should see the missing path, reject the entry, and let
    // `run_without_mem_cache` proceed to the network fetch.
    files.insert(
        "package.json".to_string(),
        CafsFileInfo { digest: "0".repeat(128), mode: 0o644, size: 0, checked_at: None },
    );

    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let err = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
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
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("stale index entry must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
}

/// Write one store-index row whose bundled manifest names
/// `other-package@9.9.9`, keyed for `fake@1.0.0`.
fn seed_row_holding_another_package(store_path: &StoreDir, index_key: &str) {
    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo { digest: "0".repeat(128), mode: 0o644, size: 0, checked_at: None },
    );
    let entry = PackageFilesIndex {
        manifest: Some(serde_json::json!({ "name": "other-package", "version": "9.9.9" })),
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(index_key, &entry).unwrap();
    drop(index);
}

/// A lockfile that pairs an integrity with the wrong package, or a
/// registry serving a tarball that isn't what its metadata says, leaves
/// a store row whose `package.json` names another package. Reusing it
/// would install that other package under this name, so the read fails
/// instead — pnpm's `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE`.
#[tokio::test]
async fn store_row_holding_another_package_fails_the_read() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);
    seed_row_holding_another_package(store_path, &index_key);

    let err = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
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
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("a row holding another package must not be reused");
    let TarballError::UnexpectedPkgContentInStore { hint } = &err else {
        panic!("expected an unexpected-content error, got: {err:?}");
    };
    assert!(hint.contains("Expected package: fake@1.0.0."), "{hint}");
    assert!(hint.contains("Actual package in the store: other-package@9.9.9."), "{hint}");

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
    let cas_paths = DownloadTarballToStore {
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
        append_manifest: None,
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

/// A corrupt row whose digest is empty (or too short / non-hex) must
/// not panic inside `StoreDir::file_path_by_hex_str` (`hex[..2]`).
/// The validation in `cas_file_path_by_mode` rejects such rows, and
/// `load_cached_cas_paths` treats that as a cache miss.
#[tokio::test]
async fn falls_through_when_digest_is_malformed() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo { digest: String::new(), mode: 0o644, size: 0, checked_at: None },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let err = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
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
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("corrupt digest must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
}

/// A corrupted store might have a directory sitting where a CAFS blob
/// belongs (stray `mkdir -p`, interrupted write, whatever). `exists()`
/// would have let it through; `metadata().is_file()` rejects it.
#[tokio::test]
async fn falls_through_when_cafs_path_is_a_directory() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let digest = "a".repeat(128);
    let cafs_path = store_path
        .cas_file_path_by_mode(&digest, 0o644)
        .expect("128-char hex must produce a valid CAFS path");
    std::fs::create_dir_all(&cafs_path).unwrap();

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo { digest, mode: 0o644, size: 0, checked_at: None },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let err = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
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
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("directory at CAFS path must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
}

/// A symlink at the CAFS path — even one pointing at a valid regular
/// file — must not be trusted. A tampered / corrupted store could
/// place one pointing outside the store entirely, so we use
/// `symlink_metadata()` and reject symlinks regardless of target.
#[tokio::test]
#[cfg(not(target_os = "windows"))]
async fn falls_through_when_cafs_path_is_a_symlink() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let pkg_id = "fake@1.0.0";
    let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);

    let digest = "b".repeat(128);
    let cafs_path = store_path
        .cas_file_path_by_mode(&digest, 0o644)
        .expect("128-char hex must produce a valid CAFS path");
    std::fs::create_dir_all(cafs_path.parent().unwrap()).unwrap();

    // Plant a symlink at the CAFS path pointing at a real regular
    // file elsewhere. `metadata()` would have followed it and the
    // check would have (incorrectly) succeeded; `symlink_metadata()`
    // must reject the link itself.
    let target = store_dir.path().join("outside-the-cafs.txt");
    std::fs::write(&target, b"evil").unwrap();
    std::os::unix::fs::symlink(&target, &cafs_path).unwrap();

    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo { digest, mode: 0o644, size: 4, checked_at: None },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let err = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
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
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("symlink at CAFS path must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
}

/// `extract_tarball_entries` must propagate any tar-side failure —
/// corrupt header, short body read, path decode — as
/// [`TarballError::ReadTarballEntries`] rather than panicking inside
/// a blocking-pool task (which would take the whole install with it
/// and could leave the pool with dangling permits).
///
/// We don't invoke `decompress_gzip` here: the decompression layer
/// has its own error path and isn't the code under test. Driving
/// `extract_tarball_entries` directly isolates the tar iterator's
/// failure modes.
#[test]
fn extract_propagates_malformed_tar_instead_of_panicking() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    // 1 KiB of 0xFF: not a tar header (checksum at bytes 148..156
    // can't possibly match), so the iterator either yields an
    // `Err` on the first entry or errors on path decode. Either
    // way the filter+map_err plumbing must surface the failure as
    // `TarballError::ReadTarballEntries`.
    let bogus: Vec<u8> = vec![0xFF; 1024];
    let err = extract_tarball_entries(&bogus, store_path, None)
        .expect_err("malformed tar must surface a TarballError, not panic");

    assert!(
        matches!(err, TarballError::ReadTarballEntries(_)),
        "expected ReadTarballEntries, got: {err:?}",
    );

    drop(tempdir);
}

/// A tarball whose entry path contains `..` (or any other
/// non-`Normal` path component) must be rejected, not silently
/// normalized. Without the guard in `extract_tarball_entries`,
/// `cleaned_entry_path` would later be joined onto the CAFS
/// extraction root by `create_cas_files` and land files outside
/// the store (directory traversal).
///
/// Note: `tar::Header::set_path` refuses to write a `..` path on
/// its own (defense in depth on the write side). To exercise the
/// read-side guard we have to bypass that by writing the name
/// bytes directly via `as_mut_bytes()` and recomputing the
/// checksum. A malicious tarball in the wild could trivially be
/// written by any non-Rust tool that doesn't sanitize.
#[test]
fn extract_rejects_parent_dir_component_in_entry_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        // Bypass `set_path`'s `..` validation: write the raw
        // name bytes directly into header[0..100]. Then
        // `set_cksum()` recomputes the checksum over those bytes
        // so the reader doesn't trip its own integrity check.
        let raw = header.as_mut_bytes();
        let name = b"package/../evil.txt";
        raw[..name.len()].copy_from_slice(name);
        for result_b in &mut raw[name.len()..100] {
            *result_b = 0;
        }
        header.set_cksum();
        builder.append(&header, &b"evil!"[..]).expect("append entry");
        builder.finish().expect("finalize tar");
    }

    let err = extract_tarball_entries(&tar_bytes, store_path, None)
        .expect_err("parent-dir component must be rejected, not normalized");

    match err {
        TarballError::ReadTarballEntries(io_err) => {
            assert_eq!(io_err.kind(), std::io::ErrorKind::InvalidData);
        }
        other => panic!("expected ReadTarballEntries(InvalidData), got: {other:?}"),
    }

    drop(tempdir);
}

/// The tarball extractor's `ignore_file_pattern` plumbing must drop
/// the matched entries from *both* `cas_paths` and
/// `pkg_files_idx.files`. The Slice D dispatcher will rely on this
/// for runtime archive filtering (Node's bundled `npm` / `corepack`,
/// matching the `NODE_EXTRAS_IGNORE_PATTERN`); without coverage
/// here, a regression that, e.g., applied the filter to `cas_paths`
/// but forgot the `pkg_files_idx` row would slip past the existing
/// `None`-path tests.
#[test]
fn extract_tarball_applies_ignore_filter_dropping_entries_from_both_maps() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, body) in [
            ("package/bin/tool", &b"binary"[..]),
            ("package/lib/node_modules/npm/package.json", &b"{}"[..]),
            ("package/README.md", &b"readme"[..]),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append_data(&mut header, path, body).expect("append entry");
        }
        builder.finish().expect("finalize tar");
    }

    fn drop_npm(path: &str) -> bool {
        path.starts_with("lib/node_modules/npm/")
    }

    let (cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, Some(&drop_npm))
            .expect("tarball extraction with ignore filter");

    dbg!(&cas_paths);
    assert!(cas_paths.contains_key("bin/tool"));
    assert!(cas_paths.contains_key("README.md"));
    assert!(
        !cas_paths.contains_key("lib/node_modules/npm/package.json"),
        "ignore filter should drop bundled npm from cas_paths",
    );

    dbg!(&pkg_files_idx.files);
    assert!(pkg_files_idx.files.contains_key("bin/tool"));
    assert!(pkg_files_idx.files.contains_key("README.md"));
    assert!(
        !pkg_files_idx.files.contains_key("lib/node_modules/npm/package.json"),
        "ignore filter should drop bundled npm from pkg_files_idx.files",
    );
    assert_eq!(pkg_files_idx.requires_build, Some(false));

    drop(tempdir);
}

#[test]
fn extract_tarball_records_requires_build_from_manifest() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let body = br#"{"scripts":{"install":"node-gyp rebuild"}}"#;
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/package.json", &body[..])
            .expect("append manifest");
        builder.finish().expect("finalize tar");
    }

    let (_cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("tarball extraction");

    assert_eq!(pkg_files_idx.requires_build, Some(true));
    drop(tempdir);
}

/// Published packages ship `package.json` files carrying a UTF-8 BOM.
/// The bundled manifest and the install-script detection derived from it
/// must survive one, or the package silently loses its build pass.
#[test]
fn extract_tarball_reads_a_manifest_that_starts_with_a_utf8_bom() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let body = b"\xEF\xBB\xBF{\"name\":\"bom\",\"scripts\":{\"install\":\"node-gyp rebuild\"}}";
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/package.json", &body[..])
            .expect("append manifest");
        builder.finish().expect("finalize tar");
    }

    let (_cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("tarball extraction");

    assert_eq!(pkg_files_idx.requires_build, Some(true));
    assert!(pkg_files_idx.manifest.is_some(), "the bundled manifest must be recorded");
    drop(tempdir);
}

/// Build a gzipped tar that inflates to more than
/// [`MAX_UNTRUSTED_PREALLOC_BYTES`] from a compressed body small enough
/// that [`should_stream_extract`] sees nothing suspicious about it — the
/// gzip bomb the eager decode ceiling exists for.
///
/// `generated` entries are `(path, size)` pairs of filler produced
/// straight into the encoder, so the archive only ever exists
/// compressed; `verbatim` entries carry their own bytes.
fn gzip_bomb_tarball(generated: &[(&str, u64)], verbatim: &[(&str, &[u8])]) -> Vec<u8> {
    fn header(size: u64) -> tar::Header {
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        header
    }

    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for &(path, size) in generated {
        builder
            .append_data(&mut header(size), path, std::io::repeat(b'a').take(size))
            .expect("append generated entry");
    }
    for &(path, bytes) in verbatim {
        builder
            .append_data(&mut header(bytes.len() as u64), path, bytes)
            .expect("append verbatim entry");
    }
    builder.into_inner().expect("finish tar").finish().expect("finish gzip")
}

/// Nothing an archive says about itself bounds what it decodes to:
/// `dist.unpackedSize` is the publisher's claim and the compressed
/// length says nothing about the ratio, so the eager decode measures the
/// archive itself and stops at the ceiling.
#[test]
fn decompress_gzip_stops_at_the_eager_ceiling() {
    let bomb =
        gzip_bomb_tarball(&[("package/bomb.bin", MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1)], &[]);
    assert!(
        !should_stream_extract(bomb.len(), None),
        "the compressed body must look small enough to route to the eager path",
    );

    let err = decompress_gzip(&bomb, None).expect_err("the archive must not inflate past the cap");
    assert!(is_eager_decode_limit_exceeded(&err), "expected an output-limit refusal, got {err:?}");
    assert!(is_transient_error(&err), "a decode failure must remain retryable");
}

/// A lockfile records no unpacked size, so on a frozen install the
/// archive's own gzip trailer is the only claim about it there is — and
/// a good enough one to route a large archive straight to the streaming
/// extractor instead of discovering its size by decoding it twice.
#[test]
fn gzip_isize_hint_routes_a_large_archive_before_it_is_decoded() {
    let bomb =
        gzip_bomb_tarball(&[("package/bomb.bin", MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1)], &[]);
    let hint = gzip_isize_hint(&bomb).expect("a gzip stream carries an unpacked size");
    assert!(
        hint > MAX_UNTRUSTED_PREALLOC_BYTES,
        "the trailer must report the archive's real size, got {hint}",
    );
    assert!(should_stream_extract(bomb.len(), Some(hint)));

    assert_eq!(gzip_isize_hint(b"not a gzip stream at all"), None);
    // Faked magic over garbage: the trailer of a body that cannot decode
    // is not a size, and reading it as one would route the body away
    // from the path whose error says it is not a gzip stream.
    let mut faked = vec![0x1f_u8, 0x8b];
    faked.extend(std::iter::repeat_n(0xa5_u8, 64));
    assert_eq!(gzip_isize_hint(&faked), None);
}

/// Reaching the eager decode ceiling refuses no package: an archive that
/// under-reports its unpacked size takes the eager path, exceeds the
/// ceiling, and is extracted in full by the streaming extractor, which
/// holds a bounded window of it rather than the whole thing.
#[test]
fn extract_gzipped_tarball_streams_an_archive_past_the_eager_ceiling() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let payload_size = MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1;
    let mut bomb = gzip_bomb_tarball(
        &[("package/bomb.bin", payload_size), ("package/tail.txt", 3)],
        &[("package/package.json", br#"{"name":"bomb"}"#)],
    );
    // Rewrite the trailer's unpacked size so nothing warns the router
    // ahead of the decode — the shape a bomb takes once the trailer is
    // a signal.
    let trailer = bomb.len() - 4;
    bomb[trailer..].copy_from_slice(&1024_u32.to_le_bytes());
    assert!(
        !should_stream_extract(bomb.len(), gzip_isize_hint(&bomb)),
        "the fixture must look small enough to route to the eager path",
    );

    let (cas_paths, pkg_files_idx) = extract_gzipped_tarball(&bomb, None, store_path, None)
        .expect("an archive past the eager ceiling must still extract");

    dbg!(cas_paths.keys().collect::<Vec<_>>());
    assert_eq!(pkg_files_idx.files["bomb.bin"].size, payload_size);
    assert_eq!(
        std::fs::metadata(&cas_paths["bomb.bin"]).expect("stat the streamed entry").len(),
        payload_size,
        "the oversized entry must land in the CAS in full",
    );
    assert_eq!(
        std::fs::read(&cas_paths["tail.txt"]).expect("read the trailing entry"),
        b"aaa",
        "entries after the oversized one must still be extracted",
    );

    drop(tempdir);
}

#[test]
fn should_stream_extract_pivots_on_compressed_size_and_unpacked_hint() {
    assert!(!should_stream_extract(0, None));
    assert!(!should_stream_extract(STREAM_EXTRACT_COMPRESSED_THRESHOLD - 1, None));
    assert!(should_stream_extract(STREAM_EXTRACT_COMPRESSED_THRESHOLD, None));
    assert!(!should_stream_extract(0, Some(MAX_UNTRUSTED_PREALLOC_BYTES - 1)));
    assert!(should_stream_extract(0, Some(MAX_UNTRUSTED_PREALLOC_BYTES)));
    // A hostile hint only routes to the (still correct) streaming path.
    assert!(should_stream_extract(0, Some(usize::MAX)));
}

/// Build a tar archive spanning every entry shape the streaming
/// extractor branches on: a manifest, an executable, a small file, a
/// directory (skipped), and one payload above
/// [`STREAM_ENTRY_BUFFER_MAX`] that must take the
/// direct-to-store streaming branch.
fn mixed_size_tar() -> (Vec<u8>, Vec<u8>) {
    let large_payload: Vec<u8> =
        (0..=STREAM_ENTRY_BUFFER_MAX).map(|index| (index % 251) as u8).collect();

    let mut builder = tar::Builder::new(Vec::new());
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_size(0);
    dir_header.set_mode(0o755);
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_cksum();
    builder.append_data(&mut dir_header, "package/lib/", &b""[..]).expect("append dir entry");
    for (path, mode, body) in [
        (
            "package/package.json",
            0o644,
            &br#"{"name":"big","scripts":{"install":"node-gyp rebuild"}}"#[..],
        ),
        ("package/bin/tool", 0o755, &b"#!/bin/sh\necho hi\n"[..]),
        ("package/big.bin", 0o644, large_payload.as_slice()),
        ("package/lib/small.js", 0o644, &b"module.exports = 1\n"[..]),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(mode);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append_data(&mut header, path, body).expect("append entry");
    }
    let tar_bytes = builder.into_inner().expect("finish tar");
    (tar_bytes, large_payload)
}

fn gzip_bytes(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("gzip bytes");
    encoder.finish().expect("finish gzip")
}

/// The streaming extractor must produce byte-identical outputs to the
/// eager whole-archive extractor — same store-relative CAS paths, same
/// digests/modes/sizes in the index row, same bundled manifest and
/// requires-build flag — because [`should_stream_extract`] routes
/// between the two per download and the shared `index.db` must not be
/// able to tell them apart.
#[test]
fn streaming_extract_matches_eager_extract() {
    let (tar_bytes, large_payload) = mixed_size_tar();

    let (eager_tempdir, eager_store) = tempdir_with_leaked_path();
    let (eager_cas_paths, eager_idx) =
        extract_tarball_entries(&tar_bytes, eager_store, None).expect("eager extraction");

    let (streaming_tempdir, streaming_store) = tempdir_with_leaked_path();
    let (streaming_cas_paths, streaming_idx) =
        stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), streaming_store, None)
            .expect("streaming extraction");

    let relative =
        |cas_paths: &HashMap<String, PathBuf>, store: &StoreDir| -> HashMap<String, PathBuf> {
            cas_paths
                .iter()
                .map(|(key, path)| {
                    let path = path.strip_prefix(store.root()).expect("path within store");
                    (key.clone(), path.to_path_buf())
                })
                .collect()
        };
    assert_eq!(
        relative(&streaming_cas_paths, streaming_store),
        relative(&eager_cas_paths, eager_store),
    );

    let comparable = |idx: &PackageFilesIndex| -> Vec<(String, String, u32, u64)> {
        let mut rows: Vec<_> = idx
            .files
            .iter()
            .map(|(path, info)| (path.clone(), info.digest.clone(), info.mode, info.size))
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(comparable(&streaming_idx), comparable(&eager_idx));
    assert_eq!(streaming_idx.manifest, eager_idx.manifest);
    assert_eq!(streaming_idx.requires_build, Some(true));
    assert_eq!(streaming_idx.algo, eager_idx.algo);

    let large_cas_path = &streaming_cas_paths["big.bin"];
    assert_eq!(
        std::fs::read(large_cas_path).expect("read streamed large entry"),
        large_payload,
        "the direct-to-store streamed entry must land byte-identical content",
    );
    assert!(
        streaming_cas_paths["bin/tool"].to_string_lossy().ends_with("-exec"),
        "executable entries must keep the -exec CAS suffix on the streaming path",
    );

    drop(eager_tempdir);
    drop(streaming_tempdir);
}

/// Streaming-path counterpart of
/// [`extract_rejects_parent_dir_component_in_entry_path`] — the
/// traversal guard must hold on both extraction paths.
#[test]
fn streaming_extract_rejects_parent_dir_component_in_entry_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        // Same `set_path`-bypass as the eager-path test: raw name
        // bytes + recomputed checksum.
        let raw = header.as_mut_bytes();
        let name = b"package/../evil.txt";
        raw[..name.len()].copy_from_slice(name);
        for result_b in &mut raw[name.len()..100] {
            *result_b = 0;
        }
        header.set_cksum();
        builder.append(&header, &b"evil!"[..]).expect("append entry");
        builder.finish().expect("finalize tar");
    }

    let err = stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), store_path, None)
        .expect_err("parent-dir component must be rejected, not normalized");

    match err {
        TarballError::ReadTarballEntries(io_err) => {
            assert_eq!(io_err.kind(), ErrorKind::InvalidData);
        }
        other => panic!("expected ReadTarballEntries(InvalidData), got: {other:?}"),
    }

    drop(tempdir);
}

/// Streaming-path counterpart of
/// [`extract_tarball_applies_ignore_filter_dropping_entries_from_both_maps`].
#[test]
fn streaming_extract_applies_ignore_filter_dropping_entries_from_both_maps() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, body) in [
            ("package/bin/tool", &b"binary"[..]),
            ("package/lib/node_modules/npm/package.json", &b"{}"[..]),
            ("package/README.md", &b"readme"[..]),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append_data(&mut header, path, body).expect("append entry");
        }
        builder.finish().expect("finalize tar");
    }

    fn drop_npm(path: &str) -> bool {
        path.starts_with("lib/node_modules/npm/")
    }

    let (cas_paths, pkg_files_idx) =
        stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), store_path, Some(&drop_npm))
            .expect("streaming extraction with ignore filter");

    dbg!(&cas_paths);
    assert!(cas_paths.contains_key("bin/tool"));
    assert!(cas_paths.contains_key("README.md"));
    assert!(!cas_paths.contains_key("lib/node_modules/npm/package.json"));
    assert!(!pkg_files_idx.files.contains_key("lib/node_modules/npm/package.json"));
    assert_eq!(pkg_files_idx.requires_build, Some(false));

    drop(tempdir);
}

/// A `package.json` above [`STREAM_ENTRY_BUFFER_MAX`] must still be
/// buffered and parsed on the streaming path — routing it through the
/// direct-to-store branch would silently record
/// `requires_build: Some(false)` and drop the bundled manifest.
#[test]
fn streaming_extract_parses_manifest_larger_than_entry_buffer() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let padding = "p".repeat(usize::try_from(STREAM_ENTRY_BUFFER_MAX).unwrap() + 1);
    let manifest =
        format!(r#"{{"name":"pad","scripts":{{"install":"node-gyp rebuild"}},"x":"{padding}"}}"#);

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/package.json", manifest.as_bytes())
            .expect("append manifest");
        builder.finish().expect("finalize tar");
    }

    let (_cas_paths, pkg_files_idx) =
        stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), store_path, None)
            .expect("streaming extraction");

    assert_eq!(pkg_files_idx.requires_build, Some(true));
    assert!(pkg_files_idx.manifest.is_some(), "the bundled manifest must be recorded");
    drop(tempdir);
}

/// A `package.json` header claiming more than
/// [`MAX_UNTRUSTED_PREALLOC_BYTES`] must be rejected before its payload
/// is read — buffering it would defeat the streaming path's
/// bounded-memory guarantee, and skipping the parse would record wrong
/// build metadata. No payload follows the forged header below: the
/// guard has to fire on the header alone.
#[test]
fn streaming_extract_rejects_manifest_beyond_prealloc_cap() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut header = tar::Header::new_gnu();
    header.set_path("package/package.json").expect("set tar entry path");
    header.set_size(MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();

    let err = stream_extract_gzipped_tarball(&gzip_bytes(header.as_bytes()), store_path, None)
        .expect_err("oversized manifest must be rejected, not buffered");

    match err {
        TarballError::ReadTarballEntries(io_err) => {
            assert_eq!(io_err.kind(), ErrorKind::InvalidData);
        }
        other => panic!("expected ReadTarballEntries(InvalidData), got: {other:?}"),
    }

    drop(tempdir);
}

/// A large entry cut short by a truncated (but gzip-valid) archive
/// must fail extraction without committing the partial payload to the
/// CAS — the blob would be correctly content-addressed, but a
/// cut-short transfer must leave the store as it found it.
#[test]
fn streaming_extract_truncated_large_entry_commits_nothing() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    let mut header = tar::Header::new_gnu();
    header.set_path("package/big.bin").expect("set tar entry path");
    header.set_size(STREAM_ENTRY_BUFFER_MAX + 1);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar_bytes.extend_from_slice(header.as_bytes());
    // Only 1 KiB of the claimed payload is present.
    tar_bytes.extend_from_slice(&[0u8; 1024]);

    let err = stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), store_path, None)
        .expect_err("truncated large entry must fail extraction");
    assert!(
        matches!(err, TarballError::ReadTarballEntries(_)),
        "expected ReadTarballEntries, got: {err:?}",
    );

    fn count_files_recursively(dir: &Path) -> usize {
        std::fs::read_dir(dir).map_or(0, |entries| {
            entries
                .map(|entry| entry.expect("read dirent"))
                .map(|entry| {
                    if entry.file_type().expect("dirent file type").is_dir() {
                        count_files_recursively(&entry.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
    }
    assert_eq!(
        count_files_recursively(&store_path.root().join("files")),
        0,
        "the truncated entry must not commit anything to the CAS",
    );

    drop(tempdir);
}

/// Corrupt gzip bytes must surface as a [`TarballError`] the retry
/// classifier treats as transient, matching the eager path's
/// `DecodeGzip` handling; on the streaming path the decoder fails
/// through the tar reader as `ReadTarballEntries`.
#[test]
fn streaming_extract_propagates_corrupt_gzip_as_read_error() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let bogus: Vec<u8> = vec![0xFF; 1024];
    let err = stream_extract_gzipped_tarball(&bogus, store_path, None)
        .expect_err("corrupt gzip must surface a TarballError, not panic");

    assert!(
        matches!(err, TarballError::ReadTarballEntries(_)),
        "expected ReadTarballEntries, got: {err:?}",
    );
    assert!(is_transient_error(&err), "a corrupt stream must remain retryable");

    drop(tempdir);
}

/// A registry `dist.unpackedSize` at the streaming pivot routes the
/// full download pipeline — integrity verification included — through
/// the streaming extractor.
#[tokio::test]
async fn download_pipeline_extracts_via_streaming_path_for_large_unpacked_hint() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let (tar_bytes, large_payload) = mixed_size_tar();
    let body = gzip_bytes(&tar_bytes);
    let pkg_integrity = {
        let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
        opts.input(&body);
        opts.result()
    };

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(&body)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();

    let (computed_integrity, cas_paths, pkg_files_idx) =
        fetch_and_extract_with_retry::<SilentReporter>(
            &client,
            &url,
            Some(&pkg_integrity),
            Some(MAX_UNTRUSTED_PREALLOC_BYTES),
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
        .expect("download with a large unpacked-size hint");

    assert_eq!(computed_integrity.to_string(), pkg_integrity.to_string());
    assert!(cas_paths.contains_key("package.json"));
    assert!(cas_paths.contains_key("bin/tool"));
    assert_eq!(
        std::fs::read(&cas_paths["big.bin"]).expect("read streamed large entry"),
        large_payload,
    );
    assert_eq!(pkg_files_idx.requires_build, Some(true));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// `RetryOpts::default()` uses pnpm's network-fetch defaults: 2
/// retries, factor 10, minTimeout 10 s, maxTimeout 60 s. The first
/// post-failure delay is `minTimeout`; subsequent delays multiply by
/// `factor` until they hit `maxTimeout`.
#[test]
fn retry_opts_delay_matches_pnpm_formula() {
    let opts = RetryOpts::default();
    assert_eq!(opts.delay_for(0), Duration::from_secs(10));
    // 10s * 10 = 100s, capped at 60s
    assert_eq!(opts.delay_for(1), Duration::from_mins(1));
    assert_eq!(opts.delay_for(5), Duration::from_mins(1));
}

/// Pathological `attempt` values must not panic / overflow. The
/// retry loop uses `attempt: u32`, so the worst case in production
/// is bounded by `retries`, but we want the math to stay sound
/// regardless.
#[test]
fn retry_opts_delay_does_not_overflow() {
    let opts = RetryOpts::default();
    assert_eq!(opts.delay_for(u32::MAX), Duration::from_mins(1));
}

#[test]
fn retry_classification_matches_pnpm_policy() {
    let url = "https://example.test/pkg.tgz".to_string();
    let mk_http =
        |status: u16| TarballError::HttpStatus(HttpStatusError { url: url.clone(), status });

    for code in [401u16, 403, 404] {
        assert!(!is_transient_error(&mk_http(code)), "HTTP {code} should fail fast");
    }
    for code in [400u16, 408, 409, 410, 418, 420, 422, 429, 500, 502, 503, 504] {
        assert!(is_transient_error(&mk_http(code)), "HTTP {code} should retry");
    }

    // Non-HTTP failures: pnpm wraps body fetch + addFilesFromTarball
    // (integrity + extraction) in one retried closure, so anything
    // raised inside that closure retries. Cover a representative
    // sample.
    let bad_integrity: Integrity =
        "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().unwrap();
    let ssri_err = bad_integrity.check(b"unrelated body").unwrap_err();
    let checksum =
        TarballError::Checksum(VerifyChecksumError { url: url.clone(), error: ssri_err });
    assert!(is_transient_error(&checksum), "integrity mismatch should retry");

    let too_large = TarballError::TarballTooLarge { url: url.clone(), advertised_size: u64::MAX };
    assert!(is_transient_error(&too_large), "TarballTooLarge should retry");
}

/// Real pnpm-published tarball (`@fastify/error@3.3.0`, 4.4 KiB).
/// Embedded so the retry-success test below has a body that
/// integrity-checks and extracts successfully on the retry attempt
/// — which is the only way to exercise the post-network steps of
/// the retry loop without going to the live registry.
const FASTIFY_ERROR_TARBALL: &[u8] =
    include_bytes!("../../../tasks/micro-benchmark/fixtures/@fastify+error-3.3.0.tgz");
const FASTIFY_ERROR_INTEGRITY: &str = "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w==";

/// `RetryOpts` for the mockito tests below: keep the 2-retry budget
/// so we exercise the full attempt count, but collapse the backoff
/// to milliseconds so the test suite isn't sitting through pnpm's
/// production 10 s + 60 s waits.
fn fast_retry_opts() -> RetryOpts {
    RetryOpts {
        retries: 2,
        factor: 1,
        min_timeout: Duration::from_millis(1),
        max_timeout: Duration::from_millis(1),
    }
}

#[test]
fn local_file_tarball_path_rejects_hosted_file_urls() {
    assert_eq!(local_file_tarball_path("file://server/share/pkg.tgz"), None);
}

#[test]
fn local_file_tarball_path_rejects_unc_like_fallback_paths() {
    assert_eq!(local_file_tarball_path("file:////server/share/pkg.tgz"), None);
    assert_eq!(local_file_tarball_path(r"file:\\server\share\pkg.tgz"), None);
}

#[test]
fn local_file_tarball_path_accepts_relative_file_specs() {
    assert_eq!(
        local_file_tarball_path("file:../vendor/pkg.tgz"),
        Some(PathBuf::from("../vendor/pkg.tgz")),
    );
}

#[test]
fn allocate_local_tarball_buffer_rejects_absurd_size_as_local_read_error() {
    let path = Path::new("pkg.tgz");
    let err = allocate_local_tarball_buffer(path, "file:pkg.tgz", u64::MAX)
        .expect_err("local oversized tarballs should fail before reading");
    match err {
        TarballError::ReadLocalTarball { path: got_path, source } => {
            assert_eq!(got_path, path);
            assert_eq!(source.kind(), ErrorKind::InvalidData);
            assert!(source.to_string().contains("too large"), "got: {source}");
        }
        other => panic!("expected ReadLocalTarball, got {other:?}"),
    }
}

#[tokio::test]
async fn open_local_tarball_rejects_directories() {
    let local_dir = tempdir().unwrap();
    let err = open_local_tarball(local_dir.path())
        .await
        .expect_err("local tarballs must be regular files");
    match err {
        TarballError::ReadLocalTarball { path, source } => {
            assert_eq!(path, local_dir.path());
            assert_eq!(source.kind(), ErrorKind::InvalidInput);
            assert!(source.to_string().contains("regular file"), "got: {source}");
        }
        other => panic!("expected ReadLocalTarball, got {other:?}"),
    }
}

#[tokio::test]
async fn read_local_tarball_buffer_rejects_growth_past_checked_size() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, b"abcd").unwrap();
    let file = tokio::fs::File::open(&tarball_path).await.unwrap();

    let err = read_local_tarball_buffer(file, &tarball_path, "file:pkg.tgz", 3)
        .await
        .expect_err("local tarball reads must be capped at the checked size");
    match err {
        TarballError::ReadLocalTarball { path, source } => {
            assert_eq!(path, tarball_path);
            assert_eq!(source.kind(), ErrorKind::InvalidData);
            assert!(source.to_string().contains("changed while reading"), "got: {source}");
        }
        other => panic!("expected ReadLocalTarball, got {other:?}"),
    }
}

#[tokio::test]
async fn read_local_tarball_metadata_reads_integrity_and_bundled_manifest() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, FASTIFY_ERROR_TARBALL).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("read the local tarball's metadata");

    assert_eq!(metadata.integrity.to_string(), FASTIFY_ERROR_INTEGRITY);
    let manifest = metadata.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("@fastify/error"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("3.3.0"));
}

/// A `file:` tarball is read to learn the name and version its
/// specifier does not carry, so the eager decode ceiling must not turn
/// a large local archive into an unresolvable dependency: past the
/// ceiling the manifest is found by streaming instead.
#[tokio::test]
async fn read_local_tarball_metadata_reads_a_manifest_past_the_eager_ceiling() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");

    let archive = gzip_bomb_tarball(
        &[("package/payload.bin", MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1)],
        &[("package/package.json", br#"{"name":"huge","version":"1.0.0"}"#)],
    );
    std::fs::write(&tarball_path, &archive).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("an archive past the eager ceiling must still resolve");

    let manifest = metadata.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("huge"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("1.0.0"));
    assert!(metadata.has_manifest_entry);
}

/// The manifest is the package's only source of identity here, so an
/// unparsable one fails the resolve rather than degrading to `None` —
/// matching how pnpm rejects the same tarball.
#[tokio::test]
async fn read_local_tarball_metadata_rejects_an_unparsable_manifest() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("package/package.json", b"{ BROKEN")])).unwrap();

    let err = read_local_tarball_metadata(&tarball_path)
        .await
        .expect_err("an unparsable bundled manifest must fail the read");
    match err {
        TarballError::ParseBundledManifest { tarball, .. } => {
            assert_eq!(tarball, tarball_path.display().to_string());
        }
        other => panic!("expected ParseBundledManifest, got {other:?}"),
    }
}

/// Duplicate `package.json` entries are last-entry-wins, matching
/// `extract_tarball_entries`, so only the surviving one is parsed — the
/// two reads must agree on which manifest describes the package.
#[tokio::test]
async fn read_local_tarball_metadata_lets_a_later_manifest_supersede_a_malformed_one() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(
        &tarball_path,
        gzipped_tar(&[
            ("package/package.json", b"{ BROKEN"),
            ("package/package.json", br#"{"name":"dup-pkg","version":"2.0.0"}"#),
        ]),
    )
    .unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("the surviving manifest parses, so the read succeeds");
    let manifest = metadata.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("dup-pkg"));
}

/// An archive with no `package.json` at all is a different shape from a
/// corrupt one: pnpm installs it, so the read degrades to `None` instead
/// of failing.
#[tokio::test]
async fn read_local_tarball_metadata_tolerates_an_archive_with_no_manifest() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("package/README.md", b"hi")])).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("an archive without a manifest still reads");
    assert!(metadata.manifest.is_none(), "got {:?}", metadata.manifest);
}

fn gzipped_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;

    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).expect("set tar entry path");
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, *bytes).expect("append tar entry");
    }
    let tar_bytes = builder.into_inner().expect("finish tar");

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).expect("gzip tar");
    encoder.finish().expect("finish gzip")
}

/// The local resolver maps this to `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND`,
/// so the error kind has to survive.
#[tokio::test]
async fn read_local_tarball_metadata_reports_a_missing_file_as_not_found() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("missing.tgz");

    let err =
        read_local_tarball_metadata(&tarball_path).await.expect_err("a missing tarball must fail");
    match err {
        TarballError::ReadLocalTarball { path, source } => {
            assert_eq!(path, tarball_path);
            assert_eq!(source.kind(), ErrorKind::NotFound);
        }
        other => panic!("expected ReadLocalTarball, got {other:?}"),
    }
}

#[tokio::test]
async fn fetch_and_extract_records_expected_or_computed_integrity() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, FASTIFY_ERROR_TARBALL).unwrap();

    let package_url = format!("file:{}", tarball_path.display());
    let client = fast_fail_client();
    let mut sha1 = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha1);
    sha1.input(FASTIFY_ERROR_TARBALL);
    let sha1 = sha1.result();
    let sha512 = integrity(FASTIFY_ERROR_INTEGRITY);
    let package_id = "@fastify/error@3.3.0";
    for package_integrity in [Some(&sha1), None] {
        let expected = package_integrity.unwrap_or(&sha512);
        let (store_dir, store_path) = tempdir_with_leaked_path();
        let (writer, writer_task) = StoreIndexWriter::spawn(store_path);
        let result = DownloadTarballToStore {
            http_client: &client,
            store_dir: store_path,
            store_index: None,
            store_index_writer: Some(Arc::clone(&writer)),
            verify_store_integrity: true,
            strict_store_pkg_content_check: true,
            package_integrity,
            package_unpacked_size: Some(16697),
            package_file_count: None,
            package_url: &package_url,
            package_id,
            requester: "",
            prefetched_cas_paths: None,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            retry_opts: test_retry_opts(),
            auth_headers: &AuthHeaders::default(),
            ignore_file_pattern: None,
            offline: true,
            progress_reported: None,
            append_manifest: None,
        }
        .fetch_and_extract::<SilentReporter>()
        .await
        .expect("local tarballs should be read from disk without network access");

        assert_eq!(&result.integrity, expected);
        let manifest = result.manifest.expect("bundled manifest");
        assert_eq!(manifest["name"], "@fastify/error");
        assert_eq!(manifest["version"], "3.3.0");
        assert!(!result.requires_build, "fixture has no install script");
        assert!(result.files_map.contains_key("package.json"));

        drop(writer);
        writer_task.await.expect("writer task").expect("writer flushed");
        let index = StoreIndex::open_in(store_path).expect("open store index");
        let key = store_index_key(&expected.to_string(), package_id);
        assert_eq!(index.keys().expect("read index keys"), vec![key.clone()]);
        let entry = index.get(&key).expect("read index entry").expect("archive is indexed");
        assert_eq!(entry.manifest, Some(manifest));
        assert_eq!(entry.requires_build, Some(false));
        drop((index, store_dir));
    }
}

/// A resolution that pins no integrity is downloaded unverified, and
/// the fetch claims no store-index row: the key pnpm addresses such a
/// package by (`pickStoreIndexKey`'s `pkg_id\tbuilt` fallback) belongs
/// to the git-hosted prepare pass, which writes the *prepared* file set
/// there.
#[tokio::test]
async fn run_without_mem_cache_fetches_unverified_and_writes_no_index_row() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, FASTIFY_ERROR_TARBALL).unwrap();

    let (store_dir, store_path) = tempdir_with_leaked_path();
    let package_url = format!("file:{}", tarball_path.display());
    let client = fast_fail_client();
    let (writer, writer_task) = StoreIndexWriter::spawn(store_path);
    let cas_paths = DownloadTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: Some(Arc::clone(&writer)),
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_integrity: None,
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &package_url,
        package_id: "@fastify/error@3.3.0",
        requester: "",
        prefetched_cas_paths: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        append_manifest: None,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect("a resolution without an integrity should still be fetched");

    assert!(cas_paths.contains_key("package.json"));

    drop(writer);
    writer_task.await.expect("writer task").expect("writer flushed");
    let index = StoreIndex::open_in(store_path).expect("open store index");
    let keys: Vec<String> = index.keys().expect("read index keys");
    assert!(keys.is_empty(), "an unverified fetch must claim no index row: {keys:?}");

    drop((store_dir, local_dir));
}

#[tokio::test]
async fn retries_then_succeeds_on_transient_5xx() {
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

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
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

    // Sanity-check: extraction actually populated the cas-paths map.
    assert!(cas_paths.contains_key("package.json"));
    fail.assert_async().await;
    ok.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn revision_addressed_tarball_does_not_retry_a_transient_failure() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let digest = "A".repeat(86);
    let path = format!("/-/tarballs/sha512/{digest}");
    let mock = server.mock("GET", path.as_str()).with_status(503).expect(1).create_async().await;
    let url = format!("{}{path}", server.url());
    let expected = integrity(&format!("sha512-{digest}=="));

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&expected),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        true,
    )
    .await
    .expect_err("a revision-addressed 503 must fail after one request");

    assert!(matches!(err, TarballError::HttpStatus(_)), "got {err:?}");
    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn revision_addressed_tarball_does_not_follow_a_redirect() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let digest = "A".repeat(86);
    let path = format!("/-/tarballs/sha512/{digest}");
    let redirect = server
        .mock("GET", path.as_str())
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .expect(1)
        .create_async()
        .await;
    let redirected = server
        .mock("GET", "/redirected.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}{path}", server.url());
    let expected = integrity(&format!("sha512-{digest}=="));

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&expected),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        true,
    )
    .await
    .expect_err("a revision-addressed redirect must not be followed");

    assert!(matches!(err, TarballError::HttpStatus(_)), "got {err:?}");
    redirect.assert_async().await;
    redirected.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn revision_addressed_mem_cache_does_not_retry_a_failed_prefetch() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let digest = "A".repeat(86);
    let path = format!("/-/tarballs/sha512/{digest}");
    let mock = server.mock("GET", path.as_str()).with_status(503).expect(1).create_async().await;
    let url = format!("{}{path}", server.url());
    let expected = integrity(&format!("sha512-{digest}=="));
    let client = ThrottledClient::default();
    let mem_cache = MemCache::default();
    let auth_headers = AuthHeaders::default();
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let download = || DownloadTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&expected),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: "test-pkg",
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &auth_headers,
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        append_manifest: None,
    };

    let (first, second) = futures_util::future::join(
        download().run_revision_addressed_with_mem_cache::<SilentReporter>(&mem_cache),
        download().run_revision_addressed_with_mem_cache::<SilentReporter>(&mem_cache),
    )
    .await;
    let first = first.expect_err("the first revision-addressed consumer must fail");
    let second = second.expect_err("the second revision-addressed consumer must fail");
    assert!(
        matches!(&first, TarballError::HttpStatus(_))
            && matches!(&second, TarballError::SiblingFetchFailed { .. })
            || matches!(&second, TarballError::HttpStatus(_))
                && matches!(&first, TarballError::SiblingFetchFailed { .. }),
        "one consumer must own the request and the other inherit its failure; got {first:?} and {second:?}",
    );

    let later = download()
        .run_revision_addressed_with_mem_cache::<SilentReporter>(&mem_cache)
        .await
        .expect_err("a later consumer must inherit the terminal failure");
    assert!(matches!(later, TarballError::SiblingFetchFailed { .. }), "got {later:?}");

    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn revision_addressed_mem_cache_does_not_reuse_a_redirect_permitting_fetch() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/-/tarballs/sha512/digest")
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .expect(2)
        .create_async()
        .await;
    let redirected = server
        .mock("GET", "/redirected.tgz")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/-/tarballs/sha512/digest", server.url());
    let expected = integrity(FASTIFY_ERROR_INTEGRITY);
    let client = ThrottledClient::default();
    let mem_cache = MemCache::default();
    let auth_headers = AuthHeaders::default();
    let verified_files_cache = SharedVerifiedFilesCache::default();
    let download = || DownloadTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: Some(&expected),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &url,
        package_id: "test-pkg",
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &auth_headers,
        ignore_file_pattern: None,
        offline: false,
        progress_reported: None,
        append_manifest: None,
    };

    download()
        .run_with_mem_cache::<SilentReporter>(&mem_cache)
        .await
        .expect("an ordinary fetch may follow the redirect");
    let err = download()
        .run_revision_addressed_with_mem_cache::<SilentReporter>(&mem_cache)
        .await
        .expect_err("a revision fetch must make its own request and reject the redirect");

    assert!(matches!(err, TarballError::HttpStatus(_)), "got {err:?}");
    redirect.assert_async().await;
    redirected.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn retries_integrity_mismatch_until_exhausted() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    // 2 retries + 1 initial = 3 attempts; every one returns the same
    // body, which the wrong integrity hash will reject.
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(b"definitely not a tarball matching the digest below")
        .expect(3)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    // Real-format integrity, deliberately not matching the body above.
    let pkg_integrity = integrity(
        "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==",
    );

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
    .expect_err("integrity mismatch should exhaust the retry budget");
    assert!(matches!(err, TarballError::Checksum(_)), "expected Checksum error, got {err:?}");
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// Integrity-less tarball resolutions must be completed from the
/// downloaded bytes before they are written to the lockfile.
#[tokio::test]
async fn fetch_for_resolution_computes_integrity_when_none_is_expected() {
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

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        manifest_subdir: None,
    }
    .run::<SilentReporter>(None)
    .await
    .expect("a registry that omits integrity should get it computed from the bytes");

    assert_eq!(resolved.integrity, integrity(FASTIFY_ERROR_INTEGRITY));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// `FetchTarballForResolution` must forward its `package_id` (the package's
/// `name@version`) for auth/scope selection, so a private scoped registry tarball
/// resolves its scope token while its integrity is computed during resolution.
#[tokio::test]
async fn fetch_for_resolution_uses_package_id_for_scoped_auth() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let registry_key = format!("{}@scope", pnpm_network::nerf_dart(&server.url()));
    let auth_headers =
        AuthHeaders::from_creds_map([(registry_key, "Bearer scoped-token".to_owned())]);

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: "@scope/test-pkg@1.0.0",
        auth_headers: &auth_headers,
        retry_opts: fast_retry_opts(),
        manifest_subdir: None,
    }
    .run::<SilentReporter>(None)
    .await
    .expect("the scope token selected via package_id should let the fetch succeed");

    assert_eq!(resolved.integrity, integrity(FASTIFY_ERROR_INTEGRITY));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// Gzip a tar holding `(path, contents)` entries, under the top-level
/// prefix a git host's archive carries.
fn gzipped_archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("repo-abc123/{path}"), contents.as_bytes())
                .expect("append entry");
        }
        builder.finish().expect("finalize tar");
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut encoder, &tar_bytes).expect("gzip tar");
    encoder.finish().expect("finish gzip")
}

/// A git dep pointing at one directory of a repo (`#path:/packages/foo`)
/// gets an archive spanning the whole repo, so the root `package.json`
/// is the repo's, not the package's. Reading the root would name the
/// lockfile key after the wrong package.
#[tokio::test]
async fn fetch_for_resolution_reads_manifest_from_subdirectory() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let archive = gzipped_archive(&[
        ("package.json", r#"{"name":"the-monorepo","version":"0.0.0"}"#),
        ("packages/foo/package.json", r#"{"name":"foo","version":"1.2.3"}"#),
    ]);
    let mock =
        server.mock("GET", "/repo.tgz").with_status(200).with_body(archive).create_async().await;

    let url = format!("{}/repo.tgz", server.url());
    let client = ThrottledClient::default();

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        // Leading slash, exactly as the resolution records it.
        manifest_subdir: Some("/packages/foo"),
    }
    .run::<SilentReporter>(None)
    .await
    .expect("subdirectory manifest should be readable from the extracted archive");

    let manifest = dbg!(resolved.manifest).expect("subdirectory manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("foo"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("1.2.3"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// The row would be keyed by the subpackage (read from
/// `<subdir>/package.json`) while carrying an index built from the
/// whole archive — the repo's manifest and every repo file. Writing
/// that would hand any consumer trusting the key a payload describing
/// the repo, so no row is written at all.
#[tokio::test]
async fn fetch_for_resolution_writes_no_index_row_for_a_subdirectory_package() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let archive = gzipped_archive(&[
        ("package.json", r#"{"name":"the-monorepo","version":"0.0.0"}"#),
        ("packages/foo/package.json", r#"{"name":"foo","version":"1.2.3"}"#),
    ]);
    let _mock =
        server.mock("GET", "/repo.tgz").with_status(200).with_body(archive).create_async().await;

    let url = format!("{}/repo.tgz", server.url());
    let client = ThrottledClient::default();
    let (writer, writer_task) = StoreIndexWriter::spawn(store_path);

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: Some(Arc::clone(&writer)),
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        manifest_subdir: Some("/packages/foo"),
    }
    .run::<SilentReporter>(None)
    .await
    .expect("subdirectory fetch should succeed");

    drop(writer);
    writer_task.await.expect("writer task").expect("writer flushed");

    // The key the row *would* have taken, had one been written.
    let key = store_index_key(&resolved.integrity.to_string(), "foo@1.2.3");
    let index = StoreIndex::open_in(store_path).expect("open store index");
    let rows = index.get_many(std::slice::from_ref(&key)).expect("read index");
    assert!(rows.is_empty(), "a subpackage key must not carry the whole repo's index: {rows:?}");
    drop(store_dir_keep);
}

/// A subdirectory without its own `package.json` degrades to `None`,
/// the same best-effort contract the archive root has — never the
/// root's manifest, which would name the key after the wrong package.
#[tokio::test]
async fn fetch_for_resolution_returns_no_manifest_for_subdirectory_without_one() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let archive = gzipped_archive(&[
        ("package.json", r#"{"name":"the-monorepo","version":"0.0.0"}"#),
        ("packages/foo/index.js", "module.exports = 1"),
    ]);
    let mock =
        server.mock("GET", "/repo.tgz").with_status(200).with_body(archive).create_async().await;

    let url = format!("{}/repo.tgz", server.url());
    let client = ThrottledClient::default();

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        manifest_subdir: Some("/packages/foo"),
    }
    .run::<SilentReporter>(None)
    .await
    .expect("a subdirectory without a package.json is not a fetch failure");

    assert_eq!(dbg!(resolved.manifest), None);
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// 404 is in pnpm's no-retry set. `expect(1)` makes the test fail if
/// the retry loop fires a second request — that would mean we're
/// spinning on a permanently-missing tarball.
#[tokio::test]
async fn fails_fast_on_404() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/missing.tgz").with_status(404).expect(1).create_async().await;

    let url = format!("{}/missing.tgz", server.url());
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
    .expect_err("404 must fail-fast without retry");
    match err {
        TarballError::HttpStatus(http) => assert_eq!(http.status, 404),
        other => panic!("expected HttpStatus(404), got: {other:?}"),
    }
    mock.assert_async().await;
    drop(store_dir_keep);
}

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
                let make_dts = |url: &'static str| DownloadTarballToStore {
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
                    append_manifest: None,
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

/// When [`AuthHeaders`] resolves a credential for the tarball URL,
/// the GET request must carry the `Authorization` header — including
/// for tarball hosts that differ from the metadata host.
/// `mockito::Matcher::Exact` rejects the request unless the header
/// matches verbatim, so a missing or wrong header would 501 the
/// request and fail the integrity check downstream.
#[tokio::test]
async fn fetch_attaches_authorization_header_when_creds_match_tarball_url() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let auth_headers = AuthHeaders::from_creds_map([(
        pnpm_network::nerf_dart(&url),
        "Bearer test-token".to_owned(),
    )]);

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &auth_headers,
        None,
        None,
        false,
    )
    .await
    .expect("server should accept the request once the bearer header is attached");

    assert!(cas_paths.contains_key("package.json"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn fetch_attaches_authorization_header_when_scope_creds_match_package_id() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer scoped-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let registry_key = format!("{}@scope", pnpm_network::nerf_dart(&server.url()));
    let auth_headers =
        AuthHeaders::from_creds_map([(registry_key, "Bearer scoped-token".to_owned())]);

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "@scope/test-pkg@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &auth_headers,
        None,
        None,
        false,
    )
    .await
    .expect("server should accept the request once the scoped bearer header is attached");

    assert!(cas_paths.contains_key("package.json"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// The retry loop must re-attach the `Authorization` header on every
/// attempt, not just the first. A regression that read `auth_headers`
/// once outside the loop would pass the single-attempt test
/// [`fetch_attaches_authorization_header_when_creds_match_tarball_url`]
/// but silently 401 on the retried call. Mock returns 503 then 200,
/// both gated on the bearer header.
#[tokio::test]
async fn retry_re_attaches_authorization_header_on_each_attempt() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let fail = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer test-token")
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let ok = server
        .mock("GET", "/pkg.tgz")
        .match_header("authorization", "Bearer test-token")
        .with_status(200)
        .with_body(FASTIFY_ERROR_TARBALL)
        .expect(1)
        .create_async()
        .await;

    let url = format!("{}/pkg.tgz", server.url());
    let client = ThrottledClient::default();
    let pkg_integrity = integrity(FASTIFY_ERROR_INTEGRITY);
    let auth_headers = AuthHeaders::from_creds_map([(
        pnpm_network::nerf_dart(&url),
        "Bearer test-token".to_owned(),
    )]);

    let (_integrity, cas_paths, _idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "test-pkg",
        "",
        store_path,
        fast_retry_opts(),
        &auth_headers,
        None,
        None,
        false,
    )
    .await
    .expect("retry attempt should also carry the bearer header");

    assert!(cas_paths.contains_key("package.json"));
    // Both mocks must have fired: header missing on the retry would
    // mean the second `match_header` rejects (501) and the test fails
    // either at this assertion or at the integrity check.
    fail.assert_async().await;
    ok.assert_async().await;
    drop(store_dir_keep);
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
    DownloadTarballToStore {
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
        append_manifest: None,
    }
    .run_with_mem_cache::<pnpm_reporter::SilentReporter>(&mem_cache)
    .await
    .expect("first call should populate the mem cache");

    // Second requester: same URL, different `package_id`. Hits the
    // immediate-`Available` branch and emits one `found_in_store`
    // because no shared progress set says this package status was
    // already reported.
    EVENTS.lock().unwrap().clear();
    DownloadTarballToStore {
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
        append_manifest: None,
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
    DownloadTarballToStore {
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
        append_manifest: None,
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
    DownloadTarballToStore {
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
        append_manifest: None,
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
    // borrow-style `DownloadTarballToStore` without lifetime
    // gymnastics on the spawned futures. The test scope is short and
    // the leak is negligible.
    let client: &'static ThrottledClient = Box::leak(Box::new(ThrottledClient::default()));
    let pkg_integrity: &'static Integrity = Box::leak(Box::new(integrity(FASTIFY_ERROR_INTEGRITY)));
    let url: &'static str = Box::leak(url.into_boxed_str());
    let mem_cache: &'static MemCache = Box::leak(Box::new(MemCache::default()));
    let auth_headers: &'static AuthHeaders = Box::leak(Box::<AuthHeaders>::default());

    let make_dts = || DownloadTarballToStore {
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
        append_manifest: None,
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

/// `pnpm:fetching-progress` and `pnpm:progress` fire from inside the
/// tarball pipeline:
///
/// * `pnpm:fetching-progress started` once per *attempt* — so a 503 +
///   200 retry pattern emits twice with `attempt = 1` then
///   `attempt = 2` (one-indexed, matching pnpm's wire shape — the
///   default reporter's `reportBigTarballsProgress` filters on
///   `attempt === 1`). `size` carries the response's `Content-Length`
///   (mockito sends one for `with_body`).
/// * `pnpm:fetching-progress in_progress` is throttled to ~200ms; the
///   tiny FASTIFY tarball used here downloads in well under that, so
///   we don't assert any `in_progress` events fire.
/// * `pnpm:progress fetched` fires once after the retry loop returns
///   `Ok` — never when an attempt fails — with the `package_id` and
///   `requester` threaded down from the install layer.
#[tokio::test]
async fn fetching_progress_and_fetched_events_fire_during_download() {
    use std::sync::Mutex;

    use pnpm_reporter::{FetchingProgressMessage, LogEvent, ProgressMessage, Reporter as _};

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
    let _ = RecordingReporter::emit; // referenced via turbofish below

    fetch_and_extract_with_retry::<RecordingReporter>(
        &client,
        &url,
        Some(&pkg_integrity),
        None,
        0,
        "@fastify/error@3.3.0",
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
    let started: Vec<(u32, Option<u64>)> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::FetchingProgress(log) => match &log.message {
                FetchingProgressMessage::Started { attempt, package_id, size } => {
                    assert_eq!(package_id, "@fastify/error@3.3.0");
                    Some((*attempt, *size))
                }
                FetchingProgressMessage::InProgress { .. } => None,
            },
            _ => None,
        })
        .collect();
    let attempts: Vec<u32> = started.iter().map(|(result_a, _)| *result_a).collect();
    assert_eq!(attempts, vec![1, 2], "started must fire once per attempt; got {captured:?}");
    // Both attempts have a response head (mockito sends Content-Length
    // for `with_body(...)` and `with_status(503)` likewise), so both
    // `started` events must carry a populated `size`. This guards
    // against emitting `started` before the response head arrives,
    // which would leave `size` always-`null` (Copilot review on
    // <https://github.com/pnpm/pacquet/pull/372>).
    for (attempt, size) in &started {
        assert!(size.is_some(), "attempt {attempt} should expose Content-Length, got null");
    }

    let fetched_count = captured
        .iter()
        .filter(|e| {
            matches!(
                e,
                LogEvent::Progress(log)
                    if matches!(&log.message, ProgressMessage::Fetched { .. }),
            )
        })
        .count();
    assert_eq!(fetched_count, 1, "fetched must fire exactly once on success");

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

    use pnpm_reporter::{FetchingProgressMessage, LogEvent};

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
            LogEvent::FetchingProgress(log) => match &log.message {
                FetchingProgressMessage::Started { size, .. } => Some(*size),
                FetchingProgressMessage::InProgress { .. } => None,
            },
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

    DownloadTarballToStore {
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
        append_manifest: None,
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
    DownloadTarballToStore {
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
        append_manifest: None,
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

/// Build a zip archive in memory with the given `(name, body)`
/// entries. Entries are stored uncompressed (`Stored`) so the test
/// doesn't depend on the deflate backend the production reader
/// uses; the zip reader handles both transparently. The high byte
/// of `unix_mode` is the entry type per stat(2) — `0o100000` for a
/// regular file — and the low bytes are the permission bits.
fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o100644);
        for (name, body) in entries {
            writer.start_file(*name, opts).expect("start zip entry");
            writer.write_all(body).expect("write zip entry body");
        }
        writer.finish().expect("finalize zip archive");
    }
    buf
}

/// The install dispatcher will later resolve `bin/node` against
/// `cas_paths` and that lookup must hit the stripped form, not the
/// prefixed form.
#[test]
fn extract_zip_strips_prefix_from_entry_paths() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let bytes = build_zip(&[
        ("node-v22.0.0-darwin-arm64/bin/node", b"binary contents"),
        ("node-v22.0.0-darwin-arm64/LICENSE", b"license text"),
    ]);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");

    let (cas_paths, pkg_files_idx) = extract_zip_entries(
        &mut archive,
        "https://example.test/node.zip",
        store_path,
        Some("node-v22.0.0-darwin-arm64"),
        None,
    )
    .expect("happy-path zip extraction");

    dbg!(&cas_paths);
    assert!(cas_paths.contains_key("bin/node"), "prefix should be stripped");
    assert!(cas_paths.contains_key("LICENSE"), "prefix should be stripped");
    assert!(
        !cas_paths.keys().any(|k| k.starts_with("node-v22")),
        "no entry should retain the prefix",
    );
    assert_eq!(pkg_files_idx.files.len(), 2);

    drop(tempdir);
}

/// The ignore filter must see the *post-strip* path. A filter that
/// drops `LICENSE` must hit
/// after the `node-v22.0.0-darwin-arm64/` prefix has been removed —
/// otherwise the Node-runtime filter (which targets
/// `^lib/node_modules/(npm|corepack)`) would never match.
#[test]
fn extract_zip_applies_ignore_filter_on_stripped_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let bytes = build_zip(&[
        ("node-v22.0.0-darwin-arm64/bin/node", b"binary"),
        ("node-v22.0.0-darwin-arm64/lib/node_modules/npm/package.json", b"{}"),
        ("node-v22.0.0-darwin-arm64/LICENSE", b"license"),
    ]);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");

    // Filter matching the `NODE_EXTRAS_IGNORE_PATTERN` shape — strips
    // bundled npm / corepack — but compiled by hand so the test
    // doesn't pull a regex engine into pnpm-tarball.
    fn node_extras_filter(path: &str) -> bool {
        path.starts_with("lib/node_modules/npm/") || path.starts_with("lib/node_modules/corepack/")
    }

    let (cas_paths, _) = extract_zip_entries(
        &mut archive,
        "https://example.test/node.zip",
        store_path,
        Some("node-v22.0.0-darwin-arm64"),
        Some(&node_extras_filter),
    )
    .expect("zip extraction with ignore filter");

    dbg!(&cas_paths);
    assert!(cas_paths.contains_key("bin/node"));
    assert!(cas_paths.contains_key("LICENSE"));
    assert!(
        !cas_paths.contains_key("lib/node_modules/npm/package.json"),
        "ignore filter should drop bundled npm",
    );

    drop(tempdir);
}

/// A zip whose entry path contains `..` (or any other escaping
/// component) must be rejected with [`TarballError::PathTraversal`].
/// Even if a later layer would have re-anchored the write, refusing
/// the archive outright is the cheapest defense against a malicious
/// publisher.
#[test]
fn extract_zip_rejects_parent_dir_component() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let bytes = build_zip(&[("../evil.txt", b"evil")]);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");

    let err =
        extract_zip_entries(&mut archive, "https://example.test/evil.zip", store_path, None, None)
            .expect_err("escaping zip entry must be rejected, not normalized");

    match err {
        TarballError::PathTraversal { url, entry_path, reason } => {
            assert_eq!(url, "https://example.test/evil.zip");
            assert!(entry_path.contains(".."), "raw entry path should be surfaced: {entry_path}");
            assert!(!reason.is_empty());
        }
        other => panic!("expected PathTraversal, got: {other:?}"),
    }

    drop(tempdir);
}

/// Path-traversal validation must run *before* the `is_dir()`
/// early-skip — otherwise an archive carrying a malicious directory
/// entry like `../evil/` is silently dropped instead of surfacing
/// [`TarballError::PathTraversal`]. Pacquet wouldn't write that
/// directory either way (the CAS write path is gated on file
/// entries), but rejecting outright keeps the "no unsafe entry
/// accepted" contract intact for tooling that inspects the error
/// code (Caught by `CodeRabbit` on [#472](https://github.com/pnpm/pacquet/pull/472)).
#[test]
fn extract_zip_rejects_directory_entry_with_parent_component() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    // Build a zip with a single directory entry whose name contains
    // `..`. `build_zip` only writes files, so go through `ZipWriter`
    // directly here to call `add_directory`.
    let bytes = {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.add_directory("../evil", opts).expect("add dir entry");
            writer.finish().expect("finalize zip");
        }
        buf
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");

    let err = extract_zip_entries(
        &mut archive,
        "https://example.test/evil-dir.zip",
        store_path,
        None,
        None,
    )
    .expect_err("escaping directory entry must be rejected, not silently skipped");

    match err {
        TarballError::PathTraversal { url, entry_path, reason } => {
            assert_eq!(url, "https://example.test/evil-dir.zip");
            assert!(entry_path.contains(".."), "raw entry path should be surfaced: {entry_path}");
            assert!(!reason.is_empty());
        }
        other => panic!("expected PathTraversal, got: {other:?}"),
    }

    drop(tempdir);
}

#[test]
fn extract_zip_uses_entry_path_when_no_prefix() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let bytes = build_zip(&[("bin/tool", b"x"), ("README.md", b"docs")]);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");

    let (cas_paths, _) =
        extract_zip_entries(&mut archive, "https://example.test/flat.zip", store_path, None, None)
            .expect("flat zip extraction");

    dbg!(&cas_paths);
    assert!(cas_paths.contains_key("bin/tool"));
    assert!(cas_paths.contains_key("README.md"));
    assert_eq!(cas_paths.len(), 2);

    drop(tempdir);
}

/// `enclosed_name()` collapses `.` segments before we build the
/// canonical `cas_paths` key. A publisher tool that wrote
/// `pkg/./foo.txt` and `pkg/foo.txt` into the same archive must
/// land at one `foo.txt` entry after the prefix strip — same key
/// the ignore filter sees, same key downstream consumers look up.
/// Without the normalization the two would split into separate
/// `./foo.txt` / `foo.txt` rows.
#[test]
fn extract_zip_normalizes_dot_segments_in_entry_paths() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let bytes = build_zip(&[
        ("node-v22.0.0-darwin-arm64/./bin/node", b"binary"),
        ("node-v22.0.0-darwin-arm64/lib/./README", b"readme"),
    ]);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open zip");

    let (cas_paths, _) = extract_zip_entries(
        &mut archive,
        "https://example.test/dotted.zip",
        store_path,
        Some("node-v22.0.0-darwin-arm64"),
        None,
    )
    .expect("zip with `.` segments");

    dbg!(&cas_paths);
    assert!(cas_paths.contains_key("bin/node"), "`.` segment must be collapsed");
    assert!(cas_paths.contains_key("lib/README"), "`.` segment must be collapsed");
    assert!(!cas_paths.keys().any(|k| k.contains("/./")), "no entry should retain a `.` segment");

    drop(tempdir);
}

/// A source that keeps producing bytes, counting how many were taken
/// from it. `cap` is a safety net for the very regression under test: a
/// caller that forgets to bound its read gets an error instead of an
/// endless loop, and the byte count says what happened.
struct EndlessReader {
    bytes_read: u64,
    cap: u64,
}

impl Read for EndlessReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes_read >= self.cap {
            return Err(std::io::Error::other("test reader ran past its safety cap"));
        }
        let take = buf.len().min(usize::try_from(self.cap - self.bytes_read).unwrap_or(usize::MAX));
        buf[..take].fill(b'x');
        self.bytes_read += take as u64;
        Ok(take)
    }
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

/// The counterpart of the rejection above: a truthful entry past the
/// buffering ceiling streams into the CAS in full, so bounding the read
/// costs a runtime archive's biggest member nothing.
#[test]
fn write_zip_entry_to_cas_streams_a_truthful_oversized_entry() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let declared_size = STREAM_ENTRY_BUFFER_MAX + 1;
    let mut payload = std::io::repeat(b'x').take(declared_size);
    let (file_path, _, size) = write_zip_entry_to_cas(
        &mut payload,
        declared_size,
        "https://example.test/runtime.zip",
        "bin/node",
        store_path,
        true,
    )
    .expect("an oversized entry must stream into the store");

    assert_eq!(size, declared_size);
    assert_eq!(
        std::fs::metadata(&file_path).expect("stat the streamed entry").len(),
        declared_size,
    );
    assert!(
        file_path.to_string_lossy().ends_with("-exec"),
        "executable entries must keep the -exec CAS suffix on the streaming branch",
    );

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

    let err = DownloadTarballToStore {
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
        append_manifest: None,
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

    let cas_paths = DownloadTarballToStore {
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
        append_manifest: None,
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

/// Pacquet's [`normalize_bundled_manifest`] picks the subset of
/// `package.json` fields downstream install code reads (bin lookup,
/// peer extraction, build-script detection) and narrows `scripts` to
/// the three lifecycle hooks. Two cases are intentionally NOT covered:
/// `semver.clean` normalization (pacquet keeps version verbatim, per
/// the function's doc comment) and the missing-version default of
/// `0.0.0` (pacquet leaves the field absent rather than synthesizing
/// one).
mod normalize_bundled_manifest_tests {
    use super::normalize_bundled_manifest;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn returns_none_for_empty_manifest() {
        assert_eq!(normalize_bundled_manifest(&json!({})), None);
    }

    #[test]
    fn returns_none_for_non_object() {
        assert_eq!(normalize_bundled_manifest(&json!("not an object")), None);
        assert_eq!(normalize_bundled_manifest(&json!(null)), None);
        assert_eq!(normalize_bundled_manifest(&json!(42)), None);
    }

    #[test]
    fn returns_none_when_manifest_has_only_excluded_fields() {
        assert_eq!(
            normalize_bundled_manifest(&json!({
                "description": "a package",
                "keywords": ["test"],
                "license": "MIT",
                "author": "test",
                "repository": "test/test",
            })),
            None,
        );
    }

    #[test]
    fn picks_included_fields_and_excludes_others() {
        let result = normalize_bundled_manifest(&json!({
            "name": "foo",
            "version": "1.0.0",
            "description": "should be excluded",
            "license": "MIT",
            "bin": { "foo": "./bin/foo.js" },
            "engines": { "node": ">=18" },
            "cpu": ["x64"],
            "os": ["linux"],
            "libc": ["glibc"],
            "dependencies": { "bar": "^1.0.0" },
            "devDependencies": { "qux": "^3.0.0" },
            "optionalDependencies": { "baz": "^2.0.0" },
            "peerDependencies": { "react": "^18" },
            "peerDependenciesMeta": { "react": { "optional": true } },
            "bundledDependencies": ["bar"],
            "directories": { "bin": "./bin" },
        }))
        .expect("non-empty pick");
        let map = result.as_object().expect("object");
        assert_eq!(map.get("name").and_then(|v| v.as_str()), Some("foo"));
        assert_eq!(map.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
        assert_eq!(map.get("bin"), Some(&json!({ "foo": "./bin/foo.js" })));
        assert_eq!(map.get("engines"), Some(&json!({ "node": ">=18" })));
        assert_eq!(map.get("cpu"), Some(&json!(["x64"])));
        assert_eq!(map.get("os"), Some(&json!(["linux"])));
        assert_eq!(map.get("libc"), Some(&json!(["glibc"])));
        assert_eq!(map.get("dependencies"), Some(&json!({ "bar": "^1.0.0" })));
        assert_eq!(map.get("devDependencies"), Some(&json!({ "qux": "^3.0.0" })));
        assert_eq!(map.get("optionalDependencies"), Some(&json!({ "baz": "^2.0.0" })));
        assert_eq!(map.get("peerDependencies"), Some(&json!({ "react": "^18" })));
        assert_eq!(
            map.get("peerDependenciesMeta"),
            Some(&json!({ "react": { "optional": true } })),
        );
        assert_eq!(map.get("bundledDependencies"), Some(&json!(["bar"])));
        assert_eq!(map.get("directories"), Some(&json!({ "bin": "./bin" })));
        // Excluded fields stay out.
        assert!(map.get("description").is_none());
        assert!(map.get("license").is_none());
        assert!(map.get("keywords").is_none());
    }

    #[test]
    fn only_picks_lifecycle_scripts_not_all_scripts() {
        let result = normalize_bundled_manifest(&json!({
            "name": "foo",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "echo pre",
                "install": "echo install",
                "postinstall": "echo post",
                "test": "jest",
                "build": "tsc",
                "start": "node index.js",
                "prepare": "tsc",
            },
        }))
        .expect("non-empty pick");
        assert_eq!(
            result.get("scripts").expect("scripts present"),
            &json!({
                "preinstall": "echo pre",
                "install": "echo install",
                "postinstall": "echo post",
            }),
        );
    }

    #[test]
    fn omits_scripts_key_when_no_lifecycle_scripts_exist() {
        let result = normalize_bundled_manifest(&json!({
            "name": "foo",
            "version": "1.0.0",
            "scripts": {
                "test": "jest",
                "build": "tsc",
            },
        }))
        .expect("non-empty pick");
        assert!(
            result.get("scripts").is_none(),
            "scripts key must be absent when no lifecycle hook is present",
        );
    }

    /// `null` and `undefined` fields are skipped. Rust's
    /// [`serde_json::Value`] has no `undefined`, but JSON `null`
    /// reaches the picker as [`serde_json::Value::Null`] and must be
    /// filtered out the same way.
    #[test]
    fn skips_null_fields() {
        let result = normalize_bundled_manifest(&json!({
            "name": "foo",
            "version": "1.0.0",
            "bin": null,
            "engines": null,
        }))
        .expect("non-empty pick");
        assert!(result.get("bin").is_none(), "null `bin` must be dropped");
        assert!(result.get("engines").is_none(), "null `engines` must be dropped");
        assert_eq!(result.get("name").and_then(|v| v.as_str()), Some("foo"));
        assert_eq!(result.get("version").and_then(|v| v.as_str()), Some("1.0.0"));
    }

    /// The bundled manifest is downstream-fed into
    /// `extract_peer_dependencies` and `extract_children`; dropping
    /// `peerDependenciesMeta` or `optionalDependencies` here would
    /// replicate the pnpm/pnpm#11934 resolver-side bug on the
    /// install-side. Pin the keys explicitly.
    #[test]
    fn preserves_optional_dependencies_and_peer_dependencies_meta_keys() {
        let result = normalize_bundled_manifest(&json!({
            "name": "consumer",
            "version": "1.0.0",
            "optionalDependencies": { "sharp": "^0.34.0" },
            "peerDependenciesMeta": {
                "@vercel/kv": { "optional": true },
                "ioredis": { "optional": true },
            },
        }))
        .expect("non-empty pick");
        assert_eq!(result.get("optionalDependencies"), Some(&json!({ "sharp": "^0.34.0" })));
        assert_eq!(
            result.get("peerDependenciesMeta"),
            Some(&json!({
                "@vercel/kv": { "optional": true },
                "ioredis": { "optional": true },
            })),
        );
    }
}

/// Saturated `dist` stats must not collide with the latency- or
/// background-class sentinels — a hostile registry publishing absurd
/// sizes would otherwise reclassify its downloads as metadata.
#[test]
fn download_priority_never_reaches_the_class_sentinels() {
    let priority = download_priority(Some(usize::MAX), Some(usize::MAX));
    assert!(priority < pnpm_network::BACKGROUND);
    assert!(priority < UNPRIORITIZED);
    assert_eq!(priority, MAX_THROUGHPUT_PRIORITY);
}

/// A runtime archive (Node.js / Bun / Deno) ships no `package.json`, so
/// `apply_append_manifest` must bake the synthesized manifest into the
/// persisted store-index row — both its `files` map and its bundled
/// `manifest` — and into this install's `cas_paths`. Without the row
/// entry, a later *warm* materialization reads a `package.json`-less row
/// and `pnpm dlx node@runtime:<v>` fails with `dlx_read_manifest`.
#[test]
fn apply_append_manifest_folds_the_synthesized_manifest_into_the_row() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let manifest_bytes =
        br#"{"name":"node","version":"26.4.0","bin":{"node":"bin/node"}}"#.to_vec();
    let mut cas_paths = HashMap::new();
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };

    apply_append_manifest(store_path, &manifest_bytes, &mut cas_paths, &mut idx)
        .expect("write the synthesized manifest into the CAS");

    // This install's slot materializes the manifest...
    assert!(cas_paths.contains_key("package.json"), "cas_paths gains package.json");
    // ...and so does the persisted row, so warm reinstalls get it too.
    let file = idx.files.get("package.json").expect("row records the package.json file");
    assert_eq!(file.size, manifest_bytes.len() as u64);
    assert!(!file.digest.is_empty(), "the synthesized file is content-addressed");
    // The bundled manifest carries the runtime's bin so the warm-batch
    // bin linker links it without stat-ing the slot's package.json.
    let manifest = idx.manifest.expect("row records the bundled manifest");
    assert_eq!(manifest.get("bin"), Some(&serde_json::json!({ "node": "bin/node" })));
}

/// An ordinary npm tarball already carries its own `package.json`;
/// `apply_append_manifest` must leave it untouched (pnpm's `manifest ==
/// null` guard) rather than displacing the real manifest with a
/// synthesized one.
#[test]
fn apply_append_manifest_is_a_noop_when_the_archive_ships_a_package_json() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let existing =
        CafsFileInfo { digest: "kept".to_string(), mode: 0o644, size: 3, checked_at: None };
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };
    idx.files.insert("package.json".to_string(), existing);
    let mut cas_paths = HashMap::new();

    apply_append_manifest(store_path, br#"{"name":"node"}"#, &mut cas_paths, &mut idx)
        .expect("a no-op still returns Ok");

    assert!(cas_paths.is_empty(), "the real package.json is not overwritten in cas_paths");
    assert_eq!(idx.files["package.json"].digest, "kept", "the row's real file entry is kept");
    assert!(idx.manifest.is_none(), "the archive's manifest handling is left alone");
}

/// The placeholder is a completion marker, not the package's identity,
/// so it must not become the row's bundled `manifest` the way
/// `apply_append_manifest`'s real one does — see
/// <https://github.com/pnpm/pnpm/issues/13410>.
#[test]
fn apply_placeholder_manifest_marks_an_archive_that_ships_no_package_json() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let mut cas_paths = HashMap::new();
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };

    apply_placeholder_manifest(store_path, &mut cas_paths, &mut idx)
        .expect("write the placeholder into the CAS");

    assert!(cas_paths.contains_key("package.json"), "cas_paths gains package.json");
    let file = idx.files.get("package.json").expect("row records the placeholder file");
    assert!(!file.digest.is_empty(), "the placeholder is content-addressed");
    let written =
        std::fs::read_to_string(&cas_paths["package.json"]).expect("read the placeholder");
    assert!(written.contains("_pnpmPlaceholder"), "got {written}");
    assert!(idx.manifest.is_none(), "a placeholder is not the package's bundled manifest");
}

#[test]
fn apply_placeholder_manifest_is_a_noop_when_a_package_json_is_already_recorded() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let existing =
        CafsFileInfo { digest: "kept".to_string(), mode: 0o644, size: 3, checked_at: None };
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };
    idx.files.insert("package.json".to_string(), existing);
    let mut cas_paths = HashMap::new();

    apply_placeholder_manifest(store_path, &mut cas_paths, &mut idx)
        .expect("a no-op still returns Ok");

    assert!(cas_paths.is_empty(), "the real package.json is not overwritten in cas_paths");
    assert_eq!(idx.files["package.json"].digest, "kept", "the row's real file entry is kept");
}

/// Entry keys are joined with `/` on every platform. The store's
/// `index.db` is shared with pnpm, whose path layer is string-based and
/// always forward-slashed, so a `PathBuf`-joined key would write
/// `bin\tool` on Windows and desynchronize the two implementations.
#[test]
fn extract_joins_nested_entry_paths_with_forward_slashes() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path("package/bin/nested/tool.js").expect("set entry path");
        header.set_cksum();
        builder.append(&header, &b"hi\n"[..]).expect("append entry");
        builder.finish().expect("finalize tar");
    }

    let (cas_paths, _) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    assert!(
        cas_paths.contains_key("bin/nested/tool.js"),
        "nested entries must be keyed with `/`, got {:?}",
        cas_paths.keys().collect::<Vec<_>>(),
    );
    assert!(
        !cas_paths.keys().any(|key| key.contains('\\')),
        "no key may carry a platform separator, got {:?}",
        cas_paths.keys().collect::<Vec<_>>(),
    );

    drop(tempdir);
}

/// Build a gzipped tar whose *compressed* body is at least `min_bytes`,
/// so the response carries a `Content-Length` over the in-progress
/// threshold. The payload is LCG noise because gzip would otherwise
/// collapse a compressible body well under the threshold.
fn incompressible_tarball(min_bytes: usize) -> Vec<u8> {
    let mut payload = vec![0_u8; min_bytes + (1 << 16)];
    let mut state: u32 = 0x1234_5678;
    for byte in &mut payload {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *byte = (state >> 24) as u8;
    }

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path("package/noise.bin").expect("set entry path");
        header.set_cksum();
        builder.append(&header, payload.as_slice()).expect("append entry");
        builder.finish().expect("finalize tar");
    }

    let mut gz = Vec::new();
    {
        use std::io::Write as _;
        let mut encoder = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::fast());
        encoder.write_all(&tar_bytes).expect("gzip the tar");
        encoder.finish().expect("finish gzip");
    }
    gz
}

/// `in_progress` fires only for tarballs at or above `BIG_TARBALL_SIZE`.
/// pnpm's reporter renders a percent gauge from these, and per-byte
/// events for the typical sub-megabyte package flood the consumer with
/// values that reach 100% before any UI tick can show them.
#[tokio::test]
async fn in_progress_events_fire_only_for_big_tarballs() {
    use std::sync::Mutex;

    use pnpm_reporter::{FetchingProgressMessage, LogEvent};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pnpm_reporter::Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    fn in_progress_count() -> usize {
        EVENTS
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    LogEvent::FetchingProgress(log)
                        if matches!(log.message, FetchingProgressMessage::InProgress { .. }),
                )
            })
            .count()
    }

    fn last_in_progress_bytes() -> Option<u64> {
        EVENTS.lock().unwrap().iter().rev().find_map(|event| match event {
            LogEvent::FetchingProgress(log) => match log.message {
                FetchingProgressMessage::InProgress { downloaded, .. } => Some(downloaded),
                FetchingProgressMessage::Started { .. } => None,
            },
            _ => None,
        })
    }

    async fn download_body<Reporter: pnpm_reporter::Reporter>(
        body: Vec<u8>,
        store_path: &'static pnpm_store_dir::StoreDir,
    ) {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/pkg.tgz")
            .with_status(200)
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        let url = format!("{}/pkg.tgz", server.url());

        fetch_and_extract_with_retry::<Reporter>(
            &ThrottledClient::default(),
            &url,
            None,
            None,
            0,
            "noise@1.0.0",
            "",
            store_path,
            fast_retry_opts(),
            &AuthHeaders::default(),
            None,
            None,
            false,
        )
        .await
        .expect("the download should succeed");

        mock.assert_async().await;
    }

    let (store_dir_keep, store_path) = tempdir_with_leaked_path();

    let big = incompressible_tarball(6 * 1024 * 1024);
    let big_len = big.len() as u64;
    EVENTS.lock().unwrap().clear();
    download_body::<RecordingReporter>(big, store_path).await;
    assert!(in_progress_count() > 0, "a tarball over the threshold must report download progress");
    // Trailing edge: the last event carries the true total rather than
    // whatever the final throttle window happened to observe.
    assert_eq!(
        last_in_progress_bytes(),
        Some(big_len),
        "the final progress event must report the whole body",
    );

    EVENTS.lock().unwrap().clear();
    download_body::<RecordingReporter>(incompressible_tarball(16 * 1024), store_path).await;
    assert_eq!(
        in_progress_count(),
        0,
        "a tarball under the threshold must not report per-chunk progress",
    );

    drop(store_dir_keep);
}

#[tokio::test]
async fn streaming_download_extracts_a_big_pinned_tarball() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let body = incompressible_tarball(5 * 1024 * 1024);
    let pinned = Integrity::from(&body);

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(body.clone())
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());

    let (verified, cas_paths, files_idx) = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&pinned),
        None,
        0,
        "noise@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect("a well-formed pinned tarball must download and extract");
    mock.assert_async().await;

    assert_eq!(verified.to_string(), pinned.to_string(), "the pinned integrity is what verifies");

    let (reference_keep, reference_store) = tempdir_with_leaked_path();
    let (reference_paths, reference_idx) =
        stream_extract_gzipped_tarball(&body, reference_store, None)
            .expect("the reference extraction of the same bytes must succeed");
    assert_eq!(
        cas_paths.keys().collect::<std::collections::BTreeSet<_>>(),
        reference_paths.keys().collect::<std::collections::BTreeSet<_>>(),
        "the streaming path must materialize the same entries",
    );
    // `checked_at` is stamped at write time; everything else must match.
    let strip_checked_at = |mut idx: PackageFilesIndex| {
        for info in idx.files.values_mut() {
            info.checked_at = None;
        }
        idx
    };
    assert_eq!(
        strip_checked_at(files_idx),
        strip_checked_at(reference_idx),
        "the streaming path must index the same file hashes",
    );

    drop(reference_keep);
    drop(store_dir_keep);
}

/// A chunked response advertises no length, so nothing decides up front
/// that its body is large — the buffered path has to notice while it
/// runs. Past the point where the archive would be extracted as a stream
/// anyway, it is, and the download completes as it would have with a
/// `Content-Length`.
#[tokio::test]
async fn chunked_download_extracts_a_body_past_the_buffering_threshold() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let body = incompressible_tarball(STREAM_EXTRACT_COMPRESSED_THRESHOLD);
    assert!(
        body.len() > STREAM_EXTRACT_COMPRESSED_THRESHOLD,
        "the body must outgrow the buffering threshold to exercise the handover",
    );
    let pinned = Integrity::from(&body);

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_chunked_body(move |writer| writer.write_all(&body))
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());

    let (verified, cas_paths, _) = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&pinned),
        None,
        0,
        "noise@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect("a chunked body must download and extract");
    mock.assert_async().await;

    assert_eq!(verified.to_string(), pinned.to_string());
    assert!(cas_paths.contains_key("noise.bin"), "got {:?}", cas_paths.keys().collect::<Vec<_>>());
    drop(store_dir_keep);
}

/// A body that never was an archive still has to be read to its end
/// before it can be judged: whether it hashes to the pinned integrity is
/// what decides between "someone tampered with this download" and "this
/// package is not a gzip stream". Dropping the bytes instead of keeping
/// them must not change which of the two is reported.
#[tokio::test]
async fn oversized_non_gzip_body_reports_the_integrity_verdict_first() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let body = vec![b'n'; STREAM_EXTRACT_COMPRESSED_THRESHOLD + (1 << 16)];
    let matching = Integrity::from(&body);
    let wrong = Integrity::from(b"the body the registry was supposed to serve");

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(body)
        .expect(6)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());

    async fn fetch_err(
        url: &str,
        pinned: &Integrity,
        store_path: &'static StoreDir,
    ) -> TarballError {
        fetch_and_extract_with_retry::<SilentReporter>(
            &ThrottledClient::default(),
            url,
            Some(pinned),
            None,
            0,
            "noise@1.0.0",
            "",
            store_path,
            fast_retry_opts(),
            &AuthHeaders::default(),
            None,
            None,
            false,
        )
        .await
        .expect_err("a body that is not an archive must fail")
    }

    let err = fetch_err(&url, &wrong, store_path).await;
    assert!(
        matches!(err, TarballError::Checksum(_)),
        "a body that does not hash to the pinned integrity must report that, got {err:?}",
    );

    let err = fetch_err(&url, &matching, store_path).await;
    assert!(
        matches!(err, TarballError::DecodeGzip(_)),
        "a body that hashes correctly but is not gzip must report the decode failure, got {err:?}",
    );

    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn streaming_download_integrity_mismatch_retries_and_fails() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let body = incompressible_tarball(5 * 1024 * 1024);
    let wrong = Integrity::from(b"not the body being served");

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(body)
        .expect(3)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&wrong),
        None,
        0,
        "noise@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("an integrity mismatch must exhaust the retry budget");
    assert!(matches!(err, TarballError::Checksum(_)), "expected Checksum error, got {err:?}");
    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn streaming_download_corrupt_archive_retries_and_fails() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut body = vec![0x1f_u8, 0x8b];
    body.extend(std::iter::repeat_n(0xa5_u8, 5 * 1024 * 1024));
    // The integrity pins the garbage itself, so only the archive
    // decode can produce the failure under test.
    let pinned = Integrity::from(&body);

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(body)
        .expect(3)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&pinned),
        None,
        0,
        "noise@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("a corrupt archive must exhaust the retry budget");
    assert!(
        matches!(err, TarballError::DecodeGzip(_)),
        "the exhausting attempt is buffered, so the eager decode diagnostic surfaces, got {err:?}",
    );
    mock.assert_async().await;
    drop(store_dir_keep);
}

#[tokio::test]
async fn streaming_download_tampered_and_corrupt_body_reports_integrity() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut body = vec![0x1f_u8, 0x8b];
    body.extend(std::iter::repeat_n(0x5a_u8, 5 * 1024 * 1024));
    let wrong = Integrity::from(b"the body the registry was supposed to serve");

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(body)
        .expect(3)
        .create_async()
        .await;
    let url = format!("{}/pkg.tgz", server.url());

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        Some(&wrong),
        None,
        0,
        "noise@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("a tampered body must exhaust the retry budget");
    assert!(
        matches!(err, TarballError::Checksum(_)),
        "the integrity verdict must outrank the decode failure, got {err:?}",
    );
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// Only regular files reach the CAFS. A symlink's zero-byte body would
/// otherwise be stored as though it were the file it points at.
#[test]
fn extract_keeps_only_regular_file_entries() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);

        let mut file = tar::Header::new_gnu();
        file.set_size(3);
        file.set_mode(0o644);
        file.set_entry_type(tar::EntryType::Regular);
        file.set_path("package/real.txt").expect("set file path");
        file.set_cksum();
        builder.append(&file, &b"hi\n"[..]).expect("append file");

        let mut dir = tar::Header::new_gnu();
        dir.set_size(0);
        dir.set_mode(0o755);
        dir.set_entry_type(tar::EntryType::Directory);
        dir.set_path("package/sub/").expect("set dir path");
        dir.set_cksum();
        builder.append(&dir, std::io::empty()).expect("append dir");

        let mut link = tar::Header::new_gnu();
        link.set_size(0);
        link.set_mode(0o777);
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_path("package/link.txt").expect("set link path");
        link.set_link_name("real.txt").expect("set link target");
        link.set_cksum();
        builder.append(&link, std::io::empty()).expect("append symlink");

        builder.finish().expect("finalize tar");
    }

    let (cas_paths, _) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    assert_eq!(
        cas_paths.keys().collect::<Vec<_>>(),
        vec!["real.txt"],
        "only the regular file may be stored",
    );

    drop(tempdir);
}

/// Build a tar carrying one entry whose raw header name is `name`,
/// bypassing `set_path`'s own validation so hostile names can be tested.
fn tar_with_raw_entry_name(name: &[u8]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    let mut builder = tar::Builder::new(&mut tar_bytes);
    let mut header = tar::Header::new_gnu();
    header.set_size(5);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    let raw = header.as_mut_bytes();
    raw[..name.len()].copy_from_slice(name);
    for byte in &mut raw[name.len()..100] {
        *byte = 0;
    }
    header.set_cksum();
    builder.append(&header, &b"bytes"[..]).expect("append entry");
    builder.finish().expect("finalize tar");
    drop(builder);
    tar_bytes
}

#[test]
fn extract_strips_only_one_component_from_a_dot_prefixed_entry_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_raw_entry_name(b"./package/package.json");
    let (cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    assert_eq!(cas_paths.keys().collect::<Vec<_>>(), vec!["package/package.json"]);
    assert_eq!(pkg_files_idx.files.keys().collect::<Vec<_>>(), vec!["package/package.json"]);
    assert!(pkg_files_idx.manifest.is_none());

    drop(tempdir);
}

/// A backslash is an ordinary filename character on Unix but a
/// separator on Windows, and these keys travel between the two through
/// the shared `index.db`. pnpm folds `\` to `/` before validating
/// (`parseTarball.ts`), so a traversal spelled with backslashes has to
/// be caught here too rather than stored verbatim.
#[test]
fn extract_rejects_backslash_traversal_in_entry_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_raw_entry_name(br"package/..\..\evil.txt");
    let err = extract_tarball_entries(&tar_bytes, store_path, None)
        .expect_err("a backslash-spelled traversal must be rejected");

    match err {
        TarballError::ReadTarballEntries(io_err) => {
            assert_eq!(io_err.kind(), std::io::ErrorKind::InvalidData);
        }
        other => panic!("expected a rejected tar entry, got {other:?}"),
    }

    drop(tempdir);
}

/// Folding `\` to `/` must not reject the benign case: an archive built
/// by Windows tooling that spells a nested path with backslashes
/// installs under pnpm, so it installs here, under the same key.
#[test]
fn extract_reads_a_windows_separator_entry_as_a_nested_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_raw_entry_name(br"package/bin\tool.js");
    let (cas_paths, _) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    assert!(
        cas_paths.contains_key("bin/tool.js"),
        "a backslash separator must resolve to the same key as `/`, got {:?}",
        cas_paths.keys().collect::<Vec<_>>(),
    );

    drop(tempdir);
}

/// A tarball URL can carry inline `user:pass@` credentials — typed on the
/// command line for `pnpm add <url>`, or declared in a manifest — and every
/// error rendering the URL lands in terminal scrollback and CI logs.
#[test]
fn url_bearing_errors_redact_inline_credentials() {
    let url = "https://alice:hunter2@example.com/pkg.tgz".to_string();
    let rendered = [
        TarballError::HttpStatus(HttpStatusError { url: url.clone(), status: 404 }).to_string(),
        TarballError::TarballTooLarge { url: url.clone(), advertised_size: u64::MAX }.to_string(),
        TarballError::SiblingFetchFailed { url: url.clone() }.to_string(),
        TarballError::OffAllowlist { url }.to_string(),
    ];
    for message in rendered {
        eprintln!("MESSAGE: {message}");
        assert!(!message.contains("hunter2"), "the password must not be rendered: {message}");
        assert!(message.contains("example.com/pkg.tgz"), "the host must survive: {message}");
    }
}
