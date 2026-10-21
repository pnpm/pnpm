use super::{
    CacheHitCheck, DiskFirst, MirrorShortcuts, RouteInputs, RouteRequest,
    UNVALIDATED_MIRROR_MAX_AGE, fetch_failure_falls_back, unvalidated_mirror_is_fresh,
};
use crate::{
    PackagePickPolicy, mirror::MetaHeaders, pick_package_from_meta::RegistryPackageSpecType,
};
use chrono::{DateTime, TimeDelta, Utc};
use pnpm_config::{NetworkPolicy, TrustPolicy, version_policy::create_package_version_policy};
use pretty_assertions::assert_eq;

const PKG_NAME: &str = "acme";

const NO_POLICY: PackagePickPolicy<'static> = PackagePickPolicy {
    published_by: None,
    fallback_published_by: None,
    published_by_exclude: None,
    trust_policy: None,
};

fn headers(etag: Option<&str>, uncacheable: bool) -> MetaHeaders {
    MetaHeaders { etag: etag.map(str::to_string), uncacheable, ..MetaHeaders::default() }
}

fn cutoff() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().into()
}

fn release_age_policy() -> PackagePickPolicy<'static> {
    PackagePickPolicy { published_by: Some(cutoff()), ..NO_POLICY }
}

/// An online range pick with no option that needs fresh metadata, against a
/// public route.
fn range_pick<'a>(
    policy: &'a PackagePickPolicy<'a>,
    mirror: Option<&'a MetaHeaders>,
) -> RouteInputs<'a> {
    RouteInputs {
        network: NetworkPolicy::Online,
        request: RouteRequest {
            spec_type: RegistryPackageSpecType::Range,
            include_latest_tag: false,
            update_checksums: false,
            refresh_metadata: false,
            pick_lowest_version: false,
            has_blocked_versions: false,
        },
        policy,
        pkg_name: PKG_NAME,
        public_scope: true,
        mirror,
    }
}

fn with_request(
    inputs: RouteInputs<'_>,
    request: impl FnOnce(&mut RouteRequest),
) -> RouteInputs<'_> {
    let mut inputs = inputs;
    request(&mut inputs.request);
    inputs
}

fn version_pick<'a>(
    policy: &'a PackagePickPolicy<'a>,
    mirror: Option<&'a MetaHeaders>,
) -> RouteInputs<'a> {
    with_request(range_pick(policy, mirror), |request| {
        request.spec_type = RegistryPackageSpecType::Version;
    })
}

const RANGE_SHORTCUTS: MirrorShortcuts = MirrorShortcuts {
    exact_version: false,
    dominant_lockfile_version: true,
    unvalidated_mirror: false,
};

