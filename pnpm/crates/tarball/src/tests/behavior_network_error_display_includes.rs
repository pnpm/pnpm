use super::{
    Arc, ArchiveStoreProjection, AuthHeaders, CafsFileInfo, Duration, FASTIFY_ERROR_INTEGRITY,
    FASTIFY_ERROR_TARBALL, HashMap, HttpStatusError, IngestTarballToStore, Integrity, MemCache,
    NetworkError, PackageFilesIndex, RetryOpts, SharedVerifiedFilesCache, SilentReporter,
    StoreIndex, StoreIndexWriter, TarballError, ThrottledClient, VerifyChecksumError, assert_eq,
    fast_fail_client, fast_retry_opts, fetch_and_extract_with_retry, gzipped_tar, integrity,
    is_transient_error, seed_row_holding_another_package, store_index_cache_key, store_index_key,
    tempdir, tempdir_with_leaked_path, test_retry_opts,
};

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

#[tokio::test]
#[cfg(not(target_os = "windows"))]
async fn packages_under_orgs_should_work() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    let cas_files = IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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

    let err = IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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

#[test]
fn package_projection_preserves_existing_store_index_keys() {
    let integrity = integrity("sha256-q80k8iD1xuGM3a48ipTFD+P7KQnhs4e5Blnos+dQpJM=");
    let package_id = "artifact@1.0.0";
    let legacy_key = store_index_key(&integrity.to_string(), package_id);

    assert_eq!(
        store_index_cache_key(
            Some(&integrity),
            package_id,
            ArchiveStoreProjection::Package { append_manifest: None },
        ),
        Some(legacy_key.clone()),
    );
    assert_ne!(
        store_index_cache_key(
            Some(&integrity),
            package_id,
            ArchiveStoreProjection::Package { append_manifest: Some(br#"{"name":"runtime"}"#) },
        ),
        Some(legacy_key),
    );
}

#[test]
fn ordinary_package_projection_preserves_existing_mem_cache_keys() {
    let package_url = "https://example.test/artifact.tgz";
    let package = ArchiveStoreProjection::Package { append_manifest: None };

    assert_eq!(package.mem_cache_key(package_url, false), package_url);
    assert_eq!(
        package.mem_cache_key(package_url, true),
        format!("revision-addressed:{package_url}"),
    );
}

#[tokio::test]
async fn mem_cache_partitions_raw_and_package_projections_in_both_orders() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("artifact.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("artifact/README.md", b"archive")])).unwrap();
    let package_url = format!("file:{}", tarball_path.display());
    let client = fast_fail_client();
    let auth_headers = AuthHeaders::default();

    for package_first in [true, false] {
        let (store_dir, store_path) = tempdir_with_leaked_path();
        let mem_cache = MemCache::default();
        let ingest = |store_projection| IngestTarballToStore {
            http_client: &client,
            store_dir: store_path,
            store_index: None,
            store_index_writer: None,
            verify_store_integrity: true,
            strict_store_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            package_integrity: None,
            package_unpacked_size: None,
            package_file_count: None,
            package_url: &package_url,
            package_id: "artifact@1.0.0",
            requester: "",
            prefetched_cas_paths: None,
            retry_opts: test_retry_opts(),
            auth_headers: &auth_headers,
            ignore_file_pattern: None,
            offline: true,
            progress_reported: None,
            store_projection,
        };

        let (package_files, raw_files) = if package_first {
            let package_files = ingest(ArchiveStoreProjection::Package { append_manifest: None })
                .run_with_mem_cache::<SilentReporter>(&mem_cache)
                .await
                .unwrap();
            let raw_files = ingest(ArchiveStoreProjection::RawArchive)
                .run_with_mem_cache::<SilentReporter>(&mem_cache)
                .await
                .unwrap();
            (package_files, raw_files)
        } else {
            let raw_files = ingest(ArchiveStoreProjection::RawArchive)
                .run_with_mem_cache::<SilentReporter>(&mem_cache)
                .await
                .unwrap();
            let package_files = ingest(ArchiveStoreProjection::Package { append_manifest: None })
                .run_with_mem_cache::<SilentReporter>(&mem_cache)
                .await
                .unwrap();
            (package_files, raw_files)
        };

        let mut package_names = package_files.keys().map(String::as_str).collect::<Vec<_>>();
        package_names.sort_unstable();
        assert_eq!(package_names, ["README.md", "package.json"]);
        assert_eq!(raw_files.keys().map(String::as_str).collect::<Vec<_>>(), ["README.md"]);
        assert_eq!(mem_cache.len(), 2);
        drop(store_dir);
    }

    drop(local_dir);
}

#[tokio::test]
async fn synthesized_projection_reuses_only_a_matching_legacy_row_offline() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    store_path.init().unwrap();
    let package_id = "artifact@1.0.0";
    let package_integrity = integrity(
        "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysPYCfdwTgVb0suyqF4bmI3ZJno7K1aUa6Q==",
    );
    let manifest = br#"{"name":"artifact","version":"1.0.0"}"#;
    let readme = b"legacy runtime archive";
    let (_, manifest_hash) = store_path.write_cas_file(manifest, false).unwrap();
    let (_, readme_hash) = store_path.write_cas_file(readme, false).unwrap();
    let legacy_key = store_index_key(&package_integrity.to_string(), package_id);
    StoreIndex::open_in(store_path)
        .unwrap()
        .set(
            &legacy_key,
            &PackageFilesIndex {
                manifest: Some(serde_json::from_slice(manifest).unwrap()),
                requires_build: Some(false),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files: HashMap::from([
                    (
                        "README.md".to_string(),
                        CafsFileInfo {
                            digest: format!("{readme_hash:x}"),
                            mode: 0o644,
                            size: readme.len() as u64,
                            checked_at: None,
                        },
                    ),
                    (
                        "package.json".to_string(),
                        CafsFileInfo {
                            digest: format!("{manifest_hash:x}"),
                            mode: 0o644,
                            size: manifest.len() as u64,
                            checked_at: None,
                        },
                    ),
                ]),
                side_effects: None,
                remote_side_effects_quarantine: None,
            },
        )
        .unwrap();

    let client = fast_fail_client();
    let auth_headers = AuthHeaders::default();
    let store_index = StoreIndex::shared_readonly_in(store_path);
    let ingest = |append_manifest| IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: store_index.clone(),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(&package_integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: "https://example.test/runtime.tgz",
        package_id,
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &auth_headers,
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package {
            append_manifest: Some(append_manifest),
        },
    };

    let files = ingest(manifest)
        .run_without_mem_cache::<SilentReporter>()
        .await
        .expect("a matching legacy runtime row should remain available offline");
    assert_eq!(std::fs::read(&files["package.json"]).unwrap(), manifest);
    assert_eq!(std::fs::read(&files["README.md"]).unwrap(), readme);

    let error = ingest(br#"{"name":"other","version":"1.0.0"}"#)
        .run_without_mem_cache::<SilentReporter>()
        .await
        .expect_err("a different synthesized manifest must not reuse the legacy row");
    assert!(matches!(error, TarballError::NoOfflineTarball { .. }));
    drop(store_dir);
}

#[tokio::test]
async fn raw_archive_projection_skips_npm_identity_checks_on_store_hits() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    store_path.init().unwrap();
    let contents = b"raw artifact";
    let (cas_path, file_hash) = store_path.write_cas_file(contents, false).unwrap();
    let integrity = integrity("sha256-q80k8iD1xuGM3a48ipTFD+P7KQnhs4e5Blnos+dQpJM=");
    let package_id = "crate:artifact@1.0.0";
    StoreIndex::open_in(store_path)
        .unwrap()
        .set(
            &store_index_cache_key(
                Some(&integrity),
                package_id,
                ArchiveStoreProjection::RawArchive,
            )
            .unwrap(),
            &PackageFilesIndex {
                manifest: Some(serde_json::json!({ "name": "unrelated", "version": "2.0.0" })),
                requires_build: Some(false),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files: HashMap::from([(
                    "README.md".to_string(),
                    CafsFileInfo {
                        digest: format!("{file_hash:x}"),
                        mode: 0o644,
                        size: contents.len() as u64,
                        checked_at: None,
                    },
                )]),
                side_effects: None,
                remote_side_effects_quarantine: None,
            },
        )
        .unwrap();

    let cas_paths = IngestTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(&integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: "https://example.test/artifact.tgz",
        package_id,
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::RawArchive,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect("raw archive cache hits must not use npm package identity semantics");

    assert_eq!(cas_paths, HashMap::from([("README.md".to_string(), cas_path)]));
    drop(store_dir);
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
    let cas_paths = IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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
    let download = || IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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
    let download = || IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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
