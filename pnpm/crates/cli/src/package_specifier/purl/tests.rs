use super::{
    Purl,
    PurlType,
    Shown,
    strip_scheme,
};
use pretty_assertions::assert_eq;

fn purl(specifier: &str) -> Purl {
    let body = strip_scheme(specifier).expect("recognize the purl scheme");
    Purl::parse(body, Shown(specifier)).expect("parse the purl")
}

fn error(specifier: &str) -> String {
    let body = strip_scheme(specifier).expect("recognize the purl scheme");
    Purl::parse(body, Shown(specifier)).expect_err("reject the purl").to_string()
}

fn expect(
    package_type: PurlType,
    namespace: Option<&str>,
    name: &str,
    version: Option<&str>,
) -> Purl {
    Purl {
        package_type,
        namespace: namespace.map(str::to_string),
        name: name.to_string(),
        version: version.map(str::to_string),
    }
}

#[test]
fn recognizes_only_the_pkg_scheme() {
    assert_eq!(strip_scheme("pkg:npm/lodash"), Some("npm/lodash"));
    assert_eq!(strip_scheme("PKG:npm/lodash"), Some("npm/lodash"));
    assert_eq!(strip_scheme("pkg://npm/lodash"), Some("npm/lodash"));
    assert_eq!(strip_scheme("npm:lodash@4"), None);
    assert_eq!(strip_scheme("lodash@4"), None);
    assert_eq!(strip_scheme("package:npm/lodash"), None);
}

#[test]
fn parses_the_type_name_and_version() {
    assert_eq!(
        purl("pkg:cargo/serde@1.0.188"),
        expect(PurlType::Cargo, None, "serde", Some("1.0.188")),
    );
}

#[test]
fn lowercases_the_type_and_keeps_the_version_opaque() {
    assert_eq!(
        purl("pkg:PyPI/requests@2.31.0rc1"),
        expect(PurlType::Pypi, None, "requests", Some("2.31.0rc1")),
    );
}

#[test]
fn version_is_optional() {
    assert_eq!(purl("pkg:npm/express"), expect(PurlType::Npm, None, "express", None));
}

#[test]
fn percent_decodes_every_component() {
    assert_eq!(
        purl("pkg:npm/%40babel/core@7.22.0"),
        expect(PurlType::Npm, Some("@babel"), "core", Some("7.22.0")),
    );
}

/// The version is read from the last path segment, so the unencoded `@` of a
/// scope cannot be mistaken for the version separator.
#[test]
fn reads_an_unencoded_scope_as_a_namespace() {
    assert_eq!(purl("pkg:npm/@babel/core"), expect(PurlType::Npm, Some("@babel"), "core", None));
}

#[test]
fn ignores_insignificant_slashes() {
    assert_eq!(
        purl("pkg://npm//%40babel/core@7.22.0"),
        expect(PurlType::Npm, Some("@babel"), "core", Some("7.22.0")),
    );
    assert_eq!(purl("pkg:npm/express/"), expect(PurlType::Npm, None, "express", None));
}

#[test]
fn keeps_multi_segment_namespaces() {
    assert_eq!(
        purl("pkg:npm/%40scope/nested/name@1.8.0"),
        expect(PurlType::Npm, Some("@scope/nested"), "name", Some("1.8.0")),
    );
}

#[test]
fn rejects_qualifiers_and_subpaths() {
    assert_eq!(
        error("pkg:pypi/django@1.11.1?file_name=Django-1.11.1.tar.gz"),
        "pkg:pypi/django@1.11.1 carries purl qualifiers, which pnpm cannot honor",
    );
    assert_eq!(
        error("pkg:npm/lodash@4.17.21#lib"),
        "pkg:npm/lodash@4.17.21 carries a purl subpath, which pnpm cannot honor",
    );
}

#[test]
fn rejects_malformed_purls() {
    for (specifier, message) in [
        ("pkg:npm", "pkg:npm is missing a purl name"),
        ("pkg:npm/", "pkg:npm/ is missing a purl name"),
        ("pkg:/lodash", "pkg:/lodash is missing a purl name"),
        ("pkg:npm/lodash@", "missing version after `@` in pkg:npm/lodash@"),
        ("pkg:1npm/lodash", "pkg:1npm/lodash has an invalid purl type `1npm`"),
        ("pkg:np m/lodash", "pkg:np m/lodash has an invalid purl type `np m`"),
        ("pkg:npm/%2f/lodash", "pkg:npm/%2f/lodash has an invalid purl namespace"),
    ] {
        assert_eq!(error(specifier), message, "{specifier}");
    }
}
