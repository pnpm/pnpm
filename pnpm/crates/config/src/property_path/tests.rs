use super::{
    ParsePropertyPathError, Segment, get_object_value_by_property_path, parse_property_path,
};
use serde_json::json;

fn keys(path: &str) -> Vec<Segment> {
    parse_property_path(path).expect("parse")
}

#[test]
fn parses_dotted_and_bracketed_forms() {
    assert_eq!(
        keys("foo.bar.baz"),
        vec![Segment::Key("foo".into()), Segment::Key("bar".into()), Segment::Key("baz".into()),],
    );
    assert_eq!(keys(".foo.bar"), vec![Segment::Key("foo".into()), Segment::Key("bar".into())]);
    assert_eq!(
        keys(r#"foo["bar"].baz"#),
        vec![Segment::Key("foo".into()), Segment::Key("bar".into()), Segment::Key("baz".into()),],
    );
    assert_eq!(keys("foo['bar']"), vec![Segment::Key("foo".into()), Segment::Key("bar".into())]);
    assert_eq!(
        keys(r#"["foo"].bar"#),
        vec![Segment::Key("foo".into()), Segment::Key("bar".into())],
    );
    assert_eq!(keys("foo[123]"), vec![Segment::Key("foo".into()), Segment::Index(123.0)]);
    assert!(keys("").is_empty());
}

#[test]
fn parses_scoped_package_keys() {
    assert_eq!(
        keys(r#"packageExtensions["@babel/parser"].peerDependencies"#),
        vec![
            Segment::Key("packageExtensions".into()),
            Segment::Key("@babel/parser".into()),
            Segment::Key("peerDependencies".into()),
        ],
    );
}

#[test]
fn parses_hyphenated_package_keys() {
    // npm pkg get/set style paths with hyphenated dependency names (GH-13163)
    assert_eq!(
        keys("dependencies.some-package-name"),
        vec![Segment::Key("dependencies".into()), Segment::Key("some-package-name".into()),],
    );
    assert_eq!(
        keys("devDependencies.some-package-name"),
        vec![Segment::Key("devDependencies".into()), Segment::Key("some-package-name".into()),],
    );
}

#[test]
fn parse_errors() {
    assert_eq!(
        parse_property_path("foo..bar"),
        Err(ParsePropertyPathError::UnexpectedToken { token: ".".into() }),
    );
    assert_eq!(parse_property_path("foo["), Err(ParsePropertyPathError::UnexpectedEndOfInput));
}

#[test]
fn gets_nested_values() {
    let value = json!({
        "packageExtensions": {
            "@babel/parser": { "peerDependencies": { "@babel/types": "*" } },
        },
        "trustPolicyExclude": ["foo", "bar"],
    });

    assert_eq!(
        get_object_value_by_property_path(&value, &keys("trustPolicyExclude[0]")),
        Some(&json!("foo")),
    );
    assert_eq!(
        get_object_value_by_property_path(&value, &keys("trustPolicyExclude[1]")),
        Some(&json!("bar")),
    );
    assert_eq!(
        get_object_value_by_property_path(
            &value,
            &keys(r#"packageExtensions["@babel/parser"].peerDependencies["@babel/types"]"#)
        ),
        Some(&json!("*")),
    );
    // out-of-range index, missing key, and non-numeric array index → None
    assert_eq!(get_object_value_by_property_path(&value, &keys("trustPolicyExclude[2]")), None);
    assert_eq!(get_object_value_by_property_path(&value, &keys("nope")), None);
    assert_eq!(get_object_value_by_property_path(&value, &keys("trustPolicyExclude.foo")), None);
}
