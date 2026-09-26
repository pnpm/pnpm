use super::{
    Arc, ArchiveStoreProjection, AuthHeaders, CafsFileInfo, HashMap, IngestTarballToStore,
    Integrity, PackageFilesIndex, SharedVerifiedFilesCache, SilentReporter, StoreDir, StoreIndex,
    StoreIndexWriter, TarballError, assert_eq, fast_fail_client, integrity, store_index_key,
    tempdir_with_leaked_path, test_retry_opts,
};

const PACKAGE_ID: &str = "fallback@1.0.0";
const MANIFEST: &[u8] = br#"{"name":"fallback","version":"1.0.0"}"#;
const SCRIPT: &[u8] = b"#!/usr/bin/env node\n";

fn package_integrity() -> Integrity {
    integrity(
        "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysPYCfdwTgVb0suyqF4bmI3ZJno7K1aUa6Q==",
    )
}

fn seed_fallback_store(store: &StoreDir) {
    let file_info = |content: &[u8], mode: u32| {
        let (_, hash) = store
            .write_cas_file(content, mode == 0o755)
            .unwrap();
        CafsFileInfo {
            digest: format!("{hash:x}"),
            mode,
            size: content.len() as u64,
            checked_at: None,
        }
    };
    let row = PackageFilesIndex {
        manifest: Some(serde_json::from_slice(MANIFEST).unwrap()),
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files: HashMap::from([
            ("package.json".to_string(), file_info(MANIFEST, 0o644)),
            ("bin.js".to_string(), file_info(SCRIPT, 0o755)),
        ]),
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    StoreIndex::open_in(store)
        .unwrap()
        .set(&store_index_key(&package_integrity().to_string(), PACKAGE_ID), &row)
        .unwrap();
}

/// Returns the ingested files and the keys `store`'s index holds afterwards.
async fn ingest_offline(
    store: &'static StoreDir,
    fallback: &'static StoreDir,
) -> (Result<HashMap<String, std::path::PathBuf>, TarballError>, Vec<String>) {
    let client = fast_fail_client();
    let auth_headers = AuthHeaders::default();
    let package_integrity = package_integrity();
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    let result = IngestTarballToStore {
        fetching: crate::ArchiveFetchOptions {
            http_client: &client,
            auth_headers: &auth_headers,
            retry_opts: test_retry_opts(),
            offline: true,
        },
        package: crate::TarballPackage {
            integrity: Some(&package_integrity),
            unpacked_size: None,
            file_count: None,
            url: "https://example.test/fallback.tgz",
            id: PACKAGE_ID,
        },
        store: crate::ArchiveStoreContext {
            dir: store,
            index: StoreIndex::shared_readonly_in(store),
            index_writer: Some(Arc::clone(&writer)),
            verify_integrity: true,
            strict_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            prefetched_cas_paths: None,
            fallback_dir: Some(fallback),
        },
        requester: "",
        ignore_file_pattern: None,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package { append_manifest: None },
    }
    .run_without_mem_cache::<SilentReporter>()
    .await;
    drop(writer);
    writer_task.await.expect("writer task").expect("writer flushed");
    let keys = StoreIndex::open_in(store)
        .unwrap()
        .keys()
        .unwrap();
    (result, keys)
}

#[tokio::test]
async fn package_missing_from_the_store_is_copied_from_the_fallback_store() {
    let (store_tmp, store) = tempdir_with_leaked_path();
    let (fallback_tmp, fallback) = tempdir_with_leaked_path();
    seed_fallback_store(fallback);

    let (result, keys) = ingest_offline(store, fallback).await;
    let files = result.expect("the fallback store should stand in for the download");

    for (name, content) in [("package.json", MANIFEST), ("bin.js", SCRIPT)] {
        let path = &files[name];
        assert!(path.starts_with(store.root()), "{name} must live in the primary store: {path:?}");
        assert_eq!(std::fs::read(path).unwrap(), content);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&files["bin.js"])
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "bin.js must stay executable");
    }
    assert_eq!(keys, [store_index_key(&package_integrity().to_string(), PACKAGE_ID)]);

    drop((store_tmp, fallback_tmp));
}

#[tokio::test]
async fn corrupt_fallback_file_is_not_copied() {
    let (store_tmp, store) = tempdir_with_leaked_path();
    let (fallback_tmp, fallback) = tempdir_with_leaked_path();
    seed_fallback_store(fallback);
    let (manifest_path, _) = fallback.write_cas_file(MANIFEST, false).unwrap();
    std::fs::write(&manifest_path, br#"{"name":"fallbacK","version":"1.0.0"}"#).unwrap();

    let (result, keys) = ingest_offline(store, fallback).await;
    let error = result.expect_err("a corrupt fallback row must fall through to the download");

    assert!(matches!(error, TarballError::NoOfflineTarball { .. }), "{error:?}");
    assert!(keys.is_empty(), "no row may be recorded for a failed copy: {keys:?}");

    drop((store_tmp, fallback_tmp));
}
