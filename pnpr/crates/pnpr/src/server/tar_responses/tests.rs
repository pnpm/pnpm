use super::{AcceptedCodings, HeaderMap, header, tar_integrity_within};
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
fn accept_encoding_honors_refusals_and_the_wildcard() {
    let accepted = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, value.parse().unwrap());
        let AcceptedCodings { zstd, gzip } = AcceptedCodings::of(&headers);
        (zstd, gzip)
    };
    assert_eq!(accepted("zstd, gzip"), (true, true));
    assert_eq!(accepted("gzip;q=0.5, zstd;q=0"), (false, true));
    assert_eq!(accepted("*"), (true, true));
    assert_eq!(accepted("br"), (false, false));
}

async fn decode_and_derive(tgz: Vec<u8>, expected: &str) -> (Vec<u8>, Option<ssri::Integrity>) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let derived = super::DerivedFrom { expected: expected.parse().ok(), copy: None };
    let body = super::gunzip_body(axum::body::Body::from(tgz), derived, move |integrity| {
        let _ = sender.send(integrity);
    });
    let tar = axum::body::to_bytes(body, usize::MAX).await.unwrap().to_vec();
    (
        tar,
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .ok(),
    )
}

/// A cached `.tgz` is served without a re-check against the packument, so
/// stale bytes must not have their tar recorded under the current digest.
#[tokio::test]
async fn the_tar_integrity_is_derived_only_from_bytes_that_match_the_digest() {
    let tar = vec![7u8; 1024];
    let tgz = gzip(&tar);
    let mut digest = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    digest.input(&tgz);
    let matching = digest.result().to_string();
    let mut other = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    other.input(b"another tgz");
    let stale = other.result().to_string();

    let (served, derived) = decode_and_derive(tgz.clone(), &matching).await;
    assert_eq!(served, tar);
    assert!(derived.is_some(), "matching bytes derive the tar integrity");

    let (served, derived) = decode_and_derive(tgz, &stale).await;
    assert_eq!(served, tar, "the response is served either way");
    assert!(derived.is_none(), "stale bytes derive nothing");
}
