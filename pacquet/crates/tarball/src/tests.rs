use super::{
    DownloadTarballToStore, HttpStatusError, MemCache, NetworkError, PrefetchedCasPaths, RetryOpts,
    SharedReportedProgressKeys, TarballError, VerifyChecksumError, allocate_tarball_buffer,
    download_priority, extract_tarball_entries, extract_zip_entries, fetch_and_extract_with_retry,
    is_transient_error, normalize_bundled_manifest, prefetch_cas_paths,
};
use pacquet_network::{AuthHeaders, ThrottledClient, UNPRIORITIZED};
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
    StoreIndexWriter, store_index_key,
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::{collections::HashMap, io::Cursor, path::PathBuf, sync::Arc, time::Duration};
use tempfile::{TempDir, tempdir};

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
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
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(1))
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .expect("build reqwest client");
    ThrottledClient::from_client(client)
}

/// Pin `walk_reqwest_chain`'s contract: a `NetworkError` formed
/// from a real reqwest connect failure must surface the leaf
/// reason (e.g. `Connection refused`) appended to the wrapper
/// message, not stop at reqwest's `error sending request for url
/// (URL)`. Without the helper, the user sees only the wrapper —
/// which is what triggered the original "what's actually failing?"
/// debugging round on this branch.
///
/// Uses `127.0.0.1:1` (port 1 is reserved; connect always fails
/// with a deterministic ECONNREFUSED on every host I've tried)
/// and `fast_fail_client`'s 1 s bounds, so the test stays
/// hermetic and quick.
#[tokio::test]
async fn network_error_display_includes_reqwest_inner_chain() {
    let url = "http://127.0.0.1:1/whatever";
    let client = fast_fail_client();
    let err =
        client.acquire().await.get(url).send().await.expect_err("connecting to port 1 must fail");
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
}

/// Default `RetryOpts` for unit tests. We don't want the suite to
/// sit through pnpm's 10 s + 60 s production backoff just to assert
/// that an unreachable URL eventually fails — every test that
/// exercises a network call here either short-circuits to a cache
/// hit or expects the failure path. `retries: 0` keeps the failure
/// path deterministic and bounded by `fast_fail_client`'s 1 s
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
        package_integrity: &integrity("sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="),
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
        package_integrity: &pkg_integrity,
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
        package_integrity: &integrity("sha512-aaaan1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w=="),
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };

    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    // A cache hit also emits package-status progress, so it records the
    // key to prevent a later warm/cold pass from counting the same
    // package status again.
    let progress_reported = SharedReportedProgressKeys::default();
    let cas_paths = DownloadTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        package_integrity: &pkg_integrity,
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
    }
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
        package_integrity: &pkg_integrity,
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        package_integrity: &pkg_integrity,
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

/// A corrupt row whose digest is empty (or too short / non-hex) used
/// to panic inside `StoreDir::file_path_by_hex_str` (`hex[..2]`). The
/// validation in `cas_file_path_by_mode` now rejects such rows, and
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
        // Empty digest — pre-fix this would panic in the spawn_blocking
        // task during `hex[..2]`.
        CafsFileInfo { digest: String::new(), mode: 0o644, size: 0, checked_at: None },
    );
    let entry = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        package_integrity: &pkg_integrity,
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        package_integrity: &pkg_integrity,
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
        algo: "sha512".to_string(),
        files,
        side_effects: None,
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
        package_integrity: &pkg_integrity,
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

/// The per-entry loop used to be a pile of `.unwrap()` /
/// `.expect()` calls that turned any tar-side failure — corrupt
/// header, short body read, path decode — into a panic inside a
/// blocking-pool task (which took the whole install with it and
/// occasionally left the pool with dangling permits). The loop now
/// lives in `extract_tarball_entries` and propagates every such
/// failure as [`TarballError::ReadTarballEntries`]. This test
/// feeds the function bytes that aren't a valid tar archive and
/// asserts we get that error rather than a panic.
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
/// per upstream's `NODE_EXTRAS_IGNORE_PATTERN`); without coverage
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

    drop(tempdir);
}

/// `RetryOpts::default()` reproduces pnpm's
/// `network/fetch/src/fetch.ts` defaults: 2 retries, factor 10,
/// minTimeout 10 s, maxTimeout 60 s. The first post-failure delay
/// is `minTimeout`; subsequent delays multiply by `factor` until
/// they hit `maxTimeout`.
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

