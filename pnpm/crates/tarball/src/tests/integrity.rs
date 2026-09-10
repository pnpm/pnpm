use super::{
    Arc, ArchiveStoreProjection, AuthHeaders, CafsFileInfo, FASTIFY_ERROR_INTEGRITY,
    FASTIFY_ERROR_TARBALL, FetchTarballForResolution, HashMap, IngestTarballToStore, Integrity,
    PackageFilesIndex, PrefetchIntegrityCheck, STREAM_EXTRACT_COMPRESSED_THRESHOLD,
    SharedVerifiedFilesCache, SilentReporter, StoreDir, StoreIndex, StoreIndexWriter, TarballError,
    ThrottledClient, assert_eq, fast_fail_client, fast_retry_opts, fetch_and_extract_with_retry,
    incompressible_tarball, integrity, prefetch_cas_paths, read_local_tarball_metadata,
    store_index_key, tempdir, tempdir_with_leaked_path, test_retry_opts,
};

#[tokio::test]
async fn should_throw_error_on_checksum_mismatch() {
    let (store_dir, store_path) = tempdir_with_leaked_path();
    IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("checksum mismatch");

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
        PrefetchIntegrityCheck::Eager,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    assert!(
        !prefetched.cas_paths.contains_key(&index_key),
        "row that fails integrity must not appear in prefetch result",
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
    .expect_err("corrupt digest must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
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
        let result = IngestTarballToStore {
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
            store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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
