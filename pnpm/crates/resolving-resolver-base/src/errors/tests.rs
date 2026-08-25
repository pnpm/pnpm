use std::collections::HashMap;

use chrono::{TimeDelta, Utc};
use miette::Diagnostic;
use pnpm_registry::{DerivedPackuments, Package, PackageDistribution, PackageVersion};

use super::{
    GitResolveError, NoMatchingVersionError, RegistryResponseError, RegistryResponseErrorOptions,
    stringify_date, strip_trailing_semver_suffix,
};

fn make_package(name: &str, versions: &[&str], dist_tags: &[(&str, &str)]) -> Package {
    Package {
        name: name.to_string(),
        dist_tags: dist_tags
            .iter()
            .map(|(tag, version)| ((*tag).to_string(), (*version).to_string()))
            .collect(),
        versions: versions
            .iter()
            .map(|version| {
                (
                    (*version).to_string(),
                    PackageVersion {
                        name: name.to_string(),
                        version: version.parse().expect("parse semver"),
                        dist: PackageDistribution::default(),
                        dependencies: None,
                        dev_dependencies: None,
                        peer_dependencies: None,
                        optional_dependencies: None,
                        peer_dependencies_meta: None,
                        other: HashMap::default(),
                        npm_user: None,
                        deprecated: None,
                    },
                )
            })
            .collect(),
        time: None,
        modified: None,
        etag: None,
        homepage: None,
        mutex: std::sync::Arc::default(),
        derived: DerivedPackuments::default(),
    }
}

fn rendered_help(error: &dyn Diagnostic) -> String {
    error.help().map(|help| help.to_string()).unwrap_or_default()
}

#[test]
fn no_matching_version_reports_the_upstream_code_and_message() {
    let meta = make_package("is-odd", &["1.0.0", "3.0.1"], &[("latest", "3.0.1")]);
    let error = NoMatchingVersionError::new(
        "is-odd@99.99.99".to_string(),
        "https://registry.npmjs.org/".to_string(),
        &meta,
    );

    assert_eq!(
        error.to_string(),
        "No matching version found for is-odd@99.99.99 while fetching it from https://registry.npmjs.org/",
    );
    assert_eq!(
        error.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_NO_MATCHING_VERSION"),
    );
}

