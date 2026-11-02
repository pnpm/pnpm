use super::RecordDigest;
use pretty_assertions::{assert_eq, assert_ne};

#[test]
fn is_the_sha512_of_the_bytes() {
    let digest = RecordDigest::of(b"abc");
    assert_eq!(
        digest.to_hex(),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
         2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
    );
    assert_eq!(
        digest.to_string(),
        "sha512-3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw==",
    );
}

#[test]
fn round_trips_through_text() {
    let digest = RecordDigest::of(b"record bytes");
    assert_eq!(
        digest
            .to_string()
            .parse::<RecordDigest>()
            .unwrap(),
        digest,
    );
    for text in ["sha256-AAAA", "sha512-AAAA", "sha512-not base64", ""] {
        assert!(text.parse::<RecordDigest>().is_err(), "accepted {text}");
    }
}

#[test]
fn covers_bytes_not_parsed_json() {
    assert_ne!(RecordDigest::of(br#"{"a":1}"#), RecordDigest::of(br#"{ "a": 1 }"#));
}
