use std::collections::HashMap;

use chrono::{DateTime, Utc};
use node_semver::Version;
use pacquet_config::version_policy::create_package_version_policy;
use pacquet_registry::{Package, PackageDistribution, PackageVersion};
use pacquet_resolving_resolver_base::{
    ResolveOptions, VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
    VersionSelectors,
};
use pretty_assertions::assert_eq;

use super::held_back_preferred;
use crate::pick_package_from_meta::{RegistryPackageSpec, RegistryPackageSpecType};

fn parse_iso(input: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(input).expect("rfc3339").with_timezone(&Utc)
}

fn make_pkg_version(name: &str, version: &str) -> PackageVersion {
    PackageVersion {
        name: name.to_string(),
        version: version.parse::<Version>().expect("parse semver"),
        dist: PackageDistribution::default(),
        dependencies: None,
        dev_dependencies: None,
        peer_dependencies: None,
        optional_dependencies: None,
        peer_dependencies_meta: None,
        other: HashMap::default(),
        npm_user: None,
        deprecated: None,
    }
}

/// `foo` with a mature `2.1.3` and a `2.1.4` published inside the
/// `2026-07-01` cutoff used by the tests below.
fn make_package() -> Package {
    Package {
        name: "foo".to_string(),
        dist_tags: HashMap::from([("latest".to_string(), "2.1.4".to_string())]),
        versions: ["2.1.3", "2.1.4"]
            .into_iter()
            .map(|version| (version.to_string(), make_pkg_version("foo", version)))
            .collect(),
        time: Some(HashMap::from([
            (
                "2.1.3".to_string(),
                serde_json::Value::String("2026-01-01T00:00:00.000Z".to_string()),
            ),
            (
                "2.1.4".to_string(),
                serde_json::Value::String("2026-07-14T12:00:00.000Z".to_string()),
            ),
        ])),
        modified: None,
        etag: None,
        homepage: None,
        mutex: std::sync::Arc::default(),
    }
}

fn range_spec() -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: "foo".to_string(),
        fetch_spec: ">=2.1.3 <3.0.0-0".to_string(),
        spec_type: RegistryPackageSpecType::Range,
        normalized_bare_specifier: None,
    }
}

fn range_selectors() -> VersionSelectors {
    VersionSelectors::from([(
        ">=2.1.3 <3.0.0-0".to_string(),
        VersionSelectorEntry::Plain(VersionSelectorType::Range),
    )])
}

fn update_opts() -> ResolveOptions {
    ResolveOptions { update_requested: true, ..ResolveOptions::default() }
}

// <https://github.com/pnpm/pnpm/issues/13071>
#[test]
fn no_warning_when_minimum_release_age_is_the_reason_for_the_held_back_pick() {
    let opts = ResolveOptions {
        published_by: Some(parse_iso("2026-07-01T00:00:00.000Z")),
        ..update_opts()
    };
    let preferred = held_back_preferred(
        &opts,
        &range_spec(),
        Some(&range_selectors()),
        &make_package(),
        "2.1.3",
    );
    assert_eq!(preferred, None);
}

#[test]
fn warns_when_a_manifest_pin_is_the_reason_for_the_held_back_pick() {
    let selectors = VersionSelectors::from([(
        "2.1.3".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1000,
        }),
    )]);
    let preferred = held_back_preferred(
        &update_opts(),
        &range_spec(),
        Some(&selectors),
        &make_package(),
        "2.1.3",
    );
    assert_eq!(preferred, Some("2.1.4".to_string()));
}

#[test]
fn excluded_package_keeps_the_unfiltered_baseline() {
    let policy = create_package_version_policy(["foo"]).expect("policy");
    let opts = ResolveOptions {
        published_by: Some(parse_iso("2026-07-01T00:00:00.000Z")),
        published_by_exclude: Some(policy),
        ..update_opts()
    };
    let preferred = held_back_preferred(
        &opts,
        &range_spec(),
        Some(&range_selectors()),
        &make_package(),
        "2.1.3",
    );
    assert_eq!(preferred, Some("2.1.4".to_string()));
}
