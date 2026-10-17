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
