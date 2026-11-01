mod archive_contract;

use super::{
    ArchiveIntegrity, ArchiveStoreProjection, FetchTarballForResolution, IgnoreEntryFilter,
    MAX_UNTRUSTED_PREALLOC_BYTES, MemCache, Reporter, RetryOpts, SharedReportedProgressKeys,
    TarLimits,
    archive_integrity::ArchiveVerification,
    archive_retry::is_transient_error,
    auth_header_for_package_download,
    download::{
        IngestTarballToStore, download_priority, fetch_and_extract_archive_with_retry,
        slow_download_warning, store_index_cache_key,
    },
    error::{HttpStatusError, NetworkError, TarballError, VerifyChecksumError},
    extract::{
        STREAM_ENTRY_BUFFER_MAX, STREAM_EXTRACT_COMPRESSED_THRESHOLD, allocate_tarball_buffer,
        apply_append_manifest, apply_placeholder_manifest, decompress_gzip,
        extract_gzipped_tarball_with_tar, extract_tarball_entries_with_bins,
        gzip::{bounded_gzip_size_hint, gzip_isize_hint, should_stream_extract},
        is_eager_decode_limit_exceeded, stream_extract_gzipped_tarball_with_tar,
    },
    local_tarball::{
        allocate_local_tarball_buffer, local_file_tarball_path, open_local_tarball,
        read_local_tarball_buffer, read_local_tarball_metadata,
    },
    prefetch::{PrefetchIntegrityCheck, PrefetchedCasPaths, prefetch_cas_paths},
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

/// The store file each name maps to, without the modes the install read.
fn store_paths(files: &pnpm_fs::CasPaths) -> HashMap<&str, &Path> {
    files
        .iter()
        .map(|(name, file)| (name.as_str(), file.path.as_path()))
        .collect()
}

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
}

fn extract_tarball_entries(
    tar_data: &[u8],
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(pnpm_fs::CasPaths, PackageFilesIndex), TarballError> {
    let (files, index, _) =
        extract_tarball_entries_with_bins(tar_data, limits, store_dir, ignore_file_pattern)?;
    Ok((files, index))
}

#[expect(
    clippy::too_many_arguments,
    reason = "test wrapper forwarding install-scoped inputs to fetch_and_extract_archive_with_retry"
)]
async fn fetch_and_extract_with_retry<TestReporter: Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    verification: &ArchiveVerification,
    package_unpacked_size: Option<usize>,
    tar_limits: TarLimits,
    download_priority: u64,
    package_id: &str,
    requester: &str,
    store_dir: &'static StoreDir,
    retry_opts: RetryOpts,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    revision_addressed: bool,
) -> Result<(ArchiveIntegrity, pnpm_fs::CasPaths, PackageFilesIndex), TarballError> {
    let (integrity, files, index, _) = fetch_and_extract_archive_with_retry::<TestReporter>(
        http_client,
        package_url,
        verification,
        package_unpacked_size,
        tar_limits,
        download_priority,
        package_id,
        requester,
        store_dir,
        retry_opts,
        auth_headers,
        ignore_file_pattern,
        progress_key,
        revision_addressed,
    )
    .await?;
    Ok((integrity, files, index))
}

/// A URL whose connect fails at once on every OS. Linux and macOS refuse
/// `0.0.0.0:1` as they would loopback; Windows rejects the address without
/// sending a packet, where a refused loopback port takes 2 s to fail.
const UNREACHABLE_URL: &str = "http://0.0.0.0:1/unreachable.tgz";

/// HTTP client for the fall-through tests. A default `ThrottledClient`
/// uses `Client::new()` with no connect / request timeout, so on a
/// firewalled runner an unreachable URL could stall for minutes of TCP
/// retry. One-second bounds are plenty for loopback and keep the
/// failure mode deterministic.
fn fast_fail_client() -> ThrottledClient {
    fast_fail_client_with_connect_timeout(Duration::from_secs(1))
}

/// [`fast_fail_client`] with a connect bound of the caller's choosing.
fn fast_fail_client_with_connect_timeout(connect_timeout: Duration) -> ThrottledClient {
    let build = |redirect| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(connect_timeout)
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
    let leaked_path = tempdir
        .path()
        .to_path_buf()
        .pipe(StoreDir::from)
        .pipe(Box::new)
        .pipe(Box::leak);
    (tempdir, leaked_path)
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
        tar_integrity: None,
    };
    let index = StoreIndex::open_in(store_path).unwrap();
    index.set(index_key, &entry).unwrap();
    drop(index);
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
    builder
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip")
}

