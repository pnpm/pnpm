mod archive_contract;

use super::{
    ArchiveStoreProjection, FetchTarballForResolution, MAX_UNTRUSTED_PREALLOC_BYTES, MemCache,
    RetryOpts, SharedReportedProgressKeys, auth_header_for_package_download,
    download::{
        IngestTarballToStore, download_priority, fetch_and_extract_with_retry, is_transient_error,
        slow_download_warning, store_index_cache_key,
    },
    error::{HttpStatusError, NetworkError, TarballError, VerifyChecksumError},
    extract::{
        STREAM_ENTRY_BUFFER_MAX, STREAM_EXTRACT_COMPRESSED_THRESHOLD, allocate_tarball_buffer,
        apply_append_manifest, apply_placeholder_manifest, bounded_gzip_size_hint, decompress_gzip,
        extract_gzipped_tarball, extract_tarball_entries, gzip_isize_hint,
        is_eager_decode_limit_exceeded, normalize_bundled_manifest, should_stream_extract,
        stream_extract_gzipped_tarball,
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

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
}

/// HTTP client for the fall-through tests. A default `ThrottledClient`
/// uses `Client::new()` with no connect / request timeout, so on a
/// firewalled runner the unreachable `http://127.0.0.1:1/...` URL
/// could stall for minutes of TCP retry. One-second bounds are
/// plenty for loopback and keep the failure mode deterministic.
fn fast_fail_client() -> ThrottledClient {
    let build = |redirect| {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(1))
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
    let leaked_path =
        tempdir.path().to_path_buf().pipe(StoreDir::from).pipe(Box::new).pipe(Box::leak);
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
    builder.into_inner().expect("finish tar").finish().expect("finish gzip")
}

/// Build a tar archive spanning every entry shape the streaming
/// extractor branches on: a manifest, an executable, a small file, a
/// directory (skipped), and one payload above
/// [`STREAM_ENTRY_BUFFER_MAX`] that must take the
/// direct-to-store streaming branch.
fn mixed_size_tar() -> (Vec<u8>, Vec<u8>) {
    let large_payload: Vec<u8> =
        (0..=STREAM_ENTRY_BUFFER_MAX).map(|index| (index % 251) as u8).collect();

    let mut builder = tar::Builder::new(Vec::new());
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_size(0);
    dir_header.set_mode(0o755);
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_cksum();
    builder.append_data(&mut dir_header, "package/lib/", &b""[..]).expect("append dir entry");
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
        let take = buf.len().min(usize::try_from(self.cap - self.bytes_read).unwrap_or(usize::MAX));
        buf[..take].fill(b'x');
        self.bytes_read += take as u64;
        Ok(take)
    }
}

/// Pacquet's [`normalize_bundled_manifest`] picks the subset of
/// `package.json` fields downstream install code reads (bin lookup,
/// peer extraction, build-script detection) and narrows `scripts` to
/// the three lifecycle hooks. Two cases are intentionally NOT covered:
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

    /// `null` and `undefined` fields are skipped. Rust's
    /// [`serde_json::Value`] has no `undefined`, but JSON `null`
    /// reaches the picker as [`serde_json::Value::Null`] and must be
    /// filtered out the same way.
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
    /// `extract_peer_dependencies` and `extract_children`; dropping
    /// `peerDependenciesMeta` or `optionalDependencies` here would
    /// replicate the pnpm/pnpm#11934 resolver-side bug on the
    /// install-side. Pin the keys explicitly.
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
