use super::{
    Arc, ArchiveStoreProjection, AuthHeaders, CafsFileInfo, HashMap, IngestTarballToStore,
    PackageFilesIndex, PathBuf, PrefetchIntegrityCheck, PrefetchedCasPaths,
    SharedReportedProgressKeys, SharedVerifiedFilesCache, SilentReporter, StoreIndex,
    StoreIndexWriter, TarballError, assert_eq, fast_fail_client, gzipped_tar, integrity,
    prefetch_cas_paths, store_index_cache_key, store_index_key, tempdir, tempdir_with_leaked_path,
    test_retry_opts,
};

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
    let download = IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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

    let cas_paths = IngestTarballToStore {
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
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
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
        PrefetchIntegrityCheck::Eager,
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
        PrefetchIntegrityCheck::Eager,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    let map = prefetched.cas_paths.get(&index_key).expect("hit");
    assert_eq!(map.get("package.json"), Some(&pkg_json_path));
    assert_eq!(prefetched.requires_build.get(&index_key), Some(&true));
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
        PrefetchIntegrityCheck::Skip,
        SharedVerifiedFilesCache::default(),
    )
    .await;

    let map = prefetched.cas_paths.get(&index_key).expect(
        "verify=false should trust the index row and surface the entry without checking disk",
    );
    assert!(map.contains_key("package.json"));
    drop(store_dir);
}

/// Under [`PrefetchIntegrityCheck::Deferred`] a row is returned
/// unchecked and only `verify_rows` decides it, so a caller that
/// materializes a few of many rows stats only those.
#[tokio::test]
async fn prefetch_cas_paths_deferred_check_drops_a_row_only_when_verified() {
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let pkg_integrity = integrity(
        "sha512-q/IXcMGuF8v7ZLf/JeYfE/pB4Wg1yxT6jXJz8JxRK7a4mJSXV1QKMXDPfZkvMHTZpYxWBDoJiXtptDWFnoCA2w==",
    );
    let index = StoreIndex::open_in(store_path).unwrap();
    let mut index_keys = Vec::new();
    for pkg_id in ["gone@1.0.0", "unasked@1.0.0"] {
        let index_key = store_index_key(&pkg_integrity.to_string(), pkg_id);
        let mut files = HashMap::new();
        files.insert(
            "package.json".to_string(),
            CafsFileInfo {
                // Digest of a file that was never written to disk.
                digest: "f".repeat(128),
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
        index.set(&index_key, &entry).unwrap();
        index_keys.push(index_key);
    }
    drop(index);
    let [gone_key, unasked_key] = index_keys.try_into().expect("two keys");

    let verified_files_cache = SharedVerifiedFilesCache::default();
    let mut prefetched = prefetch_cas_paths(
        StoreIndex::shared_readonly_in(store_path),
        store_path,
        vec![gone_key.clone(), unasked_key.clone()],
        PrefetchIntegrityCheck::Deferred,
        SharedVerifiedFilesCache::clone(&verified_files_cache),
    )
    .await;

    assert!(prefetched.cas_paths.contains_key(&gone_key), "unchecked rows are returned");
    assert_eq!(prefetched.requires_build.get(&gone_key), Some(&false));
    assert_eq!(prefetched.pending_checks.len(), 2, "both rows still owe their files check");

    let failed = prefetched.verify_rows([gone_key.as_str()], store_path, &verified_files_cache);
    assert_eq!(failed, 1);
    assert!(!prefetched.cas_paths.contains_key(&gone_key), "a verified missing blob drops the row");
    assert!(!prefetched.requires_build.contains_key(&gone_key));
    assert!(prefetched.cas_paths.contains_key(&unasked_key), "rows nobody asked about stay");
    assert!(prefetched.pending_checks.contains_key(&unasked_key));
    assert!(!prefetched.pending_checks.contains_key(&gone_key));
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
    .expect_err("stale index entry must not resolve to a cache hit");
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
    .expect_err("directory at CAFS path must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
}

#[test]
fn mem_cache_keys_include_every_file_set_discriminator() {
    let package_url = "https://example.test/artifact.tgz";
    let raw = ArchiveStoreProjection::RawArchive.mem_cache_key(package_url, false);
    let first_manifest =
        ArchiveStoreProjection::Package { append_manifest: Some(br#"{"name":"first"}"#) }
            .mem_cache_key(package_url, false);
    let same_manifest =
        ArchiveStoreProjection::Package { append_manifest: Some(br#"{"name":"first"}"#) }
            .mem_cache_key(package_url, false);
    let second_manifest =
        ArchiveStoreProjection::Package { append_manifest: Some(br#"{"name":"second"}"#) }
            .mem_cache_key(package_url, false);

    assert_ne!(raw, package_url);
    assert_eq!(first_manifest, same_manifest);
    assert_ne!(first_manifest, second_manifest);
}

#[tokio::test]
async fn raw_archive_projection_ignores_legacy_package_rows() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("artifact.tgz");
    let archive = gzipped_tar(&[("artifact/README.md", b"fresh raw artifact")]);
    std::fs::write(&tarball_path, &archive).unwrap();
    let package_url = format!("file:{}", tarball_path.display());
    let mut integrity_opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    integrity_opts.input(&archive);
    let integrity = integrity_opts.result();
    let package_id = "crate:artifact@1.0.0";

    let (store_dir, store_path) = tempdir_with_leaked_path();
    store_path.init().unwrap();
    let legacy_contents = b"{}";
    let (_, legacy_file_hash) = store_path.write_cas_file(legacy_contents, false).unwrap();
    let legacy_key = store_index_key(&integrity.to_string(), package_id);
    StoreIndex::open_in(store_path)
        .unwrap()
        .set(
            &legacy_key,
            &PackageFilesIndex {
                manifest: None,
                requires_build: Some(false),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files: HashMap::from([(
                    "package.json".to_string(),
                    CafsFileInfo {
                        digest: format!("{legacy_file_hash:x}"),
                        mode: 0o644,
                        size: legacy_contents.len() as u64,
                        checked_at: None,
                    },
                )]),
                side_effects: None,
                remote_side_effects_quarantine: None,
            },
        )
        .unwrap();

    let (writer, writer_task) = StoreIndexWriter::spawn(store_path);
    let cas_paths = IngestTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: StoreIndex::shared_readonly_in(store_path),
        store_index_writer: Some(Arc::clone(&writer)),
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(&integrity),
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &package_url,
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
    .expect("a legacy npm-projected row must not satisfy a raw archive read");

    assert_eq!(cas_paths.keys().collect::<Vec<_>>(), ["README.md"]);
    assert_eq!(std::fs::read(&cas_paths["README.md"]).unwrap(), b"fresh raw artifact");

    drop(writer);
    writer_task.await.expect("writer task").expect("writer flushed");
    let raw_key =
        store_index_cache_key(Some(&integrity), package_id, ArchiveStoreProjection::RawArchive)
            .unwrap();
    assert_ne!(raw_key, legacy_key);

    let index = StoreIndex::open_in(store_path).expect("open store index");
    assert!(index.get(&legacy_key).unwrap().is_some(), "legacy row is retained");
    let raw_entry = index.get(&raw_key).unwrap().expect("raw row is indexed separately");
    assert_eq!(raw_entry.files.keys().collect::<Vec<_>>(), ["README.md"]);

    drop((index, store_dir, local_dir));
}
