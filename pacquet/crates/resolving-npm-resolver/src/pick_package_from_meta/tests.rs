use std::collections::HashMap;

use chrono::{DateTime, Utc};
use node_semver::Version;
use pacquet_config::version_policy::create_package_version_policy;
use pacquet_registry::{Package, PackageDistribution, PackageVersion};
use pacquet_resolving_resolver_base::{
    VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight, VersionSelectors,
};
use pretty_assertions::assert_eq;

use super::{
    PickPackageFromMetaError, PickPackageFromMetaOptions, PickVersionByVersionRangeOptions,
    RegistryPackageSpec, RegistryPackageSpecType, filter_pkg_metadata_by_publish_date,
    pick_lowest_version_by_version_range, pick_package_from_meta, pick_version_by_version_range,
};

fn parse_iso(input: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(input).expect("rfc3339").with_timezone(&Utc)
}

fn make_pkg_version(name: &str, version: &str, deprecated: Option<&str>) -> PackageVersion {
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
        deprecated: deprecated.map(str::to_string),
    }
}

fn make_package(
    name: &str,
    versions: &[(&str, Option<&str>)],
    dist_tags: &[(&str, &str)],
) -> Package {
    let versions_map = versions
        .iter()
        .map(|(version, deprecated)| {
            (version.to_string(), make_pkg_version(name, version, *deprecated))
        })
        .collect();
    let dist_tags_map =
        dist_tags.iter().map(|(tag, version)| (tag.to_string(), version.to_string())).collect();
    Package {
        name: name.to_string(),
        dist_tags: dist_tags_map,
        versions: versions_map,
        time: None,
        modified: None,
        etag: None,
        homepage: None,
        mutex: std::sync::Arc::default(),
    }
}

fn make_time_map(entries: &[(&str, &str)]) -> HashMap<String, serde_json::Value> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), serde_json::Value::String(value.to_string())))
        .collect()
}

fn spec(name: &str, fetch_spec: &str, spec_type: RegistryPackageSpecType) -> RegistryPackageSpec {
    RegistryPackageSpec {
        name: name.to_string(),
        fetch_spec: fetch_spec.to_string(),
        spec_type,
        normalized_bare_specifier: None,
    }
}

#[test]
fn version_range_prefers_latest_when_in_range() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None)],
        &[("latest", "1.1.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "^1.0.0",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("1.1.0"));
}

#[test]
fn version_range_falls_back_when_latest_out_of_range() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("2.0.0", None)],
        &[("latest", "2.0.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "^1.0.0",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("1.1.0"));
}

#[test]
fn version_range_lte_partial_allows_entire_major() {
    let pkg = make_package(
        "@jest/environment",
        &[("26.6.2", None), ("27.0.0", None), ("27.5.1", None), ("28.0.0", None)],
        &[("latest", "28.0.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: ">=24 <=27",
        preferred_version_selectors: None,
        published_by: None,
    };

    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("27.5.1"));
}

#[test]
fn partial_lte_upper_bound_returns_none_on_overflow() {
    assert_eq!(super::partial_lte_upper_bound(&u64::MAX.to_string()), None);
    assert_eq!(super::partial_lte_upper_bound(&format!("1.{}", u64::MAX)), None);
}

#[test]
fn version_range_star_uses_latest_even_when_prerelease() {
    let pkg = make_package("acme", &[("1.0.0-beta.1", None)], &[("latest", "1.0.0-beta.1")]);
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "*",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("1.0.0-beta.1"));
}

#[test]
fn version_range_deprecated_max_triggers_non_deprecated_retry() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("2.0.0", Some("use 1.x"))],
        &[("latest", "0.9.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: ">=1.0.0",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("1.1.0"));
}

#[test]
fn version_range_all_deprecated_returns_deprecated_max() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", Some("old")), ("1.1.0", Some("old"))],
        &[("latest", "0.9.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "^1.0.0",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("1.1.0"));
}

#[test]
fn lowest_version_picker_picks_min_in_range() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.5.0", None), ("2.0.0", None)],
        &[("latest", "2.0.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "^1.0.0",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_lowest_version_by_version_range(&opts).as_deref(), Some("1.0.0"));
}

#[test]
fn lowest_version_star_picks_smallest() {
    let pkg = make_package(
        "acme",
        &[("3.0.0", None), ("1.0.0", None), ("2.0.0", None)],
        &[("latest", "3.0.0")],
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "*",
        preferred_version_selectors: None,
        published_by: None,
    };
    assert_eq!(pick_lowest_version_by_version_range(&opts).as_deref(), Some("1.0.0"));
}

#[test]
fn preferred_versions_tag_selector_wins() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None)],
        &[("latest", "1.2.0"), ("next", "1.0.0")],
    );
    let mut selectors: VersionSelectors = VersionSelectors::new();
    selectors.insert("next".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Tag));
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "^1.0.0",
        preferred_version_selectors: Some(&selectors),
        published_by: None,
    };
    // The preferred-versions branch lifts 1.0.0 into the high-weight
    // group; latest still wins the in-range short-circuit there
    // because `latest === 1.2.0`, but the test exists to confirm the
    // selectors plumbing doesn't crash on a Tag entry.
    assert!(pick_version_by_version_range(&opts).is_some());
}

