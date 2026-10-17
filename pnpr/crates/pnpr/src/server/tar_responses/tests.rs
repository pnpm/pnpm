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
        name: super::CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm)
            .unwrap(),
        cached: None,
        runtime: tokio::runtime::Handle::current(),
    };
    let derived =
        super::DerivedFrom { check: expected.parse().ok(), derive: true, copy: None, stale };
    let body = super::gunzip_body(axum::body::Body::from(tgz), derived, move |integrity| {
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

    let (served, derived) = decode_and_derive(tgz.clone(), &matching).await;
    assert_eq!(served, Some(tar));
    assert!(derived.is_some(), "matching bytes derive the tar integrity");

    let (served, derived) = decode_and_derive(tgz, &stale).await;
    assert_eq!(served, None, "stale bytes end the response with an error");
    assert!(derived.is_none(), "stale bytes derive nothing");
}
