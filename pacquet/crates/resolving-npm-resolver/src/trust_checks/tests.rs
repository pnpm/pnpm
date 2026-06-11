use chrono::{DateTime, Utc};
use pacquet_config::version_policy::create_package_version_policy;
use pacquet_registry::Package;

use super::{TrustCheckOptions, TrustViolation, fail_if_trust_downgraded};

#[derive(Clone, Copy)]
enum Evidence {
    None,
    Provenance,
    TrustedPublisher,
    StagedPublish,
}

/// Build a JSON object for a single version with the trust-evidence
/// shape the verifier reads (`_npmUser.approver`,
/// `_npmUser.trustedPublisher`, or `dist.attestations.provenance`). A
/// `TrustedPublisher` fixture includes both fields: per
/// `get_trust_evidence`, the publisher flag only outranks plain
/// provenance when the version also ships a provenance attestation. A
/// `StagedPublish` fixture carries an `approver`, the strongest signal.
fn version_json(name: &str, version: &str, evidence: Evidence) -> serde_json::Value {
    let mut dist = serde_json::json!({
        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        "shasum": "0000000000000000000000000000000000000000",
        "tarball": format!("https://registry/{name}-{version}.tgz")
    });
    if matches!(
        evidence,
        Evidence::Provenance | Evidence::TrustedPublisher | Evidence::StagedPublish,
    ) {
        dist["attestations"] = serde_json::json!({
            "provenance": { "predicateType": "https://slsa.dev/provenance/v1" }
        });
    }

    let mut version_obj = serde_json::json!({
        "name": name,
        "version": version,
        "dist": dist,
    });
    if matches!(evidence, Evidence::TrustedPublisher) {
        version_obj["_npmUser"] = serde_json::json!({
            "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
        });
    }
    if matches!(evidence, Evidence::StagedPublish) {
        version_obj["_npmUser"] = serde_json::json!({
            "approver": { "name": "approver", "email": "approver@example.com" }
        });
    }
    version_obj
}

fn make_package(name: &str, versions: &[(&str, &str, Evidence)]) -> Package {
    let versions_json: serde_json::Map<String, serde_json::Value> =
        versions.iter().map(|(v, _, ev)| ((*v).to_string(), version_json(name, v, *ev))).collect();
    let time_json: serde_json::Map<String, serde_json::Value> = versions
        .iter()
        .map(|(v, t, _)| ((*v).to_string(), serde_json::Value::String((*t).to_string())))
        .collect();
    let body = serde_json::json!({
        "name": name,
        "dist-tags": {},
        "time": time_json,
        "versions": versions_json,
    });
    serde_json::from_value(body).expect("deserialize fixture Package")
}

fn now_at(date: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(date).expect("parse RFC3339").with_timezone(&Utc)
}

/// First-ever version of a package: no earlier history, so no
/// baseline to downgrade from → always passes.
#[test]
fn first_version_passes_with_no_history() {
    let meta = make_package("acme", &[("1.0.0", "2025-01-10T00:00:00.000Z", Evidence::None)]);
    fail_if_trust_downgraded(&meta, "1.0.0", &TrustCheckOptions::default())
        .expect("no prior history → no downgrade possible");
}

/// Earlier version had `trustedPublisher`, current version has
/// only `provenance` → DOWNGRADE.
#[test]
fn trusted_publisher_to_provenance_downgrade_fails() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::Provenance),
        ],
    );
    let err = fail_if_trust_downgraded(&meta, "1.1.0", &TrustCheckOptions::default())
        .expect_err("trusted-publisher → provenance is a downgrade");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// Earlier version had `stagedPublish` (an approver), current version
/// has only `trustedPublisher` → DOWNGRADE. Staged publish outranks a
/// trusted publisher.
#[test]
fn staged_publish_to_trusted_publisher_downgrade_fails() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::StagedPublish),
            ("2.0.0", "2025-02-01T00:00:00.000Z", Evidence::TrustedPublisher),
        ],
    );
    let err = fail_if_trust_downgraded(&meta, "2.0.0", &TrustCheckOptions::default())
        .expect_err("staged-publish → trusted-publisher is a downgrade");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// Earlier version had `provenance`, current version has no
/// evidence at all → DOWNGRADE.
#[test]
fn provenance_to_unsigned_downgrade_fails() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::Provenance),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    let err = fail_if_trust_downgraded(&meta, "1.1.0", &TrustCheckOptions::default())
        .expect_err("provenance → no evidence is a downgrade");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// Earlier version had `trustedPublisher`, current version has no