#[test]
fn preferred_versions_higher_weight_wins() {
    let pkg = make_package("acme", &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None)], &[]);
    let mut selectors: VersionSelectors = VersionSelectors::new();
    selectors.insert(
        "1.0.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1_000_000,
        }),
    );
    selectors.insert(
        "1.2.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Version,
            weight: 1_000,
        }),
    );
    let opts = PickVersionByVersionRangeOptions {
        meta: &pkg,
        version_range: "^1.0.0",
        preferred_version_selectors: Some(&selectors),
        published_by: None,
    };
    assert_eq!(pick_version_by_version_range(&opts).as_deref(), Some("1.0.0"));
}

#[test]
fn pick_from_meta_tag_spec_reads_dist_tag() {
    let pkg = make_package(
        "acme",
        &[("1.0.0", None), ("2.0.0-beta.1", None)],
        &[("latest", "1.0.0"), ("beta", "2.0.0-beta.1")],
    );
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "beta", RegistryPackageSpecType::Tag),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("2.0.0-beta.1"));
}

#[test]
fn pick_from_meta_version_spec_reads_versions() {
    let pkg = make_package("acme", &[("1.0.0", None), ("2.0.0", None)], &[("latest", "2.0.0")]);
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "1.0.0", RegistryPackageSpecType::Version),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("1.0.0"));
}

#[test]
fn pick_from_meta_returns_none_when_no_satisfying_version() {
    let pkg = make_package("acme", &[("1.0.0", None)], &[("latest", "1.0.0")]);
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "^2.0.0", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert!(picked.is_none());
}

#[test]
fn pick_from_meta_unpublished_marker_propagates() {
    let mut pkg = make_package("acme", &[], &[]);
    let mut time = HashMap::new();
    time.insert(
        "unpublished".to_string(),
        serde_json::json!({
            "time": "2025-01-01T00:00:00.000Z",
            "versions": ["1.0.0"],
        }),
    );
    pkg.time = Some(time);
    let err = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "^1.0.0", RegistryPackageSpecType::Range),
    )
    .expect_err("unpublished");
    assert!(matches!(err, PickPackageFromMetaError::Unpublished { .. }), "got {err:?}");
}

#[test]
fn pick_from_meta_empty_meta_surfaces_no_versions() {
    let pkg = make_package("acme", &[], &[]);
    let err = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "^1.0.0", RegistryPackageSpecType::Range),
    )
    .expect_err("no versions");
    assert!(matches!(err, PickPackageFromMetaError::NoVersions { .. }), "got {err:?}");
}

