use crate::{
    PackageSelector, ParseOverridesError, VersionOverride, create_overrides_map_from_parsed,
    parse_overrides, parse_pkg_and_parent_selector,
};
use pacquet_catalogs_types::{Catalog, Catalogs};
use std::collections::HashMap;

fn vo(
    selector: &str,
    new_bare: &str,
    parent: Option<PackageSelector>,
    target: PackageSelector,
) -> VersionOverride {
    VersionOverride {
        selector: selector.to_string(),
        parent_pkg: parent,
        target_pkg: target,
        new_bare_specifier: new_bare.to_string(),
        converge: false,
    }
}

fn sel(name: &str, bare: Option<&str>) -> PackageSelector {
    PackageSelector { name: name.to_string(), bare_specifier: bare.map(str::to_owned) }
}

/// `HashMap` iteration order is unspecified, so when comparing
/// multi-entry outputs we sort by `selector` on both sides.
fn sorted(mut overrides: Vec<VersionOverride>) -> Vec<VersionOverride> {
    overrides.sort_by(|lhs, rhs| lhs.selector.cmp(&rhs.selector));
    overrides
}

#[test]
fn parses_bare_name_override() {
    let input = HashMap::from([("foo".to_string(), "1".to_string())]);
    let out = parse_overrides(&input, &Catalogs::new()).unwrap();
    assert_eq!(out, vec![vo("foo", "1", None, sel("foo", None))]);
}

#[test]
fn parses_name_at_version_override() {
    let input = HashMap::from([("foo@2".to_string(), "1".to_string())]);
    let out = parse_overrides(&input, &Catalogs::new()).unwrap();
    assert_eq!(out, vec![vo("foo@2", "1", None, sel("foo", Some("2")))]);
}

#[test]
fn parses_range_operators_in_target() {
    let input = HashMap::from([
        ("foo@>2".to_string(), "1".to_string()),
        ("foo@3 || >=2".to_string(), "1".to_string()),
    ]);
    let out = sorted(parse_overrides(&input, &Catalogs::new()).unwrap());
    assert_eq!(
        out,
        sorted(vec![
            vo("foo@>2", "1", None, sel("foo", Some(">2"))),
            vo("foo@3 || >=2", "1", None, sel("foo", Some("3 || >=2"))),
        ]),
    );
}

#[test]
fn parses_parent_child_selectors() {
    let input = HashMap::from([
        ("bar>foo".to_string(), "2".to_string()),
        ("bar@1>foo".to_string(), "2".to_string()),
        ("bar>foo@1".to_string(), "2".to_string()),
        ("bar@1>foo@1".to_string(), "2".to_string()),
    ]);
    let out = sorted(parse_overrides(&input, &Catalogs::new()).unwrap());
    assert_eq!(
        out,
        sorted(vec![
            vo("bar>foo", "2", Some(sel("bar", None)), sel("foo", None)),
            vo("bar@1>foo", "2", Some(sel("bar", Some("1"))), sel("foo", None)),
            vo("bar>foo@1", "2", Some(sel("bar", None)), sel("foo", Some("1"))),
            vo("bar@1>foo@1", "2", Some(sel("bar", Some("1"))), sel("foo", Some("1"))),
        ]),
    );
}

#[test]
fn range_operator_on_parent_does_not_split() {
    // Without the `[^ |@]>` constraint, `foo@>2>bar@>2` would split
    // at the first `>` (inside the `>2` range). Mirrors upstream's
    // exact disambiguation.
    let input = HashMap::from([
        ("foo@>2>bar@>2".to_string(), "1".to_string()),
        ("foo@3 || >=2>bar@3 || >=2".to_string(), "1".to_string()),
    ]);
    let out = sorted(parse_overrides(&input, &Catalogs::new()).unwrap());
    assert_eq!(
        out,
        sorted(vec![
            vo("foo@>2>bar@>2", "1", Some(sel("foo", Some(">2"))), sel("bar", Some(">2"))),
            vo(
                "foo@3 || >=2>bar@3 || >=2",
                "1",
                Some(sel("foo", Some("3 || >=2"))),
                sel("bar", Some("3 || >=2")),
            ),
        ]),
    );
}

#[test]
fn rejects_invalid_selector() {
    let input = HashMap::from([("%".to_string(), "2".to_string())]);
    assert_eq!(
        parse_overrides(&input, &Catalogs::new()).unwrap_err(),
        ParseOverridesError::InvalidSelector { selector: "%".to_string() },
    );
}

#[test]
fn rejects_invalid_selector_with_whitespace() {
    // `foo > bar` — the regex requires the byte before `>` to be
    // non-space, so the parser sees no parent>child split and falls
    // through to `parse_pkg_selector("foo > bar")`, which fails
    // because `parse_wanted_dependency` doesn't validate the alias.
    let input = HashMap::from([("foo > bar".to_string(), "2".to_string())]);
    assert_eq!(
        parse_overrides(&input, &Catalogs::new()).unwrap_err(),
        ParseOverridesError::InvalidSelector { selector: "foo > bar".to_string() },
    );
}

