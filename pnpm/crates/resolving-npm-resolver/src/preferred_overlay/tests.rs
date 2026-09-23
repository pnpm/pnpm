use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use node_semver::Version;
use pnpm_config::version_policy::create_package_version_policy;
use pnpm_registry::{DerivedPackuments, Package, PackageDistribution, PackageVersion};
use pnpm_resolving_resolver_base::{
    DIRECT_DEP_SELECTOR_WEIGHT, PreferredVersions, PreferredVersionsOverlay, ResolveOptions,
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight, VersionSelectors,
};
use pretty_assertions::assert_eq;

use super::{held_back_preferred, overlay_merged_selectors};
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
        derived: DerivedPackuments::default(),
    }
}

fn range_spec() -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: "foo".to_string(),
        fetch_spec: ">=2.1.3 <3.0.0-0".to_string(),
        spec_type: RegistryPackageSpecType::Range,
        revision: None,
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
    ResolveOptions {
        refresh: pnpm_resolving_resolver_base::ResolutionRefreshOptions {
            update_requested: true,
            ..Default::default()
        },
        ..ResolveOptions::default()
    }
}

// <https://github.com/pnpm/pnpm/issues/13071>
#[test]
fn no_warning_when_minimum_release_age_is_the_reason_for_the_held_back_pick() {
    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by: Some(parse_iso("2026-07-01T00:00:00.000Z")),
            ..update_opts().policy
        },
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
fn warns_when_the_newer_version_is_mature_under_the_cutoff() {
    let selectors = VersionSelectors::from([(
        "2.1.3".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1000,
        }),
    )]);
    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by: Some(parse_iso("2026-08-01T00:00:00.000Z")),
            ..update_opts().policy
        },
        ..update_opts()
    };
    let preferred =
        held_back_preferred(&opts, &range_spec(), Some(&selectors), &make_package(), "2.1.3");
    assert_eq!(preferred, Some("2.1.4".to_string()));
}

#[test]
fn excluded_package_keeps_the_unfiltered_baseline() {
    let policy = create_package_version_policy(["foo"]).expect("policy");
    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by: Some(parse_iso("2026-07-01T00:00:00.000Z")),
            published_by_exclude: Some(policy),
            ..update_opts().policy
        },
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

#[test]
fn version_trusted_by_exact_version_stays_in_the_baseline() {
    let policy = create_package_version_policy(["foo@2.1.4"]).expect("policy");
    let opts = ResolveOptions {
        policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions {
            published_by: Some(parse_iso("2026-07-01T00:00:00.000Z")),
            published_by_exclude: Some(policy),
            ..update_opts().policy
        },
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

#[test]
fn overlay_merged_selectors_bumps_weight_of_existing_version() {
    let mut preferred = PreferredVersions::new();
    let mut selectors = VersionSelectors::default();
    selectors.insert(
        "100.0.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1000,
        }),
    );
    selectors.insert(
        "100.1.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1000,
        }),
    );
    preferred.insert("foo".to_string(), selectors);

    let overlay = PreferredVersionsOverlay::layer_direct(
        None,
        BTreeMap::from([("foo".to_string(), vec!["100.0.0".to_string()])]),
    );

    let opts = ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            preferred_versions: Arc::new(preferred),
            preferred_versions_overlay: overlay,
            ..Default::default()
        },
        ..Default::default()
    };

    let merged = overlay_merged_selectors(&opts, "foo").expect("overlay merged");
    let foo_100_0 = merged.get("100.0.0").expect("has 100.0.0");
    let foo_100_1 = merged.get("100.1.0").expect("has 100.1.0");

    assert_eq!(
        foo_100_0,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1000 + DIRECT_DEP_SELECTOR_WEIGHT,
        }),
    );
    assert_eq!(
        foo_100_1,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1000,
        }),
    );
}

