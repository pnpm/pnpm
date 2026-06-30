use std::collections::HashSet;

use pacquet_resolving_jsr_specifier_parser::ParseJsrSpecifierError;

use crate::{
    parse_bare_specifier::{
        ParseNamedRegistrySpecifierError, parse_bare_specifier,
        parse_jsr_specifier_to_registry_package_spec,
        parse_named_registry_specifier_to_registry_package_spec,
    },
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
    // The parser doesn't validate the name; downstream consumers
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

fn gh_aliases() -> HashSet<String> {
    let mut set = HashSet::new();
    set.insert("gh".to_string());
    set
}

fn aliases(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn named_registry_returns_none_on_non_named_specifiers() {
    let gh = gh_aliases();
    for input in [
        "^1.0.0",
        "1.0.0",
        "latest",
        "npm:foo",
        "npm:@foo/bar",
        "jsr:@foo/bar",
        "catalog:",
        "workspace:*",
    ] {
        let result =
            parse_named_registry_specifier_to_registry_package_spec(input, &gh, None, "latest");
        assert!(matches!(result, Ok(None)), "expected None for {input:?}, got {result:?}");
    }
}

#[test]
fn named_registry_does_not_intercept_github_git_shorthand() {
    // `github:` belongs to hosted-git-info / npm-package-arg as a
    // GitHub git repository shortcut. Even if it shows up, it is not
    // in the built-in `gh` alias set.
    let gh = gh_aliases();
    for input in ["github:owner/repo", "github:owner/repo#main", "github:@acme/foo"] {
        let result =
            parse_named_registry_specifier_to_registry_package_spec(input, &gh, None, "latest");
        assert!(matches!(result, Ok(None)), "expected None for {input:?}, got {result:?}");
    }
}

#[test]
fn named_registry_with_scoped_alias_parses_version_selectors() {
    let gh = gh_aliases();
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:^1.0.0",
        &gh,
        Some("@acme/foo"),
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Range);
    assert_eq!(spec.registry_name, "gh");

    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:1.0.0",
        &gh,
        Some("@acme/foo"),
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
    assert_eq!(spec.spec.fetch_spec, "1.0.0");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Version);

    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:latest",
        &gh,
        Some("@acme/foo"),
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
    assert_eq!(spec.spec.fetch_spec, "latest");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn named_registry_with_scoped_body_falls_back_to_default_tag() {
    let gh = gh_aliases();
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:@acme/foo",
        &gh,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
    assert_eq!(spec.spec.fetch_spec, "latest");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Tag);
    assert_eq!(spec.registry_name, "gh");
}

#[test]
fn named_registry_with_scoped_body_and_selector() {
    let gh = gh_aliases();
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:@acme/foo@^1.0.0",
        &gh,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Range);

    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:@acme/foo@1.0.0",
        &gh,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.fetch_spec, "1.0.0");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Version);

    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:@acme/foo@beta",
        &gh,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.fetch_spec, "beta");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn named_registry_preserves_scoped_name_no_jsr_style_rewrite() {
    // Named registries publish the package under its original name —
    // unlike JSR which remaps `@scope/name` to `@jsr/scope__name`.
    let gh = gh_aliases();
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "gh:@acme/foo@1.0.0",
        &gh,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
}

#[test]
fn named_registry_scope_without_name_errors() {
    let gh = gh_aliases();
    for input in ["gh:@acme@^1.0.0", "gh:@acme", "gh:@acme/"] {
        let err =
            parse_named_registry_specifier_to_registry_package_spec(input, &gh, None, "latest")
                .expect_err("scope without name must error");
        assert!(
            matches!(err, ParseNamedRegistrySpecifierError::InvalidPackageName { .. }),
            "got {err:?} for {input:?}",
        );
    }
}

#[test]
fn named_registry_version_only_no_alias_declines() {
    let gh = gh_aliases();
    let result =
        parse_named_registry_specifier_to_registry_package_spec("gh:^1.0.0", &gh, None, "latest");
    assert!(matches!(result, Ok(None)), "got {result:?}");
}

#[test]
fn named_registry_version_only_with_unscoped_alias() {
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "work:^4.0.0",
        &aliases(&["work"]),
        Some("lodash"),
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "lodash");
    assert_eq!(spec.registry_name, "work");
}

#[test]
fn named_registry_unscoped_body_parses() {
    // Arbitrary named registries accept unscoped names, not just
    // GitHub Packages-style scopes.
    let aliases = aliases(&["work"]);
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "work:lodash@^4.0.0",
        &aliases,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "lodash");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Range);

    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "work:lodash",
        &aliases,
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "lodash");
    assert_eq!(spec.spec.fetch_spec, "latest");
    assert_eq!(spec.spec.spec_type, RegistryPackageSpecType::Tag);
}

#[test]
fn named_registry_reports_matched_alias() {
    let spec = parse_named_registry_specifier_to_registry_package_spec(
        "work:@acme/foo@^1.0.0",
        &aliases(&["gh", "work"]),
        None,
        "latest",
    )
    .unwrap()
    .unwrap();
    assert_eq!(spec.spec.name, "@acme/foo");
    assert_eq!(spec.registry_name, "work");
}

#[test]
fn named_registry_unknown_alias_declines() {
    // Unrecognized prefixes must fall through so other resolvers
    // (git, npm, etc.) can try.
    let result = parse_named_registry_specifier_to_registry_package_spec(
        "work:@acme/foo",
        &gh_aliases(),
        None,
        "latest",
    );
    assert!(matches!(result, Ok(None)), "got {result:?}");
}

#[test]
fn named_registry_invalid_name_error_carries_user_alias() {
    let err = parse_named_registry_specifier_to_registry_package_spec(
        "work:@acme",
        &aliases(&["work"]),
        None,
        "latest",
    )
    .expect_err("scope without name must error");
    let message = err.to_string();
    assert!(message.contains("'work:'"), "expected message to mention 'work:', got {message:?}");
}