/// pnpm's
/// [`remoteTarballFetcher.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/fetching/tarball-fetcher/src/remoteTarballFetcher.ts#L76-L84)
/// rejects only HTTP 401, 403, 404 (and the git-prepare error code,
/// which doesn't apply to registry tarballs). Every other failure
/// — arbitrary 4xx, 5xx, network reset, integrity mismatch, gzip
/// or tar parse error — falls through to `op.retry(error)` and is
/// retried. Diverging here was the original bug behind [#259].
///
/// [#259]: https://github.com/pnpm/pacquet/issues/259
#[test]
fn retry_classification_matches_pnpm_policy() {
    let url = "https://example.test/pkg.tgz".to_string();
    let mk_http =
        |status: u16| TarballError::HttpStatus(HttpStatusError { url: url.clone(), status });

    // Fail-fast set — exactly the three codes pnpm short-circuits on.
    for code in [401u16, 403, 404] {
        assert!(!is_transient_error(&mk_http(code)), "HTTP {code} should fail fast");
    }
    // Everything else, including arbitrary 4xx that pnpm does not
    // single out, must retry.
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

/// First request returns 503 (transient per pnpm's policy), the
/// retry returns 200 with the real fastify-error tarball. The
/// retry loop must drive the full pipeline — network → integrity
/// → extract — to completion on the second attempt, which is the
/// core fix for [#259](https://github.com/pnpm/pacquet/issues/259).
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
    )
    .await
    .expect("transient 503 should be followed by a successful retry");

    // Sanity-check: extraction actually populated the cas-paths map.
    assert!(cas_paths.contains_key("package.json"));
    fail.assert_async().await;
    ok.assert_async().await;
    drop(store_dir_keep);
}

/// pnpm's tarball fetcher retries integrity mismatches by re-running
/// the full `addFilesFromTarball` closure on the next attempt. With
/// a body that never matches the integrity hash, the loop must
/// retry until the budget is exhausted and then surface a
/// `Checksum` error — not fail fast on the first mismatch.
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
    )
    .await
    .expect_err("integrity mismatch should exhaust the retry budget");
    assert!(matches!(err, TarballError::Checksum(_)), "expected Checksum error, got {err:?}");
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

/// pnpm retries arbitrary 4xx codes that aren't 401/403/404 (any
/// `FetchError` throws to the outer catch, which only short-circuits
/// on the explicit no-retry set). 410 Gone is the canonical example
/// — semantically permanent but pnpm still hits it `retries+1` times.
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

/// Persistent 5xx must stop after `retries + 1` total tries. Pairs
/// with `retries_then_succeeds_on_transient_5xx` to bracket both
/// success and exhaustion paths.
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