#[test]
fn overlay_merged_selectors_inserts_new_version_with_direct_dep_weight() {
    let overlay = PreferredVersionsOverlay::layer_direct(
        None,
        BTreeMap::from([("foo".to_string(), vec!["100.0.0".to_string()])]),
    );
    let opts = ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            preferred_versions_overlay: overlay,
            ..Default::default()
        },
        ..Default::default()
    };

    let merged = overlay_merged_selectors(&opts, "foo").expect("overlay merged");
    let foo_100_0 = merged.get("100.0.0").expect("has 100.0.0");
    assert_eq!(
        foo_100_0,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: DIRECT_DEP_SELECTOR_WEIGHT,
        }),
    );
}

#[test]
fn overlay_merged_selectors_returns_none_when_overlay_is_absent_or_empty() {
    let opts = ResolveOptions::default();
    assert_eq!(overlay_merged_selectors(&opts, "foo"), None);

    let overlay = PreferredVersionsOverlay::layer(
        None,
        BTreeMap::from([("bar".to_string(), vec!["1.0.0".to_string()])]),
    );
    let opts_with_bar = ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            preferred_versions_overlay: overlay,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(overlay_merged_selectors(&opts_with_bar, "foo"), None);
}

#[test]
fn overlay_merged_selectors_weights_root_layer_higher_than_descendant_layer() {
    let direct_layer = PreferredVersionsOverlay::layer_direct(
        None,
        BTreeMap::from([("foo".to_string(), vec!["100.0.0".to_string()])]),
    );
    let child_layer = PreferredVersionsOverlay::layer(
        direct_layer,
        BTreeMap::from([("bar".to_string(), vec!["200.0.0".to_string()])]),
    );
    let opts = ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            preferred_versions_overlay: child_layer,
            ..Default::default()
        },
        ..Default::default()
    };

    let merged_foo = overlay_merged_selectors(&opts, "foo").expect("overlay merged foo");
    let foo_entry = merged_foo.get("100.0.0").expect("has 100.0.0");
    assert_eq!(
        foo_entry,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: DIRECT_DEP_SELECTOR_WEIGHT,
        }),
    );

    let merged_bar = overlay_merged_selectors(&opts, "bar").expect("overlay merged bar");
    let bar_entry = merged_bar.get("200.0.0").expect("has 200.0.0");
    assert_eq!(
        bar_entry,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1,
        }),
    );
}

#[test]
fn overlay_descendant_without_parent_layer_retains_descendant_weight() {
    let child_layer = PreferredVersionsOverlay::layer(
        None,
        BTreeMap::from([("bar".to_string(), vec!["200.0.0".to_string()])]),
    );
    let opts = ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            preferred_versions_overlay: child_layer,
            ..Default::default()
        },
        ..Default::default()
    };

    let merged_bar = overlay_merged_selectors(&opts, "bar").expect("overlay merged bar");
    let bar_entry = merged_bar.get("200.0.0").expect("has 200.0.0");
    assert_eq!(
        bar_entry,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1,
        }),
    );
}

#[test]
fn overlay_duplicate_version_across_layers_retains_maximum_weight() {
    let direct_layer = PreferredVersionsOverlay::layer_direct(
        None,
        BTreeMap::from([("foo".to_string(), vec!["1.0.0".to_string()])]),
    );
    let child_layer = PreferredVersionsOverlay::layer(
        direct_layer,
        BTreeMap::from([("foo".to_string(), vec!["1.0.0".to_string()])]),
    );
    let opts = ResolveOptions {
        version: pnpm_resolving_resolver_base::VersionSelectionOptions {
            preferred_versions_overlay: child_layer,
            ..Default::default()
        },
        ..Default::default()
    };

    let merged_foo = overlay_merged_selectors(&opts, "foo").expect("overlay merged foo");
    let foo_entry = merged_foo.get("1.0.0").expect("has 1.0.0");
    assert_eq!(
        foo_entry,
        &VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: DIRECT_DEP_SELECTOR_WEIGHT,
        }),
    );
}
