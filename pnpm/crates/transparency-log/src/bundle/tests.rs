use super::{BundleError, decode_bundle, encode_bundle};
use pretty_assertions::assert_eq;

#[test]
fn round_trips_length_prefixed_entries() {
    let entries: [&[u8]; 3] = [b"first", b"", b"third entry"];
    let bundle = encode_bundle(&entries).unwrap();
    assert_eq!(&bundle[..7], b"\x00\x05first");
    assert_eq!(&bundle[7..9], b"\x00\x00");
    assert_eq!(decode_bundle(&bundle).unwrap(), entries);
}

#[test]
fn rejects_oversized_entries_and_bundles() {
    let large = vec![0_u8; 65_536];
    assert_eq!(encode_bundle(&[large]), Err(BundleError::EntryTooLarge { size: 65_536 }));
    let too_many = vec![b"x"; 257];
    assert_eq!(encode_bundle(&too_many), Err(BundleError::TooManyEntries));
    let bundle = [0_u8, 1, b'x'].repeat(257);
    assert_eq!(decode_bundle(&bundle), Err(BundleError::TooManyEntries));
}

#[test]
fn rejects_truncated_bundles() {
    assert_eq!(decode_bundle(b"\x00"), Err(BundleError::Truncated));
    assert_eq!(decode_bundle(b"\x00\x05four"), Err(BundleError::Truncated));
}
