use super::{
    TarballError, assert_eq, extract_tarball_entries, tar_with_raw_entry_name,
    tempdir_with_leaked_path,
};

#[cfg(not(target_os = "windows"))]
use super::{
    ArchiveStoreProjection, AuthHeaders, CafsFileInfo, HashMap, IngestTarballToStore,
    PackageFilesIndex, SharedVerifiedFilesCache, SilentReporter, StoreIndex, fast_fail_client,
    integrity, store_index_key, test_retry_opts,
};

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
    .expect_err("symlink at CAFS path must not resolve to a cache hit");
    assert!(
        matches!(err, TarballError::FetchTarball(_)),
        "expected fall-through to network fetch, got: {err:?}",
    );

    drop(store_dir);
}

/// An absolute entry path names a destination of its own, so joining
/// it onto the package directory would write wherever the archive says.
/// A leading `\` counts, since it is the separator pnpm folds to `/`
/// before validating.
#[test]
fn extract_rejects_an_absolute_entry_path() {
    for name in [&b"/etc/passwd"[..], &br"\windows\evil.txt"[..]] {
        let (tempdir, store_path) = tempdir_with_leaked_path();

        let tar_bytes = tar_with_raw_entry_name(name, b"bytes");
        let err = extract_tarball_entries(&tar_bytes, store_path, None)
            .expect_err("an absolute entry path must be rejected");

        match err {
            TarballError::ReadTarballEntries(io_err) => {
                assert_eq!(io_err.kind(), std::io::ErrorKind::InvalidData);
            }
            other => panic!("expected a rejected tar entry, got {other:?}"),
        }

        drop(tempdir);
    }
}

/// A backslash is an ordinary filename character on Unix but a
/// separator on Windows, and these keys travel between the two through
/// the shared `index.db`. pnpm folds `\` to `/` before validating
/// (`parseTarball.ts`), so a traversal spelled with backslashes has to
/// be caught here too rather than stored verbatim.
#[test]
fn extract_rejects_backslash_traversal_in_entry_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_raw_entry_name(br"package/..\..\evil.txt", b"bytes");
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