/// evidence at all → DOWNGRADE. Mirrors upstream's "downgrading from
/// trustedPublisher to none" case, with a third unsigned version
/// before the publisher version to exercise the full history walk.
#[test]
fn trusted_publisher_to_unsigned_downgrade_fails() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::None),
            ("2.0.0", "2025-02-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("3.0.0", "2025-03-01T00:00:00.000Z", Evidence::None),
        ],
    );
    let err = fail_if_trust_downgraded(&meta, "3.0.0", &TrustCheckOptions::default())
        .expect_err("trusted-publisher → no evidence is a downgrade");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// No version in the history carries any trust evidence, so there is
/// no baseline to downgrade from → passes. Mirrors upstream's
/// "succeeds when no versions have attestation".
#[test]
fn no_evidence_anywhere_passes() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::None),
            ("2.0.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    fail_if_trust_downgraded(&meta, "2.0.0", &TrustCheckOptions::default())
        .expect("no evidence anywhere → no downgrade possible");
}

/// Equal-rank evidence (provenance → provenance) is not a
/// downgrade.
#[test]
fn equal_rank_passes() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::Provenance),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::Provenance),
        ],
    );
    fail_if_trust_downgraded(&meta, "1.1.0", &TrustCheckOptions::default())
        .expect("equal rank should pass");
}

/// Upgrade (provenance → trusted-publisher) is not a downgrade.
#[test]
fn rank_upgrade_passes() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::Provenance),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::TrustedPublisher),
        ],
    );
    fail_if_trust_downgraded(&meta, "1.1.0", &TrustCheckOptions::default())
        .expect("rank upgrade should pass");
}

/// Only later-published versions had stronger evidence: that
/// can't downgrade an earlier version since the history walk
/// excludes anything published on/after the target's date.
#[test]
fn later_publish_does_not_downgrade_earlier_version() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::None),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::TrustedPublisher),
        ],
    );
    fail_if_trust_downgraded(&meta, "1.0.0", &TrustCheckOptions::default())
        .expect("history walk excludes versions newer than the target");
}

/// Prerelease history is excluded when the current version is
/// stable. Mirrors upstream's `semver.prerelease(version, true)`
/// guard: a stable `1.1.0` doesn't get downgraded by a
/// `1.1.0-alpha.1` that happened to ship with provenance.
#[test]
fn stable_version_ignores_prerelease_history() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0-alpha.1", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.0.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    fail_if_trust_downgraded(&meta, "1.0.0", &TrustCheckOptions::default())
        .expect("prerelease history is excluded when target is stable");
}

/// Prerelease target inspects prerelease history (the same
/// upstream guard, inverted: the target itself is a prerelease so
/// the exclusion doesn't apply).
#[test]
fn prerelease_target_compares_against_prerelease_history() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0-alpha.1", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.0.0-alpha.2", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    let err = fail_if_trust_downgraded(&meta, "1.0.0-alpha.2", &TrustCheckOptions::default())
        .expect_err("prerelease target sees prerelease history");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// `trust_policy_ignore_after_minutes` cuts the check off once a
/// version is old enough. With `now` set 100 days after publish
/// and the ignore-cutoff at 7 days (`10_080` minutes), the check
/// skips even though prior history would have flagged a downgrade.
#[test]
fn ignore_after_skips_check_for_settled_versions() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.1.0", "2025-01-15T00:00:00.000Z", Evidence::None),
        ],
    );
    let opts = TrustCheckOptions {
        trust_policy_ignore_after_minutes: Some(7 * 24 * 60),
        now: Some(now_at("2025-05-01T00:00:00.000Z")),
        ..Default::default()
    };
    fail_if_trust_downgraded(&meta, "1.1.0", &opts).expect("settled-enough version skips check");
}

/// Recent versions still get checked when `ignore_after` is set
/// but the target is younger than the cutoff. Same fixture as
/// above with `now` close to the publish time.
#[test]
fn ignore_after_still_checks_fresh_versions() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.1.0", "2025-01-15T00:00:00.000Z", Evidence::None),
        ],
    );
    let opts = TrustCheckOptions {
        trust_policy_ignore_after_minutes: Some(7 * 24 * 60),
        now: Some(now_at("2025-01-16T00:00:00.000Z")),
        ..Default::default()
    };
    let err = fail_if_trust_downgraded(&meta, "1.1.0", &opts)
        .expect_err("fresh version still gets checked");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// `trust_policy_exclude` opting a whole package out of the check
/// short-circuits before the history walk.
#[test]
fn exclude_any_version_short_circuits_check() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    let exclude = create_package_version_policy(["acme"]).unwrap();
    let opts = TrustCheckOptions { trust_policy_exclude: Some(&exclude), ..Default::default() };
    fail_if_trust_downgraded(&meta, "1.1.0", &opts).expect("acme excluded → check short-circuits");
}

