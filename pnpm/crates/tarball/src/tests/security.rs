use super::{
    ArchiveStoreProjection, AuthHeaders, SharedVerifiedFilesCache, SilentReporter, TarballError,
    ThrottledClient, assert_eq, build_zip, extract_tarball_entries, fast_retry_opts,
    fetch_and_extract_with_retry, gzip_bytes, is_transient_error, tar_resources::sha512,
    tar_with_raw_entry_name, tempdir_with_leaked_path,
};
use crate::{IngestZipArchiveToStore, IntegrityKind, archive_integrity::ArchiveVerification};

#[cfg(not(target_os = "windows"))]
use super::{
    CafsFileInfo, HashMap, IngestTarballToStore, PackageFilesIndex, StoreIndex, UNREACHABLE_URL,
    fast_fail_client, integrity, store_index_key, test_retry_opts,
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
        tar_integrity: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(&index_key, &entry).unwrap();
    drop(index);

    let err = IngestTarballToStore {
        fetching: crate::ArchiveFetchOptions {
            http_client: &fast_fail_client(),
            auth_headers: &AuthHeaders::default(),
            retry_opts: test_retry_opts(),
            offline: false,
            tar_limits: crate::TarLimits::default(),
        },
        package: crate::TarballPackage {
            integrity_kind: crate::IntegrityKind::Resource,
            integrity: Some(&pkg_integrity),
            unpacked_size: None,
            file_count: None,
            url: UNREACHABLE_URL,
            id: pkg_id,
        },
        store: crate::ArchiveStoreContext {
            dir: store_path,
            index: StoreIndex::shared_readonly_in(store_path),
            index_writer: None,
            verify_integrity: true,
            strict_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            prefetched_cas_paths: None,
        },

        requester: "",

        ignore_file_pattern: None,

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
/// A leading `\` or a Windows drive-prefixed segment counts, since `\`
/// is folded to `/` and stripping `package/` from `package/C:/...`
/// would otherwise leave a drive-prefixed path on Windows.
#[test]
fn extract_rejects_an_absolute_entry_path() {
    for name in [
        &b"/etc/passwd"[..],
        &br"\windows\evil.txt"[..],
        &b"C:/windows/evil.txt"[..],
        &b"package/C:/windows/evil.txt"[..],
    ] {
        let (tempdir, store_path) = tempdir_with_leaked_path();

        let tar_bytes = tar_with_raw_entry_name(name, b"bytes");
        let err =
            extract_tarball_entries(&tar_bytes, crate::TarLimits::default(), store_path, None)
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
    let err = extract_tarball_entries(&tar_bytes, crate::TarLimits::default(), store_path, None)
        .expect_err("a backslash-spelled traversal must be rejected");

    match err {
        TarballError::ReadTarballEntries(io_err) => {
            assert_eq!(io_err.kind(), std::io::ErrorKind::InvalidData);
        }
        other => panic!("expected a rejected tar entry, got {other:?}"),
    }

    drop(tempdir);
}

async fn assert_rejected_tar_entry_not_retried(raw_name: &[u8], kind: IntegrityKind) {
    let tar_bytes = tar_with_raw_entry_name(raw_name, b"evil");
    let tgz_bytes = gzip_bytes(&tar_bytes);
    let pin = match kind {
        IntegrityKind::Resource | IntegrityKind::ResourceOrTar => sha512(&tgz_bytes),
        IntegrityKind::Tar => sha512(&tar_bytes),
    };

    let (tempdir, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/bad-pkg-1.0.0.tgz")
        .with_status(200)
        .with_body(&tgz_bytes)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/bad-pkg-1.0.0.tgz", server.url());

    let err = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        &ArchiveVerification::new(Some(&pin), kind),
        None,
        crate::TarLimits::default(),
        0,
        "bad-pkg@1.0.0",
        "",
        store_path,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .expect_err("a rejected tar entry path must fail");

    assert!(!is_transient_error(&err), "rejected tar entry must not be transient: {err:?}");
    let TarballError::ReadTarballEntries(io_err) = err else {
        panic!("expected ReadTarballEntries for {kind:?}, got {err:?}");
    };
    assert_eq!(io_err.kind(), std::io::ErrorKind::InvalidData);
    mock.assert_async().await;
    drop(tempdir);
}

/// <https://github.com/pnpm/pnpm/issues/17205>
#[tokio::test]
async fn fetch_does_not_retry_rejected_tar_entry_path() {
    for raw_name in [&b"package/../outside.txt"[..], &b"."[..]] {
        for kind in [IntegrityKind::Resource, IntegrityKind::Tar, IntegrityKind::ResourceOrTar] {
            assert_rejected_tar_entry_not_retried(raw_name, kind).await;
        }
    }
}

/// <https://github.com/pnpm/pnpm/issues/17205>
#[tokio::test]
async fn fetch_does_not_retry_zip_path_traversal() {
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let zip_bytes = build_zip(&[("../outside.txt", b"evil")]);
    let zip_integrity = sha512(&zip_bytes);

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/bad-runtime.zip")
        .with_status(200)
        .with_body(&zip_bytes)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/bad-runtime.zip", server.url());
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();

    let err = IngestZipArchiveToStore {
        fetching: crate::ArchiveFetchOptions {
            http_client: &client,
            auth_headers: &auth_headers,
            retry_opts: fast_retry_opts(),
            offline: false,
            tar_limits: crate::TarLimits::default(),
        },
        package: crate::ZipArchivePackage {
            integrity: &zip_integrity,
            url: &url,
            id: "bad-runtime@1.0.0",
            max_bytes: None,
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
        requester: "",
        archive_prefix: None,
        ignore_file_pattern: None,
        store_projection: ArchiveStoreProjection::RawArchive,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect_err("a zip entry escaping the archive root must fail");

    assert!(!is_transient_error(&err), "zip path traversal must not be transient: {err:?}");
    assert!(matches!(err, TarballError::PathTraversal { .. }), "got {err:?}");
    mock.assert_async().await;
    drop(tempdir);
}