#[test]
fn no_matching_version_help_lists_the_latest_release_and_how_to_see_the_rest() {
    let meta = make_package("is-odd", &["1.0.0", "2.0.0", "3.0.1"], &[("latest", "3.0.1")]);
    let error = NoMatchingVersionError::new(
        "is-odd@99.99.99".to_string(),
        "https://registry.npmjs.org/".to_string(),
        &meta,
    );

    let help = rendered_help(&error);
    dbg!(&help);
    assert!(help.contains(r#"The latest release of is-odd is "3.0.1"."#), "{help}");
    assert!(
        help.contains(r#"If you need the full list of all 3 published versions run "pnpm view is-odd versions"."#),
        "{help}",
    );
    assert!(!help.contains("Other releases are:"), "{help}");
}

#[test]
fn no_matching_version_help_lists_the_other_dist_tags_in_a_stable_order() {
    let meta = make_package(
        "is-odd",
        &["1.0.0", "3.0.1", "4.0.0-beta.1"],
        &[("latest", "3.0.1"), ("next", "4.0.0-beta.1"), ("legacy", "1.0.0")],
    );
    let error = NoMatchingVersionError::new(
        "is-odd@99.99.99".to_string(),
        "https://registry.npmjs.org/".to_string(),
        &meta,
    );

    let help = rendered_help(&error);
    dbg!(&help);
    assert!(
        help.contains("Other releases are:\n  * legacy: 1.0.0\n  * next: 4.0.0-beta.1\n"),
        "{help}",
    );
}

#[test]
fn registry_response_error_codes_the_status_and_hints_at_the_missing_package() {
    let error = RegistryResponseError::new(RegistryResponseErrorOptions {
        url: "https://registry.npmjs.org/@repro%2Fpkg-a",
        status: 404,
        status_text: "Not Found",
        pkg_name: "@repro/pkg-a",
        auth_header_value: None,
    });

    assert_eq!(error.to_string(), "GET https://registry.npmjs.org/@repro%2Fpkg-a: Not Found - 404");
    assert_eq!(error.code().map(|code| code.to_string()).as_deref(), Some("ERR_PNPM_FETCH_404"));
    assert_eq!(
        rendered_help(&error),
        "@repro/pkg-a is not in the npm registry, or you have no permission to fetch it.\n\nNo authorization header was set for the request.",
    );
}

#[test]
fn registry_response_error_masks_the_authorization_header() {
    let error = RegistryResponseError::new(RegistryResponseErrorOptions {
        url: "https://registry.npmjs.org/private-pkg",
        status: 404,
        status_text: "Not Found",
        pkg_name: "private-pkg",
        auth_header_value: Some("Bearer npm_0123456789abcdefghij"),
    });

    let help = rendered_help(&error);
    dbg!(&help);
    assert!(help.ends_with("An authorization header was used: Bearer npm_[hidden]"), "{help}");
}

#[test]
fn registry_response_error_hints_only_at_authorization_for_a_403() {
    let error = RegistryResponseError::new(RegistryResponseErrorOptions {
        url: "https://registry.npmjs.org/private-pkg",
        status: 403,
        status_text: "Forbidden",
        pkg_name: "private-pkg",
        auth_header_value: None,
    });

    assert_eq!(error.code().map(|code| code.to_string()).as_deref(), Some("ERR_PNPM_FETCH_403"));
    assert_eq!(rendered_help(&error), "No authorization header was set for the request.");
}

#[test]
fn registry_response_error_leaves_a_500_without_a_hint() {
    let error = RegistryResponseError::new(RegistryResponseErrorOptions {
        url: "https://registry.npmjs.org/pkg",
        status: 500,
        status_text: "Internal Server Error",
        pkg_name: "pkg",
        auth_header_value: Some("Bearer token"),
    });

    assert!(error.hint.is_none(), "{:?}", error.hint);
}

#[test]
fn a_release_older_than_a_day_is_dated_without_a_time_of_day() {
    let rendered = stringify_date("2024-03-15T09:42:13Z").expect("a parsable timestamp");
    dbg!(&rendered);
    assert!(rendered.starts_with("3/1"), "expected a month/day/year date, got {rendered:?}");
    assert!(!rendered.contains(':'), "an old release carries no time of day: {rendered:?}");
}

#[test]
fn a_release_published_within_the_day_carries_its_time_of_day() {
    let an_hour_ago = Utc::now() - TimeDelta::hours(1);
    let rendered = stringify_date(&an_hour_ago.to_rfc3339()).expect("a parsable timestamp");
    dbg!(&rendered);
    assert!(
        rendered.ends_with(" AM") || rendered.ends_with(" PM"),
        "a fresh release carries its time of day: {rendered:?}",
    );
}

#[test]
fn an_unparsable_timestamp_is_dropped_rather_than_echoed() {
    assert_eq!(stringify_date("last tuesday"), None);
}

#[test]
fn a_name_carrying_a_version_suffix_suggests_the_bare_name() {
    assert_eq!(strip_trailing_semver_suffix("lodash@4.17.21"), Some("lodash"));
    assert_eq!(strip_trailing_semver_suffix("lodash4.17.21"), Some("lodash"));
    assert_eq!(strip_trailing_semver_suffix("@scope/pkg@1.0.0"), Some("@scope/pkg"));
}

#[test]
fn a_plain_name_suggests_nothing() {
    assert_eq!(strip_trailing_semver_suffix("lodash"), None);
    assert_eq!(strip_trailing_semver_suffix("@scope/pkg"), None);
    assert_eq!(strip_trailing_semver_suffix("is-odd@latest"), None);
    assert_eq!(strip_trailing_semver_suffix("4.17.21"), None);
}

#[test]
fn an_unreachable_https_remote_explains_the_transport_and_how_to_substitute_it() {
    let err = GitResolveError::new(
        "zkochan/is-negative#next",
        "https://github.com/zkochan/is-negative.git",
        "git ls-remote failed: fatal: unable to access",
    );

    assert_eq!(err.code().expect("code").to_string(), "ERR_PNPM_GIT_RESOLVE_FAILED");
    assert_eq!(
        err.to_string(),
        r#"Failed to resolve git dependency "zkochan/is-negative#next": git ls-remote failed: fatal: unable to access"#,
    );
    let help = err.help().expect("help").to_string();
    assert!(
        help.contains(
            r#"git config --global url."git@github.com:".insteadOf "https://github.com/""#
        ),
        "{help}",
    );
}

#[test]
fn an_unreachable_ssh_remote_carries_no_transport_substitution_hint() {
    let err = GitResolveError::new(
        "git+ssh://git@github.com/foo/bar.git",
        "git+ssh://git@github.com/foo/bar.git",
        "git ls-remote failed: Permission denied (publickey)",
    );

    assert!(err.help().is_none());
}

#[test]
fn the_transport_hint_keeps_a_non_default_port_out_of_the_ssh_remote() {
    let err = GitResolveError::new(
        "https://git.example.com:8443/foo/bar.git",
        "https://git.example.com:8443/foo/bar.git",
        "git ls-remote failed: connection refused",
    );

    let help = err.help().expect("help").to_string();
    assert!(
        help.contains(
            r#"git config --global url."git@git.example.com:".insteadOf "https://git.example.com:8443/""#
        ),
        "{help}",
    );
}

#[test]
fn an_unreachable_remote_redacts_the_credentials_git_echoes_back() {
    let err = GitResolveError::new(
        "git+https://hunter2:x-oauth-basic@github.com/foo/bar.git",
        "https://hunter2:x-oauth-basic@github.com/foo/bar.git",
        "git ls-remote failed: fatal: unable to access 'https://hunter2:x-oauth-basic@github.com/foo/bar.git/'",
    );

    assert!(!err.to_string().contains("hunter2"), "{err}");
    assert!(!err.help().expect("help").to_string().contains("hunter2"));
}

#[test]
fn the_transport_hint_drops_the_scheme_s_own_port() {
    let err = GitResolveError::new(
        "https://github.com:443/foo/bar.git",
        "https://github.com:443/foo/bar.git",
        "git ls-remote failed: connection refused",
    );

    let help = err.help().expect("help").to_string();
    assert!(
        help.contains(
            r#"git config --global url."git@github.com:".insteadOf "https://github.com/""#
        ),
        "{help}",
    );
}
