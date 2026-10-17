use super::{
    DiskFirst, MirrorShortcuts, RouteInputs, RoutePolicy, RouteRequest, UNVALIDATED_MIRROR_MAX_AGE,
    fetch_failure_falls_back, unvalidated_mirror_is_fresh,
};
use crate::{mirror::MetaHeaders, pick_package_from_meta::RegistryPackageSpecType};
use chrono::{DateTime, TimeDelta, Utc};
use pnpm_config::NetworkPolicy;
use pretty_assertions::assert_eq;

fn headers(etag: Option<&str>, uncacheable: bool) -> MetaHeaders {
    MetaHeaders { etag: etag.map(str::to_string), uncacheable, ..MetaHeaders::default() }
}

fn cutoff() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().into()
}

/// An online range pick with no option that needs fresh metadata, against a
/// public mirror stored with an `ETag`.
fn range_pick(mirror: Option<&MetaHeaders>) -> RouteInputs<'_> {
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
        policy: RoutePolicy {
            release_age_applies: false,
            published_by: None,
            trust_no_downgrade: false,
        },
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

fn with_policy(inputs: RouteInputs<'_>, policy: impl FnOnce(&mut RoutePolicy)) -> RouteInputs<'_> {
    let mut inputs = inputs;
    policy(&mut inputs.policy);
    inputs
}

const RANGE_SHORTCUTS: MirrorShortcuts = MirrorShortcuts {
    exact_version: false,
    dominant_lockfile_version: true,
    unvalidated_mirror: false,
    written_after: None,
};

#[test]
fn a_stable_range_pick_may_use_the_lockfile_version() {
    let tagged = headers(Some(r#""etag""#), false);
    assert_eq!(MirrorShortcuts::decide(&range_pick(Some(&tagged))), RANGE_SHORTCUTS);
    assert_eq!(MirrorShortcuts::decide(&range_pick(None)), RANGE_SHORTCUTS);
}

#[test]
fn every_option_that_needs_fresh_metadata_rules_out_the_range_shortcuts() {
    let tagged = headers(Some(r#""etag""#), false);
    let base = range_pick(Some(&tagged));
    let cases: [(&str, RouteInputs<'_>); 7] = [
        ("prefer-offline", RouteInputs { network: NetworkPolicy::PreferCache, ..base }),
        ("pick lowest", with_request(base, |request| request.pick_lowest_version = true)),
        ("latest tag", with_request(base, |request| request.include_latest_tag = true)),
        ("update checksums", with_request(base, |request| request.update_checksums = true)),
        ("release age", with_policy(base, |policy| policy.release_age_applies = true)),
        ("no-downgrade", with_policy(base, |policy| policy.trust_no_downgrade = true)),
        ("blocked versions", with_request(base, |request| request.has_blocked_versions = true)),
    ];
    for (name, inputs) in cases {
        let shortcuts = MirrorShortcuts::decide(&inputs);
        assert!(!shortcuts.dominant_lockfile_version, "{name}");
        assert!(!shortcuts.unvalidated_mirror, "{name}");
    }
}

#[test]
fn only_a_public_mirror_without_an_etag_skips_revalidation() {
    let untagged = headers(None, false);
    let empty_etag = headers(Some(""), false);
    let tagged = headers(Some(r#""etag""#), false);
    let cases = [
        ("no etag", range_pick(Some(&untagged)), true),
        ("empty etag", range_pick(Some(&empty_etag)), true),
        ("etag", range_pick(Some(&tagged)), false),
        ("no mirror", range_pick(None), false),
        (
            "private route",
            RouteInputs { public_scope: false, ..range_pick(Some(&untagged)) },
            false,
        ),
        (
            "pnpm update",
            with_request(range_pick(Some(&untagged)), |request| request.refresh_metadata = true),
            false,
        ),
    ];
    for (name, inputs, unvalidated_mirror) in cases {
        assert_eq!(
            MirrorShortcuts::decide(&inputs).unvalidated_mirror,
            unvalidated_mirror,
            "{name}",
        );
    }
}

#[test]
fn a_pinned_version_may_be_read_from_the_mirror_unless_fresh_metadata_is_needed() {
    let base = with_request(range_pick(None), |request| {
        request.spec_type = RegistryPackageSpecType::Version;
    });
    let cases = [
        ("online", base, true),
        ("offline", RouteInputs { network: NetworkPolicy::Offline, ..base }, true),
        ("pick lowest", with_request(base, |request| request.pick_lowest_version = true), true),
        ("latest tag", with_request(base, |request| request.include_latest_tag = true), false),
        ("update checksums", with_request(base, |request| request.update_checksums = true), false),
        (
            "tag",
            with_request(base, |request| request.spec_type = RegistryPackageSpecType::Tag),
            false,
        ),
    ];
    for (name, inputs, exact_version) in cases {
        let shortcuts = MirrorShortcuts::decide(&inputs);
        assert_eq!(shortcuts.exact_version, exact_version, "{name}");
        assert!(!shortcuts.dominant_lockfile_version, "{name}");
    }
}

#[test]
fn the_release_age_cutoff_is_offered_where_the_policy_applies() {
    let aged = with_policy(range_pick(None), |policy| {
        policy.release_age_applies = true;
        policy.published_by = Some(cutoff());
    });
    let cases = [
        ("online range", aged, Some(cutoff())),
        (
            "offline version",
            with_request(RouteInputs { network: NetworkPolicy::Offline, ..aged }, |request| {
                request.spec_type = RegistryPackageSpecType::Version;
            }),
            Some(cutoff()),
        ),
        ("offline range", RouteInputs { network: NetworkPolicy::Offline, ..aged }, None),
        ("excluded package", with_policy(aged, |policy| policy.release_age_applies = false), None),
        ("no cutoff", with_policy(aged, |policy| policy.published_by = None), None),
    ];
    for (name, inputs, written_after) in cases {
        assert_eq!(MirrorShortcuts::decide(&inputs).written_after, written_after, "{name}");
    }
}

#[test]
fn an_uncacheable_mirror_offers_no_shortcut_online() {
    let uncacheable = headers(None, true);
    let version = with_request(range_pick(Some(&uncacheable)), |request| {
        request.spec_type = RegistryPackageSpecType::Version;
    });
    let version = with_policy(version, |policy| {
        policy.release_age_applies = true;
        policy.published_by = Some(cutoff());
    });
    assert_eq!(
        MirrorShortcuts::decide(&range_pick(Some(&uncacheable))),
        MirrorShortcuts::default(),
    );
    assert_eq!(MirrorShortcuts::decide(&version), MirrorShortcuts::default());

    let offline =
        MirrorShortcuts::decide(&RouteInputs { network: NetworkPolicy::Offline, ..version });
    assert!(offline.exact_version);
    assert_eq!(offline.written_after, Some(cutoff()));
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
    let cases = [
        (NetworkPolicy::Online, false, DiskFirst::Never),
        (NetworkPolicy::Online, true, DiskFirst::UnlessUncacheable),
        (NetworkPolicy::PreferCache, false, DiskFirst::Always),
        (NetworkPolicy::PreferCache, true, DiskFirst::Always),
        (NetworkPolicy::Offline, false, DiskFirst::Always),
    ];
    for (network, pick_lowest_version, disk_first) in cases {
        assert_eq!(
            DiskFirst::decide(network, pick_lowest_version),
            disk_first,
            "network={network:?} pick_lowest_version={pick_lowest_version}",
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