#[test]
fn parse_pkg_and_parent_selector_lone_target() {
    assert_eq!(parse_pkg_and_parent_selector("foo").unwrap(), (None, sel("foo", None)));
}

#[test]
fn parse_pkg_and_parent_selector_parent_child() {
    assert_eq!(
        parse_pkg_and_parent_selector("bar@1>foo@2").unwrap(),
        (Some(sel("bar", Some("1"))), sel("foo", Some("2"))),
    );
}

#[test]
fn catalog_protocol_with_missing_entry_errors() {
    let input = HashMap::from([("foo".to_string(), "catalog:default".to_string())]);
    let err = parse_overrides(&input, &Catalogs::new()).unwrap_err();
    let ParseOverridesError::CatalogInOverrides { message } = err else {
        panic!("expected CatalogInOverrides, got {err:?}");
    };
    assert!(
        message.contains("foo") && message.contains("default"),
        "message should mention target and catalog name, got: {message}",
    );
}

#[test]
fn catalog_protocol_resolves_to_catalog_specifier() {
    let mut catalogs = Catalogs::new();
    let mut default = Catalog::new();
    default.insert("foo".to_string(), "^1.2.3".to_string());
    catalogs.insert("default".to_string(), default);

    let input = HashMap::from([("foo".to_string(), "catalog:".to_string())]);
    let out = parse_overrides(&input, &catalogs).unwrap();
    assert_eq!(out, vec![vo("foo", "^1.2.3", None, sel("foo", None))]);
}

#[test]
fn catalog_protocol_with_named_catalog_resolves() {
    let mut catalogs = Catalogs::new();
    let mut shared = Catalog::new();
    shared.insert("bar".to_string(), "2.0.0".to_string());
    catalogs.insert("shared".to_string(), shared);

    let input = HashMap::from([("bar".to_string(), "catalog:shared".to_string())]);
    let out = parse_overrides(&input, &catalogs).unwrap();
    assert_eq!(out, vec![vo("bar", "2.0.0", None, sel("bar", None))]);
}

#[test]
fn parses_convergence_override() {
    for (selector, version, name) in
        [("foo@", "1.2.3", "foo"), ("@scope/foo@", "2.0.0-beta.1", "@scope/foo")]
    {
        let input = HashMap::from([(selector.to_string(), version.to_string())]);
        let out = parse_overrides(&input, &Catalogs::new()).unwrap();
        let expected =
            VersionOverride { converge: true, ..vo(selector, version, None, sel(name, Some(""))) };
        assert_eq!(out, vec![expected]);
    }
}

#[test]
fn resolves_catalog_value_of_convergence_override() {
    let mut catalogs = Catalogs::new();
    let mut default = Catalog::new();
    default.insert("foo".to_string(), "1.2.3".to_string());
    catalogs.insert("default".to_string(), default);

    let input = HashMap::from([("foo@".to_string(), "catalog:".to_string())]);
    let out = parse_overrides(&input, &catalogs).unwrap();
    let expected =
        VersionOverride { converge: true, ..vo("foo@", "1.2.3", None, sel("foo", Some(""))) };
    assert_eq!(out, vec![expected]);
}

#[test]
fn rejects_non_exact_convergence_override_values() {
    for value in ["^1.2.3", "latest", "-", "link:../foo", "npm:bar@1.2.3"] {
        let input = HashMap::from([("foo@".to_string(), value.to_string())]);
        let message = parse_overrides(&input, &Catalogs::new()).unwrap_err().to_string();
        eprintln!("MESSAGE:\n{message}\n");
        assert_eq!(
            message,
            format!(
                r#"The value of the convergence override "foo@" must be an exact version, but got "{value}""#
            ),
        );
    }
}

#[test]
fn rejects_empty_range_in_parent_child_selector() {
    let input = HashMap::from([("bar>foo@".to_string(), "1.2.3".to_string())]);
    let message = parse_overrides(&input, &Catalogs::new()).unwrap_err().to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(
        message,
        r#"Cannot use an empty range in the "bar>foo@" selector: convergence overrides ("pkg@") cannot be combined with parent>child selectors"#,
    );
}

#[test]
fn create_overrides_map_returns_resolved_specifiers() {
    let mut catalogs = Catalogs::new();
    let mut default = Catalog::new();
    default.insert("foo".to_string(), "^1.2.3".to_string());
    catalogs.insert("default".to_string(), default);

    let input = HashMap::from([
        ("foo".to_string(), "catalog:".to_string()),
        ("bar".to_string(), "2.0.0".to_string()),
    ]);
    let parsed = parse_overrides(&input, &catalogs).unwrap();
    let map = create_overrides_map_from_parsed(&parsed);
    assert_eq!(map.get("foo").map(String::as_str), Some("^1.2.3"));
    assert_eq!(map.get("bar").map(String::as_str), Some("2.0.0"));
}
