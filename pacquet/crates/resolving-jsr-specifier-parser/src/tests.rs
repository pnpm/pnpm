//! Tests for `parse_jsr_specifier`.

use super::{JsrSpec, ParseJsrSpecifierError, parse_jsr_specifier};

#[test]
fn skips_on_non_jsr_specifiers() {
    for input in [
        "^1.0.0",
        "1.0.0",
        "latest",
        "npm:foo",
        "npm:@foo/bar",
        "npm:@jsr/foo__bar",
        "catalog:",
        "workspace:*",
    ] {
        assert_eq!(parse_jsr_specifier(input, None), Ok(None), "input: {input}");
    }
}

#[test]
fn version_only_specifier_borrows_alias_for_name() {
    for selector in ["^1.0.0", "1.0.0", "latest"] {
        let input = format!("jsr:{selector}");
        let spec = parse_jsr_specifier(&input, Some("@foo/bar"))
            .unwrap_or_else(|err| panic!("parse failed for {input}: {err}"))
            .unwrap_or_else(|| panic!("expected Some for {input}"));
        assert_eq!(
            spec,
            JsrSpec {
                version_selector: Some(selector.to_string()),
                jsr_pkg_name: "@foo/bar".to_string(),
                npm_pkg_name: "@jsr/foo__bar".to_string(),
            },
            "input: {input}",
        );
    }
}

#[test]
fn scope_and_name_only() {
    let spec = parse_jsr_specifier("jsr:@foo/bar", None).unwrap().unwrap();
    assert_eq!(
        spec,
        JsrSpec {
            jsr_pkg_name: "@foo/bar".to_string(),
            npm_pkg_name: "@jsr/foo__bar".to_string(),
            version_selector: None,
        },
    );
}

#[test]
fn scope_name_and_selector() {
    for selector in ["^1.0.0", "1.0.0", "latest"] {
        let input = format!("jsr:@foo/bar@{selector}");
        let spec = parse_jsr_specifier(&input, None).unwrap().unwrap();
        assert_eq!(
            spec,
            JsrSpec {
                jsr_pkg_name: "@foo/bar".to_string(),
                npm_pkg_name: "@jsr/foo__bar".to_string(),
                version_selector: Some(selector.to_string()),
            },
            "input: {input}",
        );
    }
}

#[test]
fn name_without_scope_is_an_error() {
    assert_eq!(
        parse_jsr_specifier("jsr:foo@^1.0.0", None),
        Err(ParseJsrSpecifierError::MissingScope),
    );
}

#[test]
fn scope_without_name_is_an_error() {
    assert_eq!(
        parse_jsr_specifier("jsr:@foo@^1.0.0", None),
        Err(ParseJsrSpecifierError::InvalidPackageName { pkg_name: "@foo".to_string() }),
    );
    assert_eq!(
        parse_jsr_specifier("jsr:@foo", None),
        Err(ParseJsrSpecifierError::InvalidPackageName { pkg_name: "@foo".to_string() }),
    );
}

#[test]
fn version_only_specifier_without_alias_errors() {
    assert_eq!(
        parse_jsr_specifier("jsr:^1.0.0", None),
        Err(ParseJsrSpecifierError::MissingPackageName { specifier: "^1.0.0".to_string() }),
    );
}

#[test]
fn version_only_specifier_with_empty_alias_errors() {
    // An empty alias string is treated as absent, so the version-only
    // branch raises `MissingPackageName` instead of attempting to fold
    // `""` into a package name.
    assert_eq!(
        parse_jsr_specifier("jsr:^1.0.0", Some("")),
        Err(ParseJsrSpecifierError::MissingPackageName { specifier: "^1.0.0".to_string() }),
    );
}
