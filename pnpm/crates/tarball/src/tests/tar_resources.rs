use super::{
    AuthHeaders, Integrity, SilentReporter, StoreDir, TarballError, ThrottledClient, assert_eq,
    fast_retry_opts, fetch_and_extract_with_retry, gzip_bytes, incompressible_tarball,
    tar_with_entries, tempdir_with_leaked_path,
};
use crate::ArchiveLimit;
use std::io::{Read, Write};

const MANIFEST: &[u8] = br#"{"name":"tar-pkg","version":"1.0.0"}"#;

pub(super) fn small_tar() -> Vec<u8> {
    tar_with_entries(&[
        ("package/package.json", MANIFEST),
        ("package/index.js", b"module.exports = 1\n"),
    ])
}

pub(super) fn sha512(bytes: &[u8]) -> Integrity {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result()
}

fn zstd_bytes(bytes: &[u8]) -> Vec<u8> {
    zstd::encode_all(bytes, 3).expect("zstd encode")
}

/// A tar larger than the threshold at which a body with a known length is
/// extracted while it downloads, recovered from the `.tgz` fixture builder.
pub(super) fn large_tar() -> Vec<u8> {
    let tgz = incompressible_tarball(5 * 1024 * 1024);
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(tgz.as_slice()).read_to_end(&mut tar).expect("gunzip fixture");
    tar
}

async fn fetch(
    url: &str,
    integrity: Option<&Integrity>,
    unpacked_size: Option<usize>,
    store_dir: &'static StoreDir,
) -> Result<(Integrity, std::collections::HashMap<String, std::path::PathBuf>), TarballError> {
    fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        url,
        &crate::archive_integrity::ArchiveVerification::new(
            integrity,
            crate::IntegrityKind::Resource,
        ),
        unpacked_size,
        crate::TarLimits::default(),
        0,
        "tar-pkg@1.0.0",
        "",
        store_dir,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .map(|(integrity, files, _)| (integrity.resource, files))
}

pub(super) async fn serve(
    body: Vec<u8>,
    content_encoding: Option<&str>,
) -> (mockito::ServerGuard, mockito::Mock, String) {
    let mut server = mockito::Server::new_async().await;
    let mut mock = server
        .mock("GET", "/tar-pkg@1.0.0")
        .match_header("accept-encoding", "zstd, gzip")
        .with_status(200)
        .with_body(body);
    if let Some(encoding) = content_encoding {
        mock = mock.with_header("content-encoding", encoding);
    }
    let mock = mock.create_async().await;
    let url = format!("{}/tar-pkg@1.0.0", server.url());
    (server, mock, url)
}

#[tokio::test]
async fn a_tar_body_is_hashed_and_extracted_under_every_content_coding() {
    let tar = small_tar();
    let expected = sha512(&tar);
    for (body, encoding) in [
        (tar.clone(), None),
        (zstd_bytes(&tar), Some("zstd")),
        (gzip_bytes(&tar), Some("gzip")),
        (gzip_bytes(&tar), Some("x-gzip")),
    ] {
        let (_store_keep, store_dir) = tempdir_with_leaked_path();
        let (_server, mock, url) = serve(body, encoding).await;
        let (integrity, files) = fetch(&url, Some(&expected), None, store_dir)
            .await
            .expect("the tar must extract");
        dbg!(encoding, files.keys().collect::<Vec<_>>());
        assert_eq!(integrity, expected);
        assert!(files.contains_key("package.json") && files.contains_key("index.js"));
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn a_large_tar_is_extracted_while_it_downloads() {
    let tar = large_tar();
    let expected = sha512(&tar);
    for (body, encoding) in
        [(tar.clone(), None), (zstd_bytes(&tar), Some("zstd")), (gzip_bytes(&tar), Some("gzip"))]
    {
        let (_store_keep, store_dir) = tempdir_with_leaked_path();
        let (_server, mock, url) = serve(body, encoding).await;
        let (integrity, files) = fetch(&url, None, Some(tar.len()), store_dir)
            .await
            .expect("the tar must extract");
        assert_eq!(integrity, expected, "the computed hash is the tar's, for {encoding:?}");
        assert!(files.contains_key("noise.bin"));
        mock.assert_async().await;
    }
}

/// The `.tgz` a gzip-encoded response decodes to keeps today's identity.
#[tokio::test]
async fn a_gzip_encoded_tgz_is_hashed_as_the_tgz() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tgz = gzip_bytes(&small_tar());
    let expected = sha512(&tgz);
    let (_server, mock, url) = serve(gzip_bytes(&tgz), Some("gzip")).await;
    let (integrity, files) =
        fetch(&url, None, None, store_dir).await.expect("the tgz must extract");
    assert_eq!(integrity, expected);
    assert!(files.contains_key("package.json"));
    mock.assert_async().await;
}

#[tokio::test]
async fn a_tar_that_does_not_match_its_pin_fails_the_integrity_check() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tar = small_tar();
    let wrong = Integrity::from(b"another tar");
    let (_server, _mock, url) = serve(zstd_bytes(&tar), Some("zstd")).await;
    let error = fetch(&url, Some(&wrong), None, store_dir).await.unwrap_err();
    assert!(matches!(error, TarballError::Checksum(_)), "got {error:?}");
}

