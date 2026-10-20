use super::{HeaderMap, TarCoding, decode::tar_integrity_within, header, preferred_coding};
use std::io::Write;

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn a_tgz_that_decodes_past_the_limit_has_no_tar() {
    let tgz = gzip(&[0u8; 4096]);
    assert!(tar_integrity_within(tgz.as_slice(), 4096).is_some());
    assert!(tar_integrity_within(tgz.as_slice(), 4095).is_none());
}

#[test]
fn accept_encoding_prefers_the_higher_quality_and_zstd_on_a_tie() {
    let preferred = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, value.parse().unwrap());
        preferred_coding(&headers)
    };
    assert_eq!(preferred("zstd, gzip"), TarCoding::Zstd);
    assert_eq!(preferred("gzip;q=0.5, zstd;q=0"), TarCoding::Gzip);
    assert_eq!(preferred("zstd;q=0.1, gzip;q=1"), TarCoding::Gzip);
    assert_eq!(preferred("x-gzip"), TarCoding::Gzip);
    assert_eq!(preferred("*"), TarCoding::Zstd);
    assert_eq!(preferred("*;q=0.5, zstd;q=0"), TarCoding::Gzip);
    assert_eq!(preferred("br"), TarCoding::Identity);
}

async fn decode_and_derive(
    tgz: Vec<u8>,
    expected: &str,
    limit: u64,
) -> (Option<Vec<u8>>, Option<ssri::Integrity>) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let storage_dir = tempfile::TempDir::new().unwrap();
    let storage = pnpr_storage::Storage::new(
        &pnpr_config::HostedStoreConfig::Fs,
        storage_dir.path().join("storage"),
        storage_dir.path().join("cache"),
    )
    .unwrap();
    let stale = super::StaleTgz {
        storage,
        name: Some(
            super::CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm)
                .unwrap(),
        ),
        cached: None,
        runtime: tokio::runtime::Handle::current(),
    };
    let derived =
        super::DerivedFrom { check: expected.parse().ok(), hash: true, copy: None, stale, limit };
    let decodes = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let body =
        super::gunzip_body(axum::body::Body::from(tgz), derived, decodes, move |integrity, _| {
            let _ = sender.send(integrity);
        });
    let tar = axum::body::to_bytes(body, usize::MAX).await.ok().map(|bytes| bytes.to_vec());
    (
        tar,
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .ok(),
    )
}

/// A cached `.tgz` is served without a re-check against the packument, so
/// stale bytes must neither complete a response nor have their tar recorded
/// under the current digest.
#[tokio::test]
async fn only_bytes_that_match_the_digest_complete_a_tar_response() {
    let tar = vec![7u8; 1024];
    let tgz = gzip(&tar);
    let mut digest = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    digest.input(&tgz);
    let matching = digest.result().to_string();
    let mut other = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    other.input(b"another tgz");
    let stale = other.result().to_string();

    let limit = super::tar_decode_limit(None);
    let (served, derived) = decode_and_derive(tgz.clone(), &matching, limit).await;
    assert_eq!(served, Some(tar));
    assert!(derived.is_some(), "matching bytes derive the tar integrity");

    let (served, derived) = decode_and_derive(tgz, &stale, limit).await;
    assert_eq!(served, None, "stale bytes end the response with an error");
    assert!(derived.is_none(), "stale bytes derive nothing");
}

#[tokio::test]
async fn a_tar_response_stops_at_its_decode_limit() {
    let tar = vec![0u8; 4096];
    let tgz = gzip(&tar);
    let integrity = ssri::Integrity::from(&tgz).to_string();
    let (served, _) = decode_and_derive(tgz.clone(), &integrity, 4096).await;
    assert_eq!(served, Some(tar));
    let (served, derived) = decode_and_derive(tgz, &integrity, 4095).await;
    assert_eq!(served, None, "a tar past the limit ends the response with an error");
    assert!(derived.is_none());
}

#[test]
fn the_decode_limit_admits_what_pnpm_admits_for_the_declared_size() {
    let non_payload = 256 * 1024 * 1024 + 10 * 1024;
    assert_eq!(super::tar_decode_limit(Some(1024)), 1024 + non_payload);
    assert_eq!(super::tar_decode_limit(Some(u64::MAX)), super::tar_decode_limit(None));
}

#[tokio::test]
async fn a_tar_response_decodes_only_with_a_permit() {
    let tar = vec![7u8; 1024];
    let tgz = gzip(&tar);
    let storage_dir = tempfile::TempDir::new().unwrap();
    let storage = pnpr_storage::Storage::new(
        &pnpr_config::HostedStoreConfig::Fs,
        storage_dir.path().join("storage"),
        storage_dir.path().join("cache"),
    )
    .unwrap();
    let stale = super::StaleTgz {
        storage,
        name: Some(
            super::CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm)
                .unwrap(),
        ),
        cached: None,
        runtime: tokio::runtime::Handle::current(),
    };
    let derived = super::DerivedFrom {
        check: None,
        hash: false,
        copy: None,
        stale,
        limit: super::tar_decode_limit(None),
    };
    let decodes = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let held = std::sync::Arc::clone(&decodes).acquire_owned().await.unwrap();
    let body = super::gunzip_body(axum::body::Body::from(tgz), derived, decodes, |_, _| {});
    let read = tokio::spawn(axum::body::to_bytes(body, usize::MAX));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!read.is_finished(), "no decode runs while every permit is taken");
    drop(held);
    assert_eq!(read.await.unwrap().unwrap().to_vec(), tar);
}
