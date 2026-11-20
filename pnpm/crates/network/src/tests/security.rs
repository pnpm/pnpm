use super::percent_decode_str;

#[test]
fn percent_decode_handles_common_escapes() {
    assert_eq!(percent_decode_str("p%40ss"), "p@ss", "%40 → @");
    assert_eq!(percent_decode_str("user%20name"), "user name");
    assert_eq!(percent_decode_str("plain"), "plain");
    assert_eq!(
        percent_decode_str("bad-%ZZ-escape"),
        "bad-%ZZ-escape",
        "invalid hex passes through",
    );
}