#[test]
fn a_stable_range_pick_may_use_the_lockfile_version() {
    let tagged = headers(Some(r#""etag""#), false);
    assert_eq!(
        MirrorShortcuts::decide(&range_pick(&NO_POLICY, Some(&tagged))),
        Some(RANGE_SHORTCUTS),
    );
    assert_eq!(MirrorShortcuts::decide(&range_pick(&NO_POLICY, None)), Some(RANGE_SHORTCUTS));
}

#[test]
fn every_option_that_needs_fresh_metadata_rules_out_the_range_shortcuts() {
    let untagged = headers(None, false);
    let release_age = release_age_policy();
    let no_downgrade =
        PackagePickPolicy { trust_policy: Some(TrustPolicy::NoDowngrade), ..NO_POLICY };
    let base = range_pick(&NO_POLICY, Some(&untagged));
    let cases: [(&str, RouteInputs<'_>); 7] = [
        ("prefer-offline", RouteInputs { network: NetworkPolicy::PreferCache, ..base }),
        ("pick lowest", with_request(base, |request| request.pick_lowest_version = true)),
        ("latest tag", with_request(base, |request| request.include_latest_tag = true)),
        ("update checksums", with_request(base, |request| request.update_checksums = true)),
        ("release age", RouteInputs { policy: &release_age, ..base }),
        ("no-downgrade", RouteInputs { policy: &no_downgrade, ..base }),
        ("blocked versions", with_request(base, |request| request.has_blocked_versions = true)),
    ];
    for (name, inputs) in cases {
        let shortcuts = MirrorShortcuts::decide(&inputs).expect("the mirror may answer");
        assert!(!shortcuts.dominant_lockfile_version, "{name}");
        assert!(!shortcuts.unvalidated_mirror, "{name}");
    }
}

#[test]
fn a_package_excluded_from_the_release_age_keeps_the_range_shortcuts() {
    let exclude = create_package_version_policy([PKG_NAME]).unwrap();
    let excluded =
        PackagePickPolicy { published_by_exclude: Some(&exclude), ..release_age_policy() };
    let decided = MirrorShortcuts::decide(&range_pick(&excluded, None));
    assert_eq!(decided, Some(RANGE_SHORTCUTS));
}

#[test]
fn an_unvalidated_mirror_skips_cache_shortcut_when_metadata_must_be_refreshed() {
    let untagged = headers(None, false);
    let empty_etag = headers(Some(""), false);
    let tagged = headers(Some(r#""etag""#), false);
    let cases = [
        ("no etag", range_pick(&NO_POLICY, Some(&untagged)), true),
        ("empty etag", range_pick(&NO_POLICY, Some(&empty_etag)), true),
        ("etag", range_pick(&NO_POLICY, Some(&tagged)), false),
        ("no mirror", range_pick(&NO_POLICY, None), false),
        (
            "private route",
            RouteInputs { public_scope: false, ..range_pick(&NO_POLICY, Some(&untagged)) },
            false,
        ),
    ];
    for (name, inputs, unvalidated_mirror) in cases {
        let shortcuts = MirrorShortcuts::decide(&inputs).expect("the mirror may answer");
        assert_eq!(shortcuts.unvalidated_mirror, unvalidated_mirror, "{name}");
    }
    let refresh = with_request(range_pick(&NO_POLICY, Some(&untagged)), |request| {
        request.refresh_metadata = true;
    });
    assert_eq!(MirrorShortcuts::decide(&refresh), None);
}

#[test]
fn a_pinned_version_may_be_read_from_the_mirror_unless_fresh_metadata_is_needed() {
    let base = version_pick(&NO_POLICY, None);
    let cases = [
        ("online", base, true),
        ("offline", RouteInputs { network: NetworkPolicy::Offline, ..base }, true),
        ("pick lowest", with_request(base, |request| request.pick_lowest_version = true), true),
        ("latest tag", with_request(base, |request| request.include_latest_tag = true), false),
        ("update checksums", with_request(base, |request| request.update_checksums = true), false),
        (
            "offline refresh metadata",
            with_request(RouteInputs { network: NetworkPolicy::Offline, ..base }, |request| {
                request.refresh_metadata = true;
            }),
            true,
        ),
        (
            "tag",
            with_request(base, |request| request.spec_type = RegistryPackageSpecType::Tag),
            false,
        ),
    ];
    let refresh = with_request(base, |request| request.refresh_metadata = true);
    assert_eq!(MirrorShortcuts::decide(&refresh), None);
    for (name, inputs, exact_version) in cases {
        let shortcuts = MirrorShortcuts::decide(&inputs).expect("the mirror may answer");
        assert_eq!(shortcuts.exact_version, exact_version, "{name}");
        assert!(!shortcuts.dominant_lockfile_version, "{name}");
    }
}

#[test]
fn the_release_age_cutoff_is_offered_where_the_policy_applies() {
    let release_age = release_age_policy();
    let exclude = create_package_version_policy([PKG_NAME]).unwrap();
    let excluded = PackagePickPolicy { published_by_exclude: Some(&exclude), ..release_age };
    let other_excluded = create_package_version_policy(["other"]).unwrap();
    let other = PackagePickPolicy { published_by_exclude: Some(&other_excluded), ..release_age };
    let aged = range_pick(&release_age, None);
    let cases = [
        ("online range", aged, Some(cutoff())),
        (
            "offline version",
            RouteInputs { network: NetworkPolicy::Offline, ..version_pick(&release_age, None) },
            Some(cutoff()),
        ),
        ("offline range", RouteInputs { network: NetworkPolicy::Offline, ..aged }, None),
        ("excluded package", range_pick(&excluded, None), None),
        ("another package excluded", range_pick(&other, None), Some(cutoff())),
        ("no cutoff", range_pick(&NO_POLICY, None), None),
    ];
    for (name, inputs, written_after) in cases {
        assert!(MirrorShortcuts::decide(&inputs).is_some(), "{name}");
        assert_eq!(inputs.written_after(), written_after, "{name}");
    }
}

#[test]
fn an_uncacheable_mirror_offers_no_shortcut_online() {
    let uncacheable = headers(None, true);
    let release_age = release_age_policy();
    let version = version_pick(&release_age, Some(&uncacheable));
    assert_eq!(MirrorShortcuts::decide(&range_pick(&NO_POLICY, Some(&uncacheable))), None);
    assert_eq!(MirrorShortcuts::decide(&version), None);

    let offline = RouteInputs { network: NetworkPolicy::Offline, ..version };
    let shortcuts = MirrorShortcuts::decide(&offline).expect("offline reads the mirror");
    assert!(shortcuts.exact_version);
    assert_eq!(offline.written_after(), Some(cutoff()));
}

#[test]
fn an_unvalidated_mirror_is_fresh_within_its_max_age_either_way() {
    let now = cutoff();
    let just_inside = UNVALIDATED_MIRROR_MAX_AGE - TimeDelta::milliseconds(1);
    let cases = [
        (now, true),
        (now - just_inside, true),
        (now + just_inside, true),
        (now - UNVALIDATED_MIRROR_MAX_AGE, false),
        (now + UNVALIDATED_MIRROR_MAX_AGE, false),
    ];
    for (mtime, fresh) in cases {
        assert_eq!(unvalidated_mirror_is_fresh(mtime, now), fresh, "mtime={mtime}");
    }
}

#[test]
fn the_disk_is_read_first_when_the_cache_comes_first_or_the_pick_is_lowest() {
    struct Case {
        network: NetworkPolicy,
        pick_lowest_version: bool,
        refresh_metadata: bool,
        disk_first: DiskFirst,
    }
    let cases = [
        Case {
            network: NetworkPolicy::Online,
            pick_lowest_version: false,
            refresh_metadata: false,
            disk_first: DiskFirst::Never,
        },
        Case {
            network: NetworkPolicy::Online,
            pick_lowest_version: true,
            refresh_metadata: false,
            disk_first: DiskFirst::UnlessUncacheable,
        },
        Case {
            network: NetworkPolicy::Online,
            pick_lowest_version: true,
            refresh_metadata: true,
            disk_first: DiskFirst::Never,
        },
        Case {
            network: NetworkPolicy::PreferCache,
            pick_lowest_version: false,
            refresh_metadata: false,
            disk_first: DiskFirst::Always,
        },
        Case {
            network: NetworkPolicy::PreferCache,
            pick_lowest_version: true,
            refresh_metadata: false,
            disk_first: DiskFirst::Always,
        },
        Case {
            network: NetworkPolicy::PreferCache,
            pick_lowest_version: true,
            refresh_metadata: true,
            disk_first: DiskFirst::Always,
        },
        Case {
            network: NetworkPolicy::Offline,
            pick_lowest_version: false,
            refresh_metadata: false,
            disk_first: DiskFirst::Always,
        },
    ];
    for Case {
        network,
        pick_lowest_version,
        refresh_metadata,
        disk_first,
    } in cases
    {
        assert_eq!(
            DiskFirst::decide(network, pick_lowest_version, refresh_metadata),
            disk_first,
            "network={network:?} pick_lowest_version={pick_lowest_version} refresh_metadata={refresh_metadata}",
        );
    }
}

#[test]
fn only_a_private_route_denied_access_skips_the_mirror_fallback() {
    assert!(fetch_failure_falls_back(true, true));
    assert!(fetch_failure_falls_back(true, false));
    assert!(fetch_failure_falls_back(false, false));
    assert!(!fetch_failure_falls_back(false, true));
}

#[test]
fn a_verified_or_offline_cache_hit_is_served_as_picked() {
    let online = range_pick(&NO_POLICY, None);
    let offline = RouteInputs { network: NetworkPolicy::Offline, ..online };
    for picked in [true, false] {
        assert_eq!(CacheHitCheck::decide(&online, true, picked), CacheHitCheck::Serve);
        assert_eq!(CacheHitCheck::decide(&offline, false, picked), CacheHitCheck::Serve);
    }
}

#[test]
fn an_unverified_cache_hit_without_a_pick_revalidates() {
    let cases = [
        ("range", range_pick(&NO_POLICY, None)),
        ("version", version_pick(&NO_POLICY, None)),
        (
            "prefer-offline",
            RouteInputs { network: NetworkPolicy::PreferCache, ..range_pick(&NO_POLICY, None) },
        ),
    ];
    for (name, inputs) in cases {
        assert_eq!(
            CacheHitCheck::decide(&inputs, false, false),
            CacheHitCheck::Revalidate,
            "{name}",
        );
    }
}

#[test]
fn an_unverified_cache_hit_serves_only_a_pick_a_stale_mirror_cannot_change() {
    let release_age = release_age_policy();
    let no_downgrade =
        PackagePickPolicy { trust_policy: Some(TrustPolicy::NoDowngrade), ..NO_POLICY };
    let base = range_pick(&NO_POLICY, None);
    let cases = [
        (
            "prefer-offline",
            RouteInputs { network: NetworkPolicy::PreferCache, ..base },
            CacheHitCheck::Serve,
        ),
        (
            "pick lowest",
            with_request(base, |request| request.pick_lowest_version = true),
            CacheHitCheck::Serve,
        ),
        ("version", version_pick(&release_age, None), CacheHitCheck::Serve),
        (
            "refresh metadata on a version",
            with_request(version_pick(&NO_POLICY, None), |request| {
                request.refresh_metadata = true;
            }),
            CacheHitCheck::Revalidate,
        ),
        ("stable range", base, CacheHitCheck::StableRangePick),
        (
            "latest tag",
            with_request(base, |request| request.include_latest_tag = true),
            CacheHitCheck::Revalidate,
        ),
        (
            "update checksums",
            with_request(base, |request| request.update_checksums = true),
            CacheHitCheck::Revalidate,
        ),
        (
            "refresh metadata",
            with_request(base, |request| request.refresh_metadata = true),
            CacheHitCheck::Revalidate,
        ),
        ("release age", RouteInputs { policy: &release_age, ..base }, CacheHitCheck::Revalidate),
        ("no-downgrade", RouteInputs { policy: &no_downgrade, ..base }, CacheHitCheck::Revalidate),
        (
            "blocked versions",
            with_request(base, |request| request.has_blocked_versions = true),
            CacheHitCheck::Revalidate,
        ),
        (
            "tag",
            with_request(base, |request| request.spec_type = RegistryPackageSpecType::Tag),
            CacheHitCheck::Revalidate,
        ),
    ];
    for (name, inputs, check) in cases {
        assert_eq!(CacheHitCheck::decide(&inputs, false, true), check, "{name}");
    }
}