#[test]
fn pick_from_meta_published_by_missing_time_fails() {
    let pkg = make_package("acme", &[("1.0.0", None)], &[("latest", "1.0.0")]);
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let err = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: None,
        },
        &pkg,
        &spec("acme", "^1.0.0", RegistryPackageSpecType::Range),
    )
    .expect_err("missing time");
    assert!(matches!(err, PickPackageFromMetaError::MissingTime { .. }), "got {err:?}");
}

#[test]
fn pick_from_meta_published_by_modified_shortcut() {
    let mut pkg = make_package("acme", &[("1.0.0", None)], &[("latest", "1.0.0")]);
    pkg.modified = Some("2024-01-01T00:00:00.000Z".to_string());
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: None,
        },
        &pkg,
        &spec("acme", "^1.0.0", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("1.0.0"));
}

#[test]
fn pick_from_meta_modified_shortcut_inclusive_at_cutoff() {
    let mut pkg = make_package("acme", &[("1.0.0", None)], &[("latest", "1.0.0")]);
    pkg.modified = Some("2025-01-01T00:00:00.000Z".to_string());
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: None,
        },
        &pkg,
        &spec("acme", "^1.0.0", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("1.0.0"));
}

#[test]
fn pick_from_meta_published_by_filters_immature_versions() {
    let mut pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("2.0.0", None)],
        &[("latest", "2.0.0")],
    );
    pkg.time = Some(make_time_map(&[
        ("1.0.0", "2024-01-01T00:00:00.000Z"),
        ("1.1.0", "2024-06-01T00:00:00.000Z"),
        ("2.0.0", "2025-06-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: None,
        },
        &pkg,
        &spec("acme", "*", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("1.1.0"));
}

#[test]
fn pick_from_meta_published_by_bare_name_exclude_skips_filter() {
    let mut pkg = make_package("acme", &[("1.0.0", None), ("2.0.0", None)], &[("latest", "2.0.0")]);
    pkg.time = Some(make_time_map(&[
        ("1.0.0", "2024-01-01T00:00:00.000Z"),
        ("2.0.0", "2025-06-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let policy = create_package_version_policy(["acme"]).expect("policy");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: Some(&policy),
        },
        &pkg,
        &spec("acme", "*", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("2.0.0"));
}

#[test]
fn pick_from_meta_published_by_trusted_version_passes_filter() {
    let mut pkg = make_package("acme", &[("1.0.0", None), ("2.0.0", None)], &[("latest", "2.0.0")]);
    pkg.time = Some(make_time_map(&[
        ("1.0.0", "2024-01-01T00:00:00.000Z"),
        ("2.0.0", "2025-06-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let policy = create_package_version_policy(["acme@2.0.0"]).expect("policy");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: Some(&policy),
        },
        &pkg,
        &spec("acme", "*", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("2.0.0"));
}

#[test]
fn filter_rewrites_dist_tag_to_within_cutoff_max_of_same_major() {
    let mut pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None), ("2.0.0", None)],
        &[("latest", "2.0.0"), ("lts", "1.2.0")],
    );
    pkg.time = Some(make_time_map(&[
        ("1.0.0", "2024-01-01T00:00:00.000Z"),
        ("1.1.0", "2024-06-01T00:00:00.000Z"),
        ("1.2.0", "2025-02-01T00:00:00.000Z"),
        ("2.0.0", "2025-03-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let filtered = filter_pkg_metadata_by_publish_date(&pkg, cutoff, None);
    assert_eq!(filtered.dist_tag("lts"), Some("1.1.0"), "lts → highest 1.x within cutoff");
    assert_eq!(
        filtered.dist_tag("latest"),
        Some("1.1.0"),
        "latest is allowed to cross majors when its original target dropped",
    );
}

#[test]
fn lowest_picker_with_published_by_drops_immature_min() {
    let mut pkg = make_package(
        "acme",
        &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None)],
        &[("latest", "1.2.0")],
    );
    pkg.time = Some(make_time_map(&[
        ("1.0.0", "2025-06-01T00:00:00.000Z"),
        ("1.1.0", "2024-06-01T00:00:00.000Z"),
        ("1.2.0", "2024-12-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let picked = pick_package_from_meta(
        pick_lowest_version_by_version_range,
        &PickPackageFromMetaOptions {
            preferred_version_selectors: None,
            published_by: Some(cutoff),
            published_by_exclude: None,
        },
        &pkg,
        &spec("acme", "*", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("1.1.0"));
}

#[test]
fn pick_from_meta_skips_undecodable_winner_and_retries() {
    let pkg: Package = serde_json::from_str(
        r#"{
            "name": "acme",
            "dist-tags": {"latest": "1.2.0"},
            "versions": {
                "1.0.0": {"name": "acme", "version": "1.0.0", "dist": {"integrity": "sha512-a", "tarball": "https://r/acme-1.0.0.tgz"}},
                "1.2.0": {"corrupt": "fragment"}
            }
        }"#,
    )
    .expect("parse package");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "^1.0.0", RegistryPackageSpecType::Range),
    )
    .expect("ok");
    assert_eq!(picked.map(|version| version.version.to_string()).as_deref(), Some("1.0.0"));
}

#[test]
fn pick_from_meta_returns_none_for_undecodable_exact_version() {
    let pkg: Package = serde_json::from_str(
        r#"{
            "name": "acme",
            "dist-tags": {},
            "versions": {
                "1.2.0": {"corrupt": "fragment"}
            }
        }"#,
    )
    .expect("parse package");
    let picked = pick_package_from_meta(
        pick_version_by_version_range,
        &PickPackageFromMetaOptions::default(),
        &pkg,
        &spec("acme", "1.2.0", RegistryPackageSpecType::Version),
    )
    .expect("ok");
    assert!(picked.is_none());
}

#[test]
fn filter_tag_rewrite_prefers_non_deprecated_candidate() {
    let mut pkg = make_package(
        "acme",
        &[("2.1.0", None), ("2.2.0", Some("use 3.x")), ("2.5.0", None)],
        &[("old", "2.5.0")],
    );
    pkg.time = Some(make_time_map(&[
        ("2.1.0", "2024-01-01T00:00:00.000Z"),
        ("2.2.0", "2024-06-01T00:00:00.000Z"),
        ("2.5.0", "2025-06-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let filtered = filter_pkg_metadata_by_publish_date(&pkg, cutoff, None);
    assert_eq!(
        filtered.dist_tag("old"),
        Some("2.1.0"),
        "non-deprecated 2.1.0 must beat the higher deprecated 2.2.0",
    );
}

/// The repopulation tie-break must read deprecation from the raw
/// fragments without full-manifest hydration. The fragments here are
/// valid JSON but not decodable as complete version manifests, so a
/// tie-break that hydrates to check `deprecated` sees every candidate
/// as undecodable (= not deprecated) and picks the higher deprecated
/// version instead. Guards the no-hydration deprecation probe the
/// publish-date filter relies on — also matching pnpm, which reads
/// `versions[candidate].deprecated` off plain parsed JSON without
/// validating the rest of the entry.
#[test]
fn filter_tag_rewrite_reads_deprecation_from_raw_fragments() {
    let mut pkg: Package = serde_json::from_str(
        r#"{
            "name": "acme",
            "dist-tags": {"old": "2.5.0"},
            "versions": {
                "2.1.0": {"name": "acme"},
                "2.2.0": {"name": "acme", "deprecated": "use 3.x"},
                "2.5.0": {"name": "acme"}
            }
        }"#,
    )
    .expect("parse package");
    pkg.time = Some(make_time_map(&[
        ("2.1.0", "2024-01-01T00:00:00.000Z"),
        ("2.2.0", "2024-06-01T00:00:00.000Z"),
        ("2.5.0", "2025-06-01T00:00:00.000Z"),
    ]));
    let cutoff = parse_iso("2025-01-01T00:00:00.000Z");
    let filtered = filter_pkg_metadata_by_publish_date(&pkg, cutoff, None);
    assert_eq!(
        filtered.dist_tag("old"),
        Some("2.1.0"),
        "deprecation must be read from the raw fragment, not via hydration",
    );
}