#[tokio::test]
async fn a_zstd_window_above_8_mib_is_refused() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tar = large_tar();
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    encoder.window_log(24).unwrap();
    encoder.include_contentsize(false).unwrap();
    encoder.write_all(&tar).unwrap();
    let (_server, _mock, url) = serve(encoder.finish().unwrap(), Some("zstd")).await;
    let error = fetch(&url, None, None, store_dir).await.unwrap_err();
    assert!(
        matches!(error, TarballError::ContentDecode { encoding: "zstd", .. }),
        "a 16 MiB window must not decode, got {error:?}",
    );
}

#[tokio::test]
async fn an_encoding_pnpm_did_not_offer_is_refused() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (_server, mock, url) = serve(small_tar(), Some("br")).await;
    let error = fetch(&url, None, None, store_dir).await.unwrap_err();
    assert!(
        matches!(&error, TarballError::UnsupportedContentEncoding { encoding, .. } if encoding == "br"),
        "got {error:?}",
    );
    mock.expect(1).assert_async().await;
}

#[tokio::test]
async fn file_payload_beyond_the_unpacked_size_fails_the_package() {
    let tar = small_tar();
    let payload = MANIFEST.len() + b"module.exports = 1\n".len();
    for unpacked_size in [payload, payload - 1] {
        let (_store_keep, store_dir) = tempdir_with_leaked_path();
        let (_server, _mock, url) = serve(zstd_bytes(&tar), Some("zstd")).await;
        let received = fetch(&url, None, Some(unpacked_size), store_dir).await;
        if unpacked_size == payload {
            received.expect("an exact unpacked size must pass");
        } else {
            let error = received.unwrap_err();
            assert!(
                matches!(error, TarballError::ArchiveLimit(ArchiveLimit::Payload(limit)) if limit == unpacked_size as u64),
                "got {error:?}",
            );
        }
    }
}

#[tokio::test]
async fn the_configured_payload_ceiling_applies_without_an_unpacked_size() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (_server, _mock, url) = serve(zstd_bytes(&small_tar()), Some("zstd")).await;
    let error = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        &crate::archive_integrity::ArchiveVerification::new(None, crate::IntegrityKind::Resource),
        None,
        crate::TarLimits::new(8),
        0,
        "tar-pkg@1.0.0",
        "",
        store_dir,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, TarballError::ArchiveLimit(ArchiveLimit::Payload(8))), "got {error:?}");
}

/// A `.tgz` keeps the plain ceiling, so a registry that under-reports
/// `dist.unpackedSize` breaks no install that worked before.
#[tokio::test]
async fn a_tgz_is_not_held_to_its_unpacked_size() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (_server, _mock, url) = serve(gzip_bytes(&small_tar()), None).await;
    fetch(&url, None, Some(1), store_dir).await.expect("the tgz must extract");
}

