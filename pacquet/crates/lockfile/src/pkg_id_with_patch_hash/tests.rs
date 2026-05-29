use super::PkgIdWithPatchHash;
use pretty_assertions::assert_eq;

/// `#[serde(transparent)]` guarantees the on-disk shape is the
/// raw string — no `{ "0": "..." }` wrapping, no struct
/// indirection. Pins both sides so a future refactor that
/// drops the attribute would surface here rather than in some
/// downstream `.modules.yaml` consumer that suddenly can't
/// parse pacquet's output.
#[test]
fn serde_round_trip_matches_plain_string() {
    let original = PkgIdWithPatchHash::from("foo@1.0.0(patch_hash=abc)");
    let json = serde_json::to_string(&original).unwrap();
    assert_eq!(json, r#""foo@1.0.0(patch_hash=abc)""#);
    let round_tripped: PkgIdWithPatchHash = serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped, original);
}

/// Per upstream rule (and `CLAUDE.md` rule 3 for non-validating
/// brands), construction must be infallible. Pin both
/// `From<String>` (via `derive_more`) and `From<&str>` (manual
/// impl) so call sites can choose without intermediate
/// allocations.
#[test]
fn constructs_from_string_and_str_without_validation() {
    let from_string = PkgIdWithPatchHash::from(String::from("bar@2.0.0"));
    let from_str = PkgIdWithPatchHash::from("bar@2.0.0");
    assert_eq!(from_string, from_str);
    // The "obviously not a real ident" case must still go
    // through — non-validating means non-validating.
    let nonsense = PkgIdWithPatchHash::from("");
    assert_eq!(nonsense.as_str(), "");
}