/// `trust_policy_exclude` opting one specific version out of the
/// check covers just that version.
#[test]
fn exclude_exact_version_short_circuits_check() {
    let meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    let exclude = create_package_version_policy(["acme@1.1.0"]).unwrap();
    let opts = TrustCheckOptions { trust_policy_exclude: Some(&exclude), ..Default::default() };
    fail_if_trust_downgraded(&meta, "1.1.0", &opts)
        .expect("acme@1.1.0 excluded → check short-circuits");

    // A different version of the same package is still checked.
    let err = fail_if_trust_downgraded(&meta, "1.0.0", &opts).err();
    // 1.0.0 has trusted-publisher itself, so the check still passes
    // even though it's not excluded — the exclude policy only
    // matters when there'd otherwise be a downgrade. This test pins
    // that the exclude is targeted, not blanket.
    assert!(err.is_none(), "1.0.0 has its own trusted-publisher → passes");
}

/// An excluded `name@version` short-circuits *before* the `time`
/// lookup, so a packument with no `time` map still passes rather than
/// surfacing `TrustCheckFailed`. Pins the ordering of the exclude
/// check ahead of the time assertion. Mirrors upstream's "does not
/// fail with `ERR_PNPM_MISSING_TIME` when package@version is excluded".
#[test]
fn exclude_exact_version_with_missing_time_does_not_fail() {
    let mut meta = make_package("acme", &[("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::None)]);
    if let Some(time) = meta.time.as_mut() {
        time.clear();
    }
    let exclude = create_package_version_policy(["acme@1.0.0"]).unwrap();
    let opts = TrustCheckOptions { trust_policy_exclude: Some(&exclude), ..Default::default() };
    fail_if_trust_downgraded(&meta, "1.0.0", &opts)
        .expect("excluded version short-circuits before the missing-time check");
}

/// Same as above, but the whole package name is excluded. Mirrors
/// upstream's "does not fail with `ERR_PNPM_MISSING_TIME` when package
/// name is excluded".
#[test]
fn exclude_package_name_with_missing_time_does_not_fail() {
    let mut meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::None),
            ("2.0.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    if let Some(time) = meta.time.as_mut() {
        time.clear();
    }
    let exclude = create_package_version_policy(["acme"]).unwrap();
    let opts = TrustCheckOptions { trust_policy_exclude: Some(&exclude), ..Default::default() };
    fail_if_trust_downgraded(&meta, "2.0.0", &opts)
        .expect("excluded package short-circuits before the missing-time check");
}

/// Missing `time` entry for the target version surfaces as
/// `TrustCheckFailed`. Mirrors upstream's
/// `Missing time for version X of Y` `PnpmError`.
#[test]
fn missing_time_surfaces_trust_check_failed() {
    let mut meta = make_package("acme", &[("1.0.0", "2025-01-10T00:00:00.000Z", Evidence::None)]);
    // Drop the version's time entry.
    if let Some(time) = meta.time.as_mut() {
        time.clear();
    }
    let err = fail_if_trust_downgraded(&meta, "1.0.0", &TrustCheckOptions::default())
        .expect_err("missing time should fail with TrustCheckFailed");
    assert!(matches!(err, TrustViolation::TrustCheckFailed { .. }), "got {err:?}");
}

/// A timestamp string that doesn't parse as RFC3339 also surfaces
/// as `TrustCheckFailed`.
#[test]
fn unparsable_timestamp_surfaces_trust_check_failed() {
    let mut meta = make_package("acme", &[("1.0.0", "2025-01-10T00:00:00.000Z", Evidence::None)]);
    if let Some(time) = meta.time.as_mut() {
        time.insert("1.0.0".to_string(), serde_json::Value::String("not-a-date".to_string()));
    }
    let err = fail_if_trust_downgraded(&meta, "1.0.0", &TrustCheckOptions::default())
        .expect_err("unparsable timestamp should fail");
    assert!(matches!(err, TrustViolation::TrustCheckFailed { .. }), "got {err:?}");
}

/// Regression: a prior version with no entry in the `time` map must
/// not abort the history walk. The scan needs to consult every
/// other prior version's evidence so a stronger-evidence ancestor
/// (here `1.0.0` with `TrustedPublisher`) still gates downgrades to
/// a weaker successor (`1.1.0` with no evidence). Without the
/// per-version `continue`, a single missing-time entry would mask
/// the entire ancestry and let the downgrade slip through.
#[test]
fn prior_version_missing_time_does_not_mask_trust_history() {
    let mut meta = make_package(
        "acme",
        &[
            ("1.0.0", "2025-01-01T00:00:00.000Z", Evidence::TrustedPublisher),
            ("1.0.1", "2025-01-15T00:00:00.000Z", Evidence::Provenance),
            ("1.1.0", "2025-02-01T00:00:00.000Z", Evidence::None),
        ],
    );
    // Drop the middle version's `time` entry so it has a manifest
    // but no publish timestamp — the exact shape that previously
    // tripped the early-return.
    if let Some(time) = meta.time.as_mut() {
        time.remove("1.0.1");
    }
    let err = fail_if_trust_downgraded(&meta, "1.1.0", &TrustCheckOptions::default())
        .expect_err("missing-time on a prior version must not mask the 1.0.0 baseline");
    assert!(matches!(err, TrustViolation::TrustDowngrade { .. }), "got {err:?}");
}

/// A prior version whose manifest fragment does not decode fails the
/// check closed (`TrustCheckFailed`) instead of being skipped — the
/// skipped fragment could be the strongest prior evidence, and
/// passing without it would let a downgrade through undetected.
#[test]
fn undecodable_prior_version_fails_closed() {
    let body = serde_json::json!({
        "name": "acme",
        "dist-tags": {},
        "time": {
            "1.0.0": "2025-01-01T00:00:00.000Z",
            "1.1.0": "2025-02-01T00:00:00.000Z",
        },
        "versions": {
            "1.0.0": { "corrupt": "fragment" },
            "1.1.0": version_json("acme", "1.1.0", Evidence::None),
        },
    });
    let meta: Package = serde_json::from_value(body).expect("deserialize fixture Package");
    let err = fail_if_trust_downgraded(&meta, "1.1.0", &TrustCheckOptions::default())
        .expect_err("undecodable prior manifest must fail the trust check");
    assert!(matches!(err, TrustViolation::TrustCheckFailed { .. }), "got {err:?}");
}

mod get_trust_evidence {
    use pacquet_registry::PackageVersion;

    use super::{Evidence, version_json};
    use crate::trust_checks::{TrustEvidence, get_trust_evidence};

    fn parse(version: serde_json::Value) -> PackageVersion {
        serde_json::from_value(version).expect("deserialize fixture PackageVersion")
    }

    /// `_npmUser.trustedPublisher` without `dist.attestations.provenance`
    /// is ignored — the publisher flag alone is metadata a staged
    /// publish could mint, so it cannot stand in for the attestation.
    #[test]
    fn trusted_publisher_without_provenance_is_none() {
        let mut version = version_json("acme", "1.0.0", Evidence::None);
        version["_npmUser"] = serde_json::json!({
            "trustedPublisher": { "id": "github", "oidcConfigId": "release" }
        });
        assert!(get_trust_evidence(&parse(version)).is_none());
    }

    /// `_npmUser.trustedPublisher` *with* provenance ranks as
    /// `TrustedPublisher` (the strongest evidence).
    #[test]
    fn trusted_publisher_with_provenance_ranks_strongest() {
        let version = version_json("acme", "1.0.0", Evidence::TrustedPublisher);
        assert!(matches!(
            get_trust_evidence(&parse(version)),
            Some(TrustEvidence::TrustedPublisher)
        ));
    }

    /// `_npmUser.approver` ranks as `StagedPublish`, the strongest
    /// evidence.
    #[test]
    fn approver_ranks_as_staged_publish() {
        let version = version_json("acme", "1.0.0", Evidence::StagedPublish);
        assert!(matches!(get_trust_evidence(&parse(version)), Some(TrustEvidence::StagedPublish)));
    }

    /// `_npmUser.approver` wins even when `trustedPublisher` and
    /// provenance are also present — staged publish takes priority.
    #[test]
    fn approver_outranks_trusted_publisher() {
        let mut version = version_json("acme", "1.0.0", Evidence::TrustedPublisher);
        version["_npmUser"]["approver"] =
            serde_json::json!({ "name": "approver", "email": "approver@example.com" });
        assert!(matches!(get_trust_evidence(&parse(version)), Some(TrustEvidence::StagedPublish)));
    }

    /// `dist.attestations.provenance` alone ranks as `Provenance`.
    #[test]
    fn provenance_alone_ranks_as_provenance() {
        let version = version_json("acme", "1.0.0", Evidence::Provenance);
        assert!(matches!(get_trust_evidence(&parse(version)), Some(TrustEvidence::Provenance)));
    }

    /// Neither field present → `None`.
    #[test]
    fn no_evidence_returns_none() {
        let version = version_json("acme", "1.0.0", Evidence::None);
        assert!(get_trust_evidence(&parse(version)).is_none());
    }

    /// An `_npmUser` record without a `trustedPublisher` field is
    /// ignored.
    #[test]
    fn npm_user_without_trusted_publisher_is_none() {
        let mut version = version_json("acme", "1.0.0", Evidence::None);
        version["_npmUser"] = serde_json::json!({ "name": "alice", "email": "alice@example.com" });
        assert!(get_trust_evidence(&parse(version)).is_none());
    }
}
