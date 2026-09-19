//! What the active `minimumReleaseAge` / `publishedBy` policy admits, and
//! what it flags.
//!
//! Every reading of the cutoff lives here so the resolver's answers agree:
//! a caller that filtered differently from the pick would misreport why a
//! version was chosen, or point a user at a version pnpm would then refuse.

use super::{
    DateTime, LockfileResolution, MINIMUM_RELEASE_AGE_VIOLATION_CODE, Package,
    PackageVersionPolicy, PkgName, ResolutionPolicyViolation, Utc, parse_packument_timestamp,
};

/// The raw `dist-tags.latest` when the active `minimumReleaseAge`
/// policy would allow installing it, `None` otherwise. The install
/// summary's `(X is available)` hint must only ever name the actual
/// latest tag, so an immature latest suppresses the hint instead of
/// being rewritten to an older mature version. Suppression requires
/// positive evidence of immaturity: a missing or unparsable
/// timestamp keeps the raw tag, matching
/// [`detect_min_release_age_violation`], which likewise only flags a
/// version it can date.
pub(super) fn latest_allowed_by_policy<'a>(
    meta: &'a Package,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Option<&'a str> {
    let latest = meta.dist_tag("latest")?;
    let Some(cutoff) = published_by else { return Some(latest) };
    (!known_immature(meta, latest, cutoff, published_by_exclude)).then_some(latest)
}

/// Whether the policy trusts `version` outright, by excluding the package
/// wholesale or by naming that exact version.
fn policy_trusts(
    meta: &Package,
    version: &str,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> bool {
    use pnpm_config::version_policy::PolicyMatch;
    let Some(policy) = published_by_exclude else { return false };
    match policy.matches(&meta.name) {
        PolicyMatch::AnyVersion => true,
        PolicyMatch::ExactVersions(versions) => versions
            .iter()
            .any(|exact| exact == version),
        PolicyMatch::No => false,
    }
}

/// Whether the cutoff has positive evidence that `version` is too new.
///
/// A version pnpm cannot date is not flagged, matching
/// [`detect_min_release_age_violation`], which likewise only flags a version
/// it can date. Reporting is what wants this direction: hiding an available
/// version over metadata pnpm failed to read would be its own wrong answer.
fn known_immature(
    meta: &Package,
    version: &str,
    cutoff: DateTime<Utc>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> bool {
    if policy_trusts(meta, version, published_by_exclude) {
        return false;
    }
    matches!(
        meta.published_at(version).and_then(parse_packument_timestamp),
        Some(published_at) if published_at > cutoff,
    )
}

/// Whether `version` clears the cutoff the way the pick's own filter requires.
///
/// The inverse of [`known_immature`]: admission needs positive evidence of
/// maturity, because `filter_pkg_metadata_by_publish_date` drops every version
/// it cannot date. Recommending a version wants this direction, so pnpm never
/// names one the pick would then refuse.
pub(super) fn installable_under_policy(
    meta: &Package,
    version: &str,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> bool {
    let Some(cutoff) = published_by else { return true };
    policy_trusts(meta, version, published_by_exclude)
        || matches!(
            meta.published_at(version).and_then(parse_packument_timestamp),
            Some(published_at) if published_at <= cutoff,
        )
}

/// Resolver-time `minimumReleaseAge` check. Returns a violation entry
/// when the picked version's publish timestamp falls past the policy
/// cutoff and isn't excluded by name/version.
pub(super) fn detect_min_release_age_violation(
    name: &PkgName,
    version: &str,
    published_at: Option<&str>,
    resolution: &LockfileResolution,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&PackageVersionPolicy>,
) -> Option<ResolutionPolicyViolation> {
    let cutoff = published_by?;
    let timestamp = published_at?;
    if let Some(policy) = published_by_exclude {
        use pnpm_config::version_policy::PolicyMatch;
        match policy.matches(&name.to_string()) {
            PolicyMatch::AnyVersion => return None,
            PolicyMatch::ExactVersions(versions)
                if versions
                    .iter()
                    .any(|exact| exact == version) =>
            {
                return None;
            }
            _ => {}
        }
    }
    let parsed = parse_packument_timestamp(timestamp)?;
    if parsed <= cutoff {
        return None;
    }
    Some(ResolutionPolicyViolation {
        name: name.clone(),
        version: version.to_string(),
        resolution: resolution.clone(),
        code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
        reason: format!(
            "was published at {timestamp}, within the minimumReleaseAge cutoff ({cutoff})",
            cutoff = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        ),
    })
}