#[tokio::test]
async fn data_after_the_end_of_archive_fails_the_package() {
    let mut tar = small_tar();
    let zero_padding = vec![0u8; 10 * 1024];
    let mut padded = tar.clone();
    padded.extend_from_slice(&zero_padding);
    tar.extend_from_slice(b"trailing junk");
    for (body, accepted) in [(padded, true), (tar, false)] {
        let (_store_keep, store_dir) = tempdir_with_leaked_path();
        let (_server, _mock, url) = serve(body, None).await;
        let received = fetch(&url, None, None, store_dir).await;
        dbg!(accepted, received.as_ref().err());
        assert_eq!(received.is_ok(), accepted);
    }
}

#[tokio::test]
async fn an_empty_body_is_not_an_archive() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (_server, _mock, url) = serve(Vec::new(), None).await;
    let error = fetch(&url, None, None, store_dir).await.unwrap_err();
    assert!(matches!(error, TarballError::ReadTarballEntries(_)), "got {error:?}");
}

/// A tar whose only entry is a skipped one with a body of `len` bytes.
fn tar_with_skipped_entry(len: usize) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Char);
    header.set_size(len as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "package/device", vec![0u8; len].as_slice())
        .unwrap();
    builder.into_inner().unwrap()
}

/// The bodies of skipped entries count against the non-payload limit, on the
/// eager path and the streaming one.
#[test]
fn skipped_entry_bodies_count_as_non_payload() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tar = tar_with_skipped_entry(4096);
    let mut limits = crate::TarLimits::default();
    limits.max_non_payload = 1024;
    let eager = super::extract_tarball_entries(&tar, limits, store_dir, None).unwrap_err();
    let streamed =
        super::stream_extract_gzipped_tarball(&gzip_bytes(&tar), limits, store_dir, None)
            .unwrap_err();
    for error in [eager, streamed] {
        assert!(
            matches!(error, TarballError::ArchiveLimit(ArchiveLimit::NonPayload(1024))),
            "got {error:?}",
        );
    }
    super::extract_tarball_entries(&tar, crate::TarLimits::default(), store_dir, None)
        .expect("the default limit admits the archive");
}

/// Extraction stops at the entry that breaks the payload ceiling, and the
/// rest of the body is then drained into the hash. That rest overrunning the
/// non-payload limit must not hide the payload failure.
#[tokio::test]
async fn a_streamed_payload_failure_is_reported_over_the_drain_that_follows() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tar = tar_with_entries(&[("package/data.bin", &[7u8; 4096])]);
    let (_server, _mock, url) = serve(zstd_bytes(&tar), Some("zstd")).await;
    let mut limits = crate::TarLimits::new(1024);
    limits.max_non_payload = 1024;
    // Declaring more than fits in memory routes the body to the streaming
    // extractor.
    let streamed_size = Some(crate::MAX_UNTRUSTED_PREALLOC_BYTES + 1);
    let error = fetch_and_extract_with_retry::<SilentReporter>(
        &ThrottledClient::default(),
        &url,
        &crate::archive_integrity::ArchiveVerification::new(None, crate::IntegrityKind::Resource),
        streamed_size,
        limits,
        0,
        "tar-pkg@1.0.0",
        "",
        store_dir,
        fast_retry_opts(),
        &AuthHeaders::default(),
        None,
        None,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, TarballError::ArchiveLimit(ArchiveLimit::Payload(_))), "got {error:?}");
}

#[tokio::test]
async fn an_html_page_is_not_an_archive() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let page = b"<!doctype html><title>Sign in</title>".repeat(64);
    let (_server, _mock, url) = serve(page, None).await;
    let error = fetch(&url, None, None, store_dir).await.unwrap_err();
    assert!(matches!(error, TarballError::UnrecognizedArchive), "got {error:?}");
}
