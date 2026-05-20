use pacquet_resolving_jsr_specifier_parser::ParseJsrSpecifierError;

use crate::{
    parse_bare_specifier::{parse_bare_specifier, parse_jsr_specifier_to_registry_package_spec},
    pick_package_from_meta::RegistryPackageSpecType,
};

const DEFAULT_TAG: &str = "latest";
const REGISTRY: &str = "https://registry.npmjs.org/";

#[test]
fn version_selector_classified_as_version() {
    let spec = parse_bare_specifier("1.0.0", Some("foo"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "foo");
    assert_eq!(spec.fetch_spec, "1.0.0");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Version);
}

#[test]
fn range_selector_classified_as_range() {
    let spec = parse_bare_specifier("^1.0.0", Some("foo"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "foo");
    assert_eq!(spec.fetch_spec, "^1.0.0");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Range);
}

#[test]
fn tag_selector_classified_as_tag() {
    let spec = parse_bare_specifier("latest", Some("foo"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "foo");
    assert_eq!(spec.fetch_spec, "latest");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn no_alias_no_npm_prefix_declines() {
    assert!(parse_bare_specifier("^1.0.0", None, DEFAULT_TAG, REGISTRY).is_none());
}

#[test]
fn npm_alias_with_range_uses_outer_alias_as_name() {
    let spec =
        parse_bare_specifier("npm:^1.0.0", Some("is-positive"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "is-positive");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Range);
}

#[test]
fn npm_alias_with_exact_version_uses_outer_alias_as_name() {
    let spec = parse_bare_specifier("npm:1.0.0", Some("@acme/foo"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "@acme/foo");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Version);
    assert_eq!(spec.fetch_spec, "1.0.0");
}

#[test]
fn npm_alias_with_inner_name_and_range() {
    let spec =
        parse_bare_specifier("npm:lodash@^4.0.0", Some("foo"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "lodash");
    assert_eq!(spec.fetch_spec, "^4.0.0");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Range);
}

#[test]
fn npm_alias_with_inner_scoped_name_and_range() {
    let spec =
        parse_bare_specifier("npm:@scope/foo@^1.0.0", Some("foo"), DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "@scope/foo");
    assert_eq!(spec.fetch_spec, "^1.0.0");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Range);
}

#[test]
fn npm_alias_unversioned_falls_back_to_default_tag() {
    let spec = parse_bare_specifier("npm:is-positive", None, DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "is-positive");
    assert_eq!(spec.fetch_spec, "latest");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn npm_alias_scoped_unversioned_falls_back_to_default_tag() {
    let spec = parse_bare_specifier("npm:@scope/foo", None, DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "@scope/foo");
    assert_eq!(spec.fetch_spec, "latest");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn tarball_url_under_registry_is_parsed() {
    let url = "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz";
    let spec = parse_bare_specifier(url, None, DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "foo");
    assert_eq!(spec.fetch_spec, "1.0.0");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Version);
    assert_eq!(spec.normalized_bare_specifier.as_deref(), Some(url));
}

#[test]
fn tarball_url_for_scoped_package_decodes_path() {
    let url = "https://registry.npmjs.org/@scope/foo/-/foo-1.0.0.tgz";
    let spec = parse_bare_specifier(url, None, DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "@scope/foo");
    assert_eq!(spec.fetch_spec, "1.0.0");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Version);
}

#[test]
fn unrelated_url_declines() {
    let url = "https://example.com/foo/-/foo-1.0.0.tgz";
    assert!(parse_bare_specifier(url, None, DEFAULT_TAG, REGISTRY).is_none());
}

#[test]
fn tarball_url_with_mismatched_filename_declines() {
    // `<registry>/foo/-/bar-1.0.0.tgz` would be a registry-side bug
    // (or a typo'd URL); the parser must not silently map it to a
    // confused `(name, version)` pair just because the length math
    // works out. Anchor on the scopeless-name prefix.
    let url = "https://registry.npmjs.org/foo/-/bar-1.0.0.tgz";
    assert!(parse_bare_specifier(url, None, DEFAULT_TAG, REGISTRY).is_none());
}

#[test]
fn git_protocol_specifier_declines() {
    assert!(
        parse_bare_specifier(
            "git+ssh://git@github.com/owner/repo",
            Some("foo"),
            DEFAULT_TAG,
            REGISTRY,
        )
        .is_none(),
    );
}

#[test]
fn workspace_protocol_specifier_declines() {
    assert!(parse_bare_specifier("workspace:*", Some("foo"), DEFAULT_TAG, REGISTRY).is_none());
}

#[test]
fn npm_prefix_without_alias_uses_bare_as_name_and_falls_back_to_default_tag() {
    // No outer alias → enter the lastIndexOf('@') branch. `^1.0.0` has
    // no `@`, so name = '^1.0.0' and bare = default tag ('latest').
    // Upstream's parser doesn't validate the name; downstream consumers
    // surface the malformed name as ERR_PNPM_INVALID_PACKAGE_NAME from
    // pick_package's validator.
    let spec = parse_bare_specifier("npm:^1.0.0", None, DEFAULT_TAG, REGISTRY).unwrap();
    assert_eq!(spec.name, "^1.0.0");
    assert_eq!(spec.fetch_spec, "latest");
    assert_eq!(spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn jsr_specifier_with_scope_name_and_range() {
    let spec = parse_jsr_specifier_to_registry_package_spec("jsr:@foo/bar@^1.0.0", None, "latest")
        .unwrap()
        .unwrap();
    assert_eq!(spec.spec.name, "@jsr/foo__bar");
    assert_eq!(spec.spec.fetch_spec, "^1.0.0");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Range);
    assert_eq!(spec.jsr_pkg_name, "@foo/bar");
}

#[test]
fn jsr_specifier_without_selector_falls_back_to_default_tag() {
    let spec = parse_jsr_specifier_to_registry_package_spec("jsr:@foo/bar", None, "latest")
        .unwrap()
        .unwrap();
    assert_eq!(spec.spec.name, "@jsr/foo__bar");
    assert_eq!(spec.spec.fetch_spec, "latest");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Tag);
    assert_eq!(spec.jsr_pkg_name, "@foo/bar");
}

#[test]
fn jsr_version_only_specifier_borrows_alias_for_jsr_pkg_name() {
    let spec =
        parse_jsr_specifier_to_registry_package_spec("jsr:^1.0.0", Some("@foo/bar"), "latest")
            .unwrap()
            .unwrap();
    assert_eq!(spec.spec.name, "@jsr/foo__bar");
    assert_eq!(spec.spec.fetch_spec, "^1.0.0");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Range);
    assert_eq!(spec.jsr_pkg_name, "@foo/bar");
}

#[test]
fn jsr_specifier_declines_for_non_jsr_input() {
    assert!(
        parse_jsr_specifier_to_registry_package_spec("npm:lodash@^4", None, "latest")
            .unwrap()
            .is_none(),
    );
    assert!(
        parse_jsr_specifier_to_registry_package_spec("^1.0.0", Some("foo"), "latest")
            .unwrap()
            .is_none(),
    );
}

#[test]
fn jsr_specifier_with_unscoped_name_errors() {
    let err = parse_jsr_specifier_to_registry_package_spec("jsr:foo@^1.0.0", None, "latest")
        .expect_err("unscoped JSR name must error");
    assert!(matches!(err, ParseJsrSpecifierError::MissingScope), "got {err:?}");
}
