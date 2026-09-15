use super::{
    BTreeMap, Config, ConfigAuditLevel, HashMap, HashSet, InstalledPackages, PackumentPublishInfo,
    Range, RangeSpecStyle, age_cutoff, caret_range_for_patched, classify_for_update,
    create_overrides, deprecate, filter_advisories_for_fix, fix_advisory,
    format_fix_with_update_output, minimum_release_age_excludes, publish_times,
    report_fixed_remaining, report_of,
};

#[test]
fn caret_range_for_patched_uses_minimum_with_caret() {
    assert_eq!(caret_range_for_patched(">=2.0.0"), "^2.0.0");
    assert_eq!(caret_range_for_patched(">=1.2.3"), "^1.2.3");
    // A non-inferred range is passed through unchanged.
    assert_eq!(caret_range_for_patched("not-a-range"), "not-a-range");
}

#[test]
fn create_overrides_sorts_and_skips_unfixable() {
    let advisories = BTreeMap::from([
        (
            "1".to_string(),
            fix_advisory(1, "zoo", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        ),
        (
            "2".to_string(),
            fix_advisory(2, "abc", "<1.5.0", Some(">=1.5.0"), ConfigAuditLevel::Low, "GHSA-b"),
        ),
        // No patched range: cannot produce an override.
        (
            "3".to_string(),
            fix_advisory(3, "unfixable", ">=0.0.0", None, ConfigAuditLevel::High, "GHSA-c"),
        ),
    ]);

    let overrides = create_overrides(&advisories, RangeSpecStyle::Major);

    assert_eq!(
        overrides.into_iter().collect::<Vec<_>>(),
        vec![
            ("abc@<1.5.0".to_string(), "^1.5.0".to_string()),
            ("zoo@<2.0.0".to_string(), "^2.0.0".to_string()),
        ],
    );
}

#[test]
fn filter_advisories_for_fix_drops_below_level_and_ignored() {
    let report = report_of(vec![
        fix_advisory(1, "high", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-high-1"),
        fix_advisory(2, "info", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::Info, "GHSA-info-1"),
        fix_advisory(
            3,
            "ignored",
            "<2.0.0",
            Some(">=2.0.0"),
            ConfigAuditLevel::Critical,
            "GHSA-skip-1",
        ),
    ]);
    let mut config = Config::default();
    config.audit_config.ignore_ghsas = vec!["GHSA-skip-1".to_string()];

    let filtered = filter_advisories_for_fix(&report, ConfigAuditLevel::Low, &config);

    assert!(filtered.contains_key("1"), "high stays");
    assert!(!filtered.contains_key("2"), "info is below the low audit level");
    assert!(!filtered.contains_key("3"), "ignored GHSA is dropped");
}

#[test]
fn format_fix_with_update_output_lists_fixed_and_remaining() {
    let advisories = BTreeMap::from([
        (
            "1".to_string(),
            fix_advisory(
                1,
                "fixed-pkg",
                "<2.0.0",
                Some(">=2.0.0"),
                ConfigAuditLevel::High,
                "GHSA-a",
            ),
        ),
        (
            "2".to_string(),
            fix_advisory(
                2,
                "stuck-pkg",
                "<2.0.0",
                Some(">=2.0.0"),
                ConfigAuditLevel::Low,
                "GHSA-b",
            ),
        ),
    ]);

    let output = format_fix_with_update_output(&[1], &[2], &advisories);

    assert!(output.contains("1 vulnerability was fixed, 1 vulnerability remains."));
    assert!(output.contains("The fixed vulnerabilities are:"));
    assert!(output.contains("fixed-pkg"));
    assert!(output.contains("The remaining vulnerabilities are:"));
    assert!(output.contains("stuck-pkg"));
}

#[test]
fn minimum_release_age_excludes_uses_patched_minimums_and_skips_unfixable() {
    let advisories = report_of(vec![
        fix_advisory(1, "foo", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        // No patched range: contributes no exclude entry.
        fix_advisory(2, "bar", ">=0.0.0", None, ConfigAuditLevel::High, "GHSA-b"),
    ])
    .advisories;

    // No publish-time information at all: fail open, keep the entry.
    let excludes = minimum_release_age_excludes(&advisories, &HashMap::new(), age_cutoff())
        .expect("compute excludes");

    assert_eq!(excludes, vec!["foo@2.0.0".to_string()]);
}

#[test]
fn minimum_release_age_excludes_drops_versions_older_than_the_cutoff() {
    let advisories = report_of(vec![
        fix_advisory(1, "old", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        fix_advisory(2, "fresh", "<3.0.0", Some(">=3.0.0"), ConfigAuditLevel::High, "GHSA-b"),
    ])
    .advisories;
    let times = HashMap::from([
        (
            "old".to_string(),
            Some(PackumentPublishInfo {
                time: HashMap::from([("2.0.0".to_string(), "2020-01-01T00:00:00Z".to_string())]),
                deprecated: HashSet::new(),
            }),
        ),
        (
            "fresh".to_string(),
            Some(PackumentPublishInfo {
                time: HashMap::from([("3.0.0".to_string(), "2026-06-01T00:00:00Z".to_string())]),
                deprecated: HashSet::new(),
            }),
        ),
    ]);

    let excludes =
        minimum_release_age_excludes(&advisories, &times, age_cutoff()).expect("compute excludes");

    assert_eq!(excludes, vec!["fresh@3.0.0".to_string()], "the old fix needs no bypass");
}

#[test]
fn minimum_release_age_excludes_drops_versions_published_exactly_at_the_cutoff() {
    let advisories = report_of(vec![fix_advisory(
        1,
        "foo",
        "<2.0.0",
        Some(">=2.0.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;
    let times = publish_times("foo", &[("2.0.0", "2026-01-01T00:00:00Z")]);

    let excludes =
        minimum_release_age_excludes(&advisories, &times, age_cutoff()).expect("compute excludes");

    assert!(excludes.is_empty(), "a version at the cutoff is mature: {excludes:?}");
}

#[test]
fn minimum_release_age_excludes_keeps_versions_with_unknown_publish_times() {
    let advisories = report_of(vec![
        fix_advisory(2, "garbled", "<3.0.0", Some(">=3.0.0"), ConfigAuditLevel::High, "GHSA-b"),
        fix_advisory(3, "unfetchable", "<4.0.0", Some(">=4.0.0"), ConfigAuditLevel::High, "GHSA-c"),
    ])
    .advisories;
    let times = HashMap::from([
        // The timestamp does not parse.
        (
            "garbled".to_string(),
            Some(PackumentPublishInfo {
                time: HashMap::from([("3.0.0".to_string(), "not-a-date".to_string())]),
                deprecated: HashSet::new(),
            }),
        ),
        // The fetch failed outright.
        ("unfetchable".to_string(), None),
    ]);

    let excludes =
        minimum_release_age_excludes(&advisories, &times, age_cutoff()).expect("compute excludes");

    assert_eq!(
        excludes,
        vec!["garbled@3.0.0".to_string(), "unfetchable@4.0.0".to_string()],
        "unknown publish times fail open so a fresh fix stays installable",
    );
}

#[test]
fn minimum_release_age_excludes_drops_versions_missing_from_the_packument() {
    let advisories = report_of(vec![fix_advisory(
        1,
        "absent",
        "<2.0.0",
        Some(">=2.0.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;
    // The packument was fetched but names no such version: the patched
    // release was never published.
    let times = publish_times("absent", &[("1.0.0", "2020-01-01T00:00:00Z")]);

    let excludes =
        minimum_release_age_excludes(&advisories, &times, age_cutoff()).expect("compute excludes");

    assert!(excludes.is_empty(), "an unpublished patched version gets no bypass: {excludes:?}");
}

#[test]
fn minimum_release_age_excludes_uses_lowest_published_version_satisfying_range() {
    let advisories = report_of(vec![fix_advisory(
        1,
        "foo",
        "<2.0.0",
        Some(">=2.0.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;
    // 2.0.0 was never published; 2.0.1 is the lowest published version
    // satisfying >=2.0.0 and it is fresh enough to need the bypass.
    let times = publish_times("foo", &[("2.0.1", "2026-06-01T00:00:00Z")]);

    let excludes =
        minimum_release_age_excludes(&advisories, &times, age_cutoff()).expect("compute excludes");

    assert_eq!(
        excludes,
        vec!["foo@2.0.1".to_string()],
        "the lowest published satisfying version is used",
    );
}

#[test]
fn minimum_release_age_excludes_skips_deprecated_versions() {
    let advisories = report_of(vec![fix_advisory(
        1,
        "lodash-es",
        "<4.18.0",
        Some(">=4.18.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;
    // 4.18.0 is deprecated; 4.18.1 is the lowest non-deprecated published
    // version satisfying >=4.18.0.
    let mut info = publish_times(
        "lodash-es",
        &[("4.18.0", "2026-06-01T00:00:00Z"), ("4.18.1", "2026-06-01T01:00:00Z")],
    );
    deprecate(&mut info, "lodash-es", "4.18.0");

    let excludes =
        minimum_release_age_excludes(&advisories, &info, age_cutoff()).expect("compute excludes");

    assert_eq!(
        excludes,
        vec!["lodash-es@4.18.1".to_string()],
        "the deprecated version is skipped in favor of the next non-deprecated one",
    );
}

#[test]
fn minimum_release_age_excludes_skips_deprecated_versions_spelled_differently() {
    let advisories = report_of(vec![fix_advisory(
        1,
        "lodash-es",
        "<4.18.0",
        Some(">=4.18.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;
    // The `time` map spells 4.18.0 with a leading `v` while `versions` — the
    // source of the deprecation set — does not.
    let mut info = publish_times(
        "lodash-es",
        &[("v4.18.0", "2026-06-01T00:00:00Z"), ("4.18.1", "2026-06-01T01:00:00Z")],
    );
    deprecate(&mut info, "lodash-es", "4.18.0");

    let excludes =
        minimum_release_age_excludes(&advisories, &info, age_cutoff()).expect("compute excludes");

    assert_eq!(
        excludes,
        vec!["lodash-es@4.18.1".to_string()],
        "deprecation is matched on the parsed version, not the raw packument key",
    );
}

#[test]
fn minimum_release_age_excludes_prefers_a_stable_release_over_a_lower_prerelease() {
    let advisories = report_of(vec![fix_advisory(
        1,
        "foo",
        "<=1.9.9",
        Some(">=1.9.10"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;
    // 2.0.0-beta.1 sorts below 2.0.0 but is not a release users should be
    // pointed at as the fix.
    let times = publish_times(
        "foo",
        &[("2.0.0-beta.1", "2026-06-01T00:00:00Z"), ("2.0.0", "2026-06-01T01:00:00Z")],
    );

    let excludes =
        minimum_release_age_excludes(&advisories, &times, age_cutoff()).expect("compute excludes");

    assert_eq!(
        excludes,
        vec!["foo@2.0.0".to_string()],
        "the stable release outranks the lower-sorting prerelease",
    );
}

#[test]
fn classify_for_update_routes_unparsable_ranges_to_remaining() {
    let advisories = report_of(vec![
        fix_advisory(1, "ok", "<2.0.0", Some(">=2.0.0"), ConfigAuditLevel::High, "GHSA-a"),
        fix_advisory(2, "any", ">=0.0.0", None, ConfigAuditLevel::High, "GHSA-b"),
        // An untrusted registry could send a range we can't parse.
        fix_advisory(3, "broken", "not a range", None, ConfigAuditLevel::High, "GHSA-c"),
        // ...or pad an unfixable sentinel with whitespace.
        fix_advisory(4, "padded", "  >=0.0.0  ", None, ConfigAuditLevel::High, "GHSA-d"),
    ])
    .advisories;

    let classification = classify_for_update(&advisories);

    assert!(classification.vulnerabilities.contains_key("ok"));
    assert!(classification.unfixable.contains_key("any"));
    assert!(
        classification.unfixable.contains_key("padded"),
        "a padded sentinel is still unfixable",
    );
    assert_eq!(classification.unparsable, vec![3], "an unparsable range must not be dropped");
}

#[test]
fn classify_for_update_trims_module_names() {
    // A whitespace-padded module name from an untrusted registry must key the
    // guard and the installed-name comparison by the clean package name.
    let advisories = report_of(vec![fix_advisory(
        1,
        "  vulnerable  ",
        "<2.0.0",
        Some(">=2.0.0"),
        ConfigAuditLevel::High,
        "GHSA-a",
    )])
    .advisories;

    let classification = classify_for_update(&advisories);

    assert!(classification.vulnerabilities.contains_key("vulnerable"));
    assert!(!classification.vulnerabilities.contains_key("  vulnerable  "));
}

#[test]
fn report_fixed_remaining_keeps_non_semver_installed_packages_remaining() {
    let mut vulnerabilities: HashMap<String, Vec<(u64, Range)>> = HashMap::new();
    vulnerabilities.insert("gone".to_string(), vec![(1, "<2.0.0".parse().unwrap())]);
    vulnerabilities.insert("bumped".to_string(), vec![(2, "<2.0.0".parse().unwrap())]);
    vulnerabilities.insert("non-semver".to_string(), vec![(3, "<2.0.0".parse().unwrap())]);

    // `non-semver` is still installed but only under a non-semver key, so its
    // version can't be range-checked.
    let installed = InstalledPackages {
        names: ["bumped".to_string(), "non-semver".to_string()].into_iter().collect(),
        versions: HashMap::from([("bumped".to_string(), vec!["2.0.0".parse().unwrap()])]),
    };

    let (fixed, remaining) =
        report_fixed_remaining(&vulnerabilities, &HashMap::new(), &[], &installed);

    assert!(fixed.contains(&1), "an absent package is fixed");
    assert!(fixed.contains(&2), "a package bumped out of range is fixed");
    assert!(
        remaining.contains(&3),
        "a package surviving only under a non-semver key cannot be proven fixed",
    );
    assert!(!fixed.contains(&3));
}
