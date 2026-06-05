#![allow(
    clippy::needless_pass_by_value,
    reason = "nested test helpers take owned fixture values; by-value keeps call sites and assert ergonomics simple"
)]

use super::{ParsePkgNameError, PkgName};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::borrow::Cow;

#[test]
fn parse_ok() {
    fn case(input: &'static str, output: PkgName) {
        eprintln!("CASE: {input:?}");
        let actual: PkgName = input.parse().unwrap();
        assert_eq!(&actual, &output);
    }

    case("@foo/bar", PkgName { scope: Some("foo".to_string()), bare: "bar".to_string() });
    case("foo-bar", PkgName { scope: None, bare: "foo-bar".to_string() });
}

#[test]
fn deserialize_ok() {
    fn case(input: &'static str, output: PkgName) {
        eprintln!("CASE: {input:?}");
        let actual: PkgName = serde_saphyr::from_str(input).unwrap();
        assert_eq!(&actual, &output);
    }

    case("'@foo/bar'", PkgName { scope: Some("foo".to_string()), bare: "bar".to_string() });
    case("foo-bar", PkgName { scope: None, bare: "foo-bar".to_string() });
}

#[test]
fn deserialize_decodes_escape_sequences() {
    // The YAML scalar `"\u0040foo/bar"` decodes the `\u0040` escape
    // to `@`, yielding `@foo/bar`. The deserializer must allocate a
    // fresh buffer to apply the escape, so a borrowed `&'de str`
    // source would reject this input.
    let input = r#""\u0040foo/bar""#;
    eprintln!("CASE: {input:?}");
    let actual: PkgName = serde_saphyr::from_str(input).unwrap();
    dbg!(&actual);
    assert_eq!(actual, PkgName { scope: Some("foo".to_string()), bare: "bar".to_string() });
}

#[test]
fn parse_err() {
    macro_rules! case {
        ($input:expr => $message:expr, $pattern:pat) => {{
            let input = $input;
            eprintln!("CASE: {input:?}");
            let error = input.parse::<PkgName>().unwrap_err();
            dbg!(&error);
            assert_eq!(error.to_string(), $message);
            assert!(matches!(&error, $pattern));
        }};
    }

    case!("@foo" => "Missing bare name", ParsePkgNameError::MissingName);
    case!("" => "Name is empty", ParsePkgNameError::EmptyName);
}

#[test]
fn to_string() {
    fn case(input: PkgName, output: &'static str) {
        eprintln!("CASE: {input:?}");
        assert_eq!(input.to_string(), output);
    }

    case(PkgName { scope: Some("foo".to_string()), bare: "bar".to_string() }, "@foo/bar");
    case(PkgName { scope: None, bare: "foo-bar".to_string() }, "foo-bar");
}

#[test]
fn serialize() {
    fn case(input: PkgName, output: &'static str) {
        eprintln!("CASE: {input:?}");
        let received = input.pipe_ref(serde_saphyr::to_string).unwrap();
        assert_eq!(received, output);
    }

    case(PkgName { scope: Some("foo".to_string()), bare: "bar".to_string() }, "\"@foo/bar\"\n");
    case(PkgName { scope: None, bare: "foo-bar".to_string() }, "foo-bar\n");
}

/// `TryFrom<String>` and `TryFrom<Cow<'_, str>>` route through
/// the validating parser. Owned and borrowed input forms must
/// behave identically, since both back the serde deserializer
/// and the public constructor in different contexts.
#[test]
fn try_from_owned_and_cow_route_through_parse() {
    let from_string = PkgName::try_from("@foo/bar".to_string()).expect("valid scoped name parses");
    assert_eq!(from_string.scope.as_deref(), Some("foo"));
    assert_eq!(from_string.bare, "bar");

    let from_cow = PkgName::try_from(Cow::Borrowed("foo-bar")).expect("valid bare name parses");
    assert!(from_cow.scope.is_none());
    assert_eq!(from_cow.bare, "foo-bar");

    // Invalid input still propagates `ParsePkgNameError` from both
    // entry points — pin that the error type matches.
    let owned_err =
        PkgName::try_from(String::new()).expect_err("empty string must fail validation");
    assert!(matches!(owned_err, ParsePkgNameError::EmptyName));

    let cow_err =
        PkgName::try_from(Cow::Owned(String::new())).expect_err("empty cow must fail validation");
    assert!(matches!(cow_err, ParsePkgNameError::EmptyName));
}
