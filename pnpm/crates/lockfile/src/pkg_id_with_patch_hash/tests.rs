use super::PkgIdWithPatchHash;
use pretty_assertions::assert_eq;

#[test]
fn serde_round_trip_matches_plain_string() {
    let original = PkgIdWithPatchHash::from("foo@1.0.0(patch_hash=abc)");
    let json = serde_json::to_string(&original).unwrap();
    assert_eq!(json, r#""foo@1.0.0(patch_hash=abc)""#);
    let round_tripped: PkgIdWithPatchHash = serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped, original);
}

#[test]
fn constructs_from_string_and_str_without_validation() {
    let from_string = PkgIdWithPatchHash::from(String::from("bar@2.0.0"));
    let from_str = PkgIdWithPatchHash::from("bar@2.0.0");
    assert_eq!(from_string, from_str);
    let nonsense = PkgIdWithPatchHash::from("");
    assert_eq!(nonsense.as_str(), "");
}
