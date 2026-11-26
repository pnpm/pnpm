use super::{
    DecodeError,
    decode,
    encode,
    encoded_len,
};
use pretty_assertions::assert_eq;

#[test]
fn encodes_the_sha256_of_an_empty_input_as_nix_does() {
    // `nix hash convert --to nix32` of the SHA-256 of zero bytes.
    let digest: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    let encoded = "0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73";
    assert_eq!(encode(&digest), encoded);
    assert_eq!(decode(encoded, 32).unwrap(), digest);
}

#[test]
fn round_trips_store_path_hashes() {
    let bytes: Vec<u8> = (0..20)
        .map(|value| value * 13 + 7)
        .collect();
    let encoded = encode(&bytes);
    assert_eq!(encoded.len(), encoded_len(20));
    assert_eq!(decode(&encoded, 20).unwrap(), bytes);
}

#[test]
fn rejects_digits_outside_the_alphabet() {
    assert_eq!(decode(&"e".repeat(32), 20), Err(DecodeError));
}

#[test]
fn rejects_the_wrong_length() {
    assert_eq!(decode("0000", 20), Err(DecodeError));
}

#[test]
fn rejects_bits_past_the_last_byte() {
    // 52 characters carry 260 bits; the top four must be zero for 32 bytes.
    let overflowing = format!("z{}", "0".repeat(51));
    assert_eq!(decode(&overflowing, 32), Err(DecodeError));
}