/// Build a tar archive spanning every entry shape the streaming
/// extractor branches on: a manifest, an executable, a small file, a
/// directory (skipped), and one payload above
/// [`STREAM_ENTRY_BUFFER_MAX`] that must take the
/// direct-to-store streaming branch.
fn mixed_size_tar() -> (Vec<u8>, Vec<u8>) {
    let large_payload: Vec<u8> = (0..=STREAM_ENTRY_BUFFER_MAX)
        .map(|index| (index % 251) as u8)
        .collect();

    let mut builder = tar::Builder::new(Vec::new());
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_size(0);
    dir_header.set_mode(0o755);
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_cksum();
    builder
        .append_data(&mut dir_header, "package/lib/", &b""[..])
        .expect("append dir entry");
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
        let take = buf
            .len()
            .min(usize::try_from(self.cap - self.bytes_read).unwrap_or(usize::MAX));
        buf[..take].fill(b'x');
        self.bytes_read += take as u64;
        Ok(take)
    }
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

/// Build a tar carrying one entry whose raw header name is `name`,
/// bypassing `set_path`'s own validation so hostile names can be
/// tested. `set_path` drops a leading `./` from a multi-component path,
/// so a dot-prefixed name only survives written this way.
fn tar_with_raw_entry_name(name: &[u8], body: &[u8]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    let mut builder = tar::Builder::new(&mut tar_bytes);
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    let raw = header.as_mut_bytes();
    raw[..name.len()].copy_from_slice(name);
    for byte in &mut raw[name.len()..100] {
        *byte = 0;
    }
    header.set_cksum();
    builder.append(&header, body).expect("append entry");
    builder.finish().expect("finalize tar");
    drop(builder);
    tar_bytes
}

/// Build a tar whose payload directory sits beside two root-level
/// entries: macOS `bsdtar` emits the zero-length `AppleDouble` `._package`
/// when the source cannot store xattrs natively, and `tar czf README
/// package` puts an ordinary file up there too.
fn tar_with_root_level_entries() -> Vec<u8> {
    tar_with_entries(&[
        ("._package", b""),
        ("README", b"a file that sits at the archive root\n"),
        ("package/package.json", br#"{"name":"pkg-root-entry","version":"1.0.0"}"#),
        ("package/index.js", b"module.exports = 'hello'\n"),
    ])
}

/// Build an uncompressed tar carrying `entries` in order, each written
/// verbatim as a regular file, so a caller can shape an archive that
/// omits the top-level directory a published tarball wraps its payload
/// in, or that carries an entry beside it at the archive root.
fn tar_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, body) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append_data(&mut header, path, *body).expect("append entry");
    }
    builder.into_inner().expect("finish tar")
}

mod authorization;

mod streaming_formats_warning_for_slow;

mod streaming_fetching_progress_and_fetched;

mod behavior_network_error_display_includes;

mod behavior_retries_other_4xx_codes;

mod reporting;

mod integrity;

mod files;

mod security;

mod manifests;

mod lockfile;

mod fetch_for_resolution;

mod tar_resources;

mod tar_integrity;

/// [`extract_gzipped_tarball_with_tar`] without the tar's integrity.
fn extract_gzipped_tarball(
    gz_data: &[u8],
    unpacked_size: Option<usize>,
    limits: crate::TarLimits,
    store_dir: &pnpm_store_dir::StoreDir,
    ignore_file_pattern: Option<&crate::IgnoreEntryFilter>,
) -> Result<(pnpm_fs::CasPaths, pnpm_store_dir::PackageFilesIndex), TarballError> {
    extract_gzipped_tarball_with_tar(gz_data, unpacked_size, limits, store_dir, ignore_file_pattern)
        .map(|(files, index, _, _)| (files, index))
}

/// [`stream_extract_gzipped_tarball_with_tar`] without the tar's integrity.
fn stream_extract_gzipped_tarball(
    gz_data: &[u8],
    limits: crate::TarLimits,
    store_dir: &pnpm_store_dir::StoreDir,
    ignore_file_pattern: Option<&crate::IgnoreEntryFilter>,
) -> Result<(pnpm_fs::CasPaths, pnpm_store_dir::PackageFilesIndex), TarballError> {
    stream_extract_gzipped_tarball_with_tar(gz_data, limits, store_dir, ignore_file_pattern)
        .map(|(files, index, _, _)| (files, index))
}