/// Regression test for the `run_with_mem_cache` deadlock that hung
/// `pacquet install` on real-network workloads at high concurrency.
/// The if-let branch used to hold a `DashMap::Ref` (a synchronous
/// shard read guard) across two `.await` points; under enough
/// concurrency another task on the same worker would call
/// `mem_cache.insert` for a key hashing to the same shard, block
/// on the `parking_lot` write, and starve every worker.
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
                    package_integrity: pkg_integrity,
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
    let auth_headers = AuthHeaders::from_creds_map(
        [(pacquet_network::nerf_dart(&url), "Bearer test-token".to_owned())],
        None,
    );

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
    )
    .await
    .expect("server should accept the request once the bearer header is attached");

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
    let auth_headers = AuthHeaders::from_creds_map(
        [(pacquet_network::nerf_dart(&url), "Bearer test-token".to_owned())],
        None,
    );

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

    use pacquet_reporter::{LogEvent, ProgressMessage};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pacquet_reporter::Reporter for RecordingReporter {
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
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: &pkg_integrity,
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
    }
    .run_with_mem_cache::<pacquet_reporter::SilentReporter>(&mem_cache)
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
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: &pkg_integrity,
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

    use pacquet_reporter::{LogEvent, ProgressMessage};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pacquet_reporter::Reporter for RecordingReporter {
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
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: &pkg_integrity,
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
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: &pkg_integrity,
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
/// errors. Before the `CacheValue::Failed` fix, the failing task
/// returned without flipping the cache slot to `Available` or
/// notifying waiters, so the second requester would park on
/// `Notify::notified` forever. Now the owner sets `Failed`, removes
/// the entry from `mem_cache`, and notifies waiters; both requesters
/// surface a `TarballError`.
///
/// Two concurrent `run_with_mem_cache` calls for the same URL,
/// pointing at a 404 endpoint with `retries: 0` so the failure is
/// fast. With a 30 s wall-clock cap, the test asserts the deadlock
/// regression by demanding both calls complete (rather than hanging
/// the whole runtime).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_with_mem_cache_recovers_from_owning_fetch_error() {
    use pacquet_reporter::SilentReporter;

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
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: pkg_integrity,
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
    };

    // Drive both calls concurrently. Pre-fix: the first to hit the
    // `else` branch goes through the network, fails, returns
    // without notifying — the second parks on `Notify` forever.
    // Post-fix: the owner notifies after setting `Failed`; the
    // waiter wakes up, observes `Failed`, and surfaces
    // `SiblingFetchFailed` (or its own attempt's error).
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

    use pacquet_reporter::{FetchingProgressMessage, LogEvent, ProgressMessage, Reporter as _};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pacquet_reporter::Reporter for RecordingReporter {
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
    // `started` events must carry a populated `size`. Pinning this
    // here so the previous regression — emit-before-send leaving
    // `size` always-`null` — can't sneak back in (Copilot review on
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

    use pacquet_reporter::{FetchingProgressMessage, LogEvent};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pacquet_reporter::Reporter for RecordingReporter {
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

    use pacquet_reporter::{LogEvent, ProgressMessage};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pacquet_reporter::Reporter for RecordingReporter {
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
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: &pkg_integrity,
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
        pacquet_store_dir::StoreIndex::shared_readonly_in(store_path)
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
        verified_files_cache: SharedVerifiedFilesCache::clone(&verified_files_cache),
        package_integrity: &pkg_integrity,
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

    use pacquet_reporter::{LogEvent, RequestRetryLog};

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl pacquet_reporter::Reporter for RecordingReporter {
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

/// Happy path: a zip with two file entries under a top-level
/// `node-vX.Y.Z-darwin-arm64/` directory is extracted with the
/// prefix stripped from each `cas_paths` key. Mirrors upstream's
/// `basenamePrefix` strip — the install dispatcher will later
/// resolve `bin/node` against `cas_paths` and that lookup must
/// hit the stripped form, not the prefixed form.
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

/// The ignore filter must see the *post-strip* path, the same one
/// upstream's regex sees. A filter that drops `LICENSE` must hit
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

    // Filter mirroring the upstream `NODE_EXTRAS_IGNORE_PATTERN`
    // shape — strips bundled npm / corepack — but compiled by hand
    // so the test doesn't pull a regex engine into pacquet-tarball.
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
/// Mirrors upstream's `validatePathSecurity` rejection — even if a
/// later layer would have re-anchored the write, refusing the
/// archive outright is the cheapest defense against a malicious
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

/// `archive_prefix: None` keeps entry paths verbatim — same as
/// upstream's `basename === ''` branch in `extractZipToTarget`.
/// A zip without a top-level wrapper directory must round-trip
/// each entry's path into `cas_paths` as-is.
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

/// `offline: true` short-circuits the fetcher before any network
/// request when the package isn't in the local store. Mocks a server
/// with `.expect(0)` so the assertion fires *only* if the fetcher
/// ever calls the mocked URL; the offline gate must keep it from
/// ever reaching `fetch_and_extract_with_retry`.
#[tokio::test]
async fn offline_mode_skips_network_on_cache_miss() {
    use pacquet_diagnostics::miette::Diagnostic;

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
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: &pkg_integrity,
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
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("offline + cache miss must error before reaching the network");

    // Variant shape + diagnostic code together. The `code` check
    // pins the user-facing surface — `ERR_PACQUET_NO_OFFLINE_TARBALL`
    // is part of the CLI contract, just like upstream's
    // `ERR_PNPM_NO_OFFLINE_META`.
    let TarballError::NoOfflineTarball { package_id, url: errored_url } = &err else {
        panic!("expected NoOfflineTarball, got {err:?}");
    };
    assert_eq!(package_id, pkg_id);
    assert_eq!(errored_url, &url);
    let code = err.code().map(|c| c.to_string()).unwrap_or_default();
    assert_eq!(
        code, "ERR_PACQUET_NO_OFFLINE_TARBALL",
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
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: &pkg_integrity,
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

/// Ported from upstream's
/// [`normalizeBundledManifest.test.ts`](https://github.com/pnpm/pnpm/blob/1fb8a2d5d8/store/cafs/test/normalizeBundledManifest.test.ts).
///
/// Pacquet's [`normalize_bundled_manifest`] picks the subset of
/// `package.json` fields downstream install code reads (bin lookup,
/// peer extraction, build-script detection) and narrows `scripts` to
/// the three lifecycle hooks. Adding a case here? Add (or mirror) the
/// upstream case too. Two upstream cases are intentionally NOT ported:
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
        // Mirrors upstream's type guard at the top of
        // `normalizeBundledManifest` — a non-object input degrades to
        // `None` rather than panicking.
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

    /// Upstream skips `null` and `undefined` fields. Rust's
    /// [`serde_json::Value`] has no `undefined`, but JSON `null`
    /// reaches the picker as [`serde_json::Value::Null`] and must be
    /// filtered the same way (`if (...!v.is_null())` in the source).
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
    /// [`extract_peer_dependencies`](https://github.com/pnpm/pnpm/blob/1fb8a2d5d8/pacquet/crates/resolving-deps-resolver/src/resolve_dependency_tree.rs#L776-L824)
    /// and `extract_children`; dropping `peerDependenciesMeta` or
    /// `optionalDependencies` here would replicate the pnpm/pnpm#11934
    /// resolver-side bug on the install-side. Pin the keys explicitly.
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

/// Saturated `dist` stats must not collide with the latency-class
/// sentinel (`UNPRIORITIZED`) — a hostile registry publishing absurd
/// sizes would otherwise reclassify its downloads as metadata.
#[test]
fn download_priority_never_reaches_the_latency_sentinel() {
    let priority = download_priority(Some(usize::MAX), Some(usize::MAX));
    assert!(priority < UNPRIORITIZED);
    assert_eq!(priority, UNPRIORITIZED - 1);
}
