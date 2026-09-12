use super::{
    AuthHeaders, Cursor, FASTIFY_ERROR_INTEGRITY, FASTIFY_ERROR_TARBALL, Integrity,
    MAX_THROUGHPUT_PRIORITY, PackageFilesIndex, Read, STREAM_ENTRY_BUFFER_MAX,
    STREAM_EXTRACT_COMPRESSED_THRESHOLD, SilentReporter, TarballError, ThrottledClient,
    UNPRIORITIZED, assert_eq, build_zip, download_priority, extract_tarball_entries,
    extract_zip_entries, fast_retry_opts, fetch_and_extract_with_retry, gzip_bytes,
    incompressible_tarball, integrity, stream_extract_gzipped_tarball, tar_with_raw_entry_name,
    tar_with_root_level_entries, tempdir_with_leaked_path, write_zip_entry_to_cas,
};

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

    use pnpm_reporter::{
        FetchingProgressLog, FetchingProgressMessage, LogEvent, ProgressMessage, Reporter as _,
    };

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
            LogEvent::FetchingProgress(FetchingProgressLog {
                message: FetchingProgressMessage::Started { attempt, package_id, size },
                ..
            }) => {
                assert_eq!(package_id, "@fastify/error@3.3.0");
                Some((*attempt, *size))
            }
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

#[test]
fn extract_strips_only_one_component_from_a_dot_prefixed_entry_path() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_raw_entry_name(b"./package/package.json", b"bytes");
    let (cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    assert_eq!(cas_paths.keys().collect::<Vec<_>>(), vec!["package/package.json"]);
    assert_eq!(pkg_files_idx.files.keys().collect::<Vec<_>>(), vec!["package/package.json"]);
    assert!(pkg_files_idx.manifest.is_none());

    drop(tempdir);
}

/// An entry at the archive root has no top-level directory on it to
/// strip, so it is keyed by its own name — what pnpm does, and what
/// keeps the shared `index.db` describing one file layout. Rejecting
/// such an entry fails the whole archive, which npm and pnpm both
/// install.
#[test]
fn extract_keys_a_root_level_entry_by_its_own_name() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_root_level_entries();
    let (cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    let mut keys = cas_paths.keys().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, vec!["._package", "README", "index.js", "package.json"]);
    assert_eq!(
        pkg_files_idx.manifest.as_ref().and_then(|manifest| manifest["name"].as_str()),
        Some("pkg-root-entry"),
        "the manifest under `package/` is still the one captured",
    );

    drop(tempdir);
}

/// The streaming extractor keys a root-level entry the same way, since
/// [`should_stream_extract`] routes between the two per download and the
/// shared `index.db` must not be able to tell them apart.
#[test]
fn streaming_extract_keys_a_root_level_entry_by_its_own_name() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_root_level_entries();
    let (cas_paths, _) = stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), store_path, None)
        .expect("extract the tarball");

    let mut keys = cas_paths.keys().collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, vec!["._package", "README", "index.js", "package.json"]);

    drop(tempdir);
}

/// A lone `.` names the archive root rather than a file inside it, so no
/// key can address it. It stays rejected, unlike the root-level entries
/// above that have a name to be keyed by.
#[test]
fn extract_rejects_an_entry_naming_the_archive_root() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let tar_bytes = tar_with_raw_entry_name(b"./.", b"bytes");
    let err = extract_tarball_entries(&tar_bytes, store_path, None)
        .expect_err("an entry naming the archive root must be rejected");

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

    let tar_bytes = tar_with_raw_entry_name(br"package/bin\tool.js", b"bytes");
    let (cas_paths, _) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");

    assert!(
        cas_paths.contains_key("bin/tool.js"),
        "a backslash separator must resolve to the same key as `/`, got {:?}",
        cas_paths.keys().collect::<Vec<_>>(),
    );

    drop(tempdir);
}
