use super::{
    AuthHeaders, Duration, ErrorKind, FASTIFY_ERROR_TARBALL, HashMap, MAX_UNTRUSTED_PREALLOC_BYTES,
    PackageFilesIndex, Path, PathBuf, STREAM_ENTRY_BUFFER_MAX, STREAM_EXTRACT_COMPRESSED_THRESHOLD,
    SilentReporter, StoreDir, TarballError, ThrottledClient, allocate_local_tarball_buffer,
    allocate_tarball_buffer, assert_eq, bounded_gzip_size_hint, decompress_gzip,
    extract_gzipped_tarball, extract_tarball_entries, fast_retry_opts,
    fetch_and_extract_with_retry, gzip_bomb_tarball, gzip_bytes, gzip_isize_hint, integrity,
    is_eager_decode_limit_exceeded, is_transient_error, local_file_tarball_path, mixed_size_tar,
    open_local_tarball, read_local_tarball_buffer, read_local_tarball_metadata,
    should_stream_extract, slow_download_warning, stream_extract_gzipped_tarball, tempdir,
    tempdir_with_leaked_path,
};

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
