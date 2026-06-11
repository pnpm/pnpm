//! Trust-downgrade detection.
//!
//! Ports pnpm's
//! [`trustChecks.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/trustChecks.ts).
//!
//! The check walks every published version of a package whose
//! publish time is strictly before the version under inspection.
//! For each it asks [`get_trust_evidence`] which "rank" of evidence
//! the version exposes:
//!
//! - `stagedPublish` (rank 3) — `_npmUser.approver` is present. A
//!   staged publish required a 2FA publish approval, the strongest
//!   trust signal.
//! - `trustedPublisher` (rank 2) — `_npmUser.trustedPublisher` and
//!   `dist.attestations.provenance` are both present.
//! - `provenance` (rank 1) — `dist.attestations.provenance` is
//!   present without a trusted-publisher record.
//! - `None` (rank 0 / no evidence).
//!
//! The strongest rank seen across the prior history is the
//! "baseline." If the current version's rank is lower than the
//! baseline, that's a trust *downgrade* — supply-chain incident
//! signal — and the verifier rejects the entry with
//! [`crate::TRUST_DOWNGRADE_VIOLATION_CODE`].
//!
//! Prereleases of the same major-minor-patch are excluded from the
//! history walk when the current version is *not* a prerelease:
//! upstream's `semver.prerelease(version, true)` decides this.

use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use miette::Diagnostic;
use node_semver::Version;
use pacquet_config::version_policy::{PackageVersionPolicy, PolicyMatch};
use pacquet_registry::{Package, PackageVersion};
use pacquet_resolving_resolver_base::parse_packument_timestamp;

/// Rank of supply-chain evidence on a single version. Variants are
/// declared weakest-first so the derived `Ord` matches `trust_rank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustEvidence {
    /// `dist.attestations.provenance` is set.
    Provenance,
    /// `_npmUser.trustedPublisher` and `dist.attestations.provenance`
    /// are both set. Without the attestation the publisher flag is
    /// just metadata a future staged-publish flow could mint, so it
    /// only counts as the stronger signal when the version also
    /// shipped a provenance attestation.
    TrustedPublisher,
    /// `_npmUser.approver` is set. The version was published through a
    /// staged publish requiring a 2FA approval — the strongest signal,
    /// ranked above a trusted publisher.
    StagedPublish,
}

/// Failure surfaced by [`fail_if_trust_downgraded`]. Each variant
/// maps to a `TRUST_*` diagnostic code mirroring upstream's
/// `PnpmError('TRUST_CHECK_FAIL', ...)` / `PnpmError('TRUST_DOWNGRADE', ...)`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum TrustViolation {
    /// Reserved for the metadata-shape failures upstream raises with
    /// the `TRUST_CHECK_FAIL` code: missing `time` map, missing per-
    /// version manifest, unparsable publish timestamp. Surfaced as
    /// a "could not be checked" violation reason at the verifier
    /// boundary.
    #[display("trust check failed: {reason}")]
    #[diagnostic(code(pacquet_resolving_npm_resolver::trust_check_failed))]
    TrustCheckFailed {
        #[error(not(source))]
        reason: String,
    },

    /// Earlier versions had stronger trust evidence than the version
    /// being verified — supply-chain incident signal. Mirrors
    /// upstream's `TRUST_DOWNGRADE` code.
    #[display("High-risk trust downgrade for \"{name}@{version}\" (possible package takeover)")]
    #[diagnostic(
        code(pacquet_resolving_npm_resolver::trust_downgrade),
        help(
            "Trust checks are based solely on publish date, not semver. A package cannot be installed if any earlier-published version had stronger trust evidence. Earlier versions had {past_pretty}, but this version has {current_pretty}. A trust downgrade may indicate a supply chain incident."
        )
    )]
    TrustDowngrade {
        #[error(not(source))]
        name: String,
        version: String,
        past_pretty: &'static str,
        current_pretty: &'static str,
    },
}

/// Options bundle for [`fail_if_trust_downgraded`].
#[derive(Debug, Default, Clone)]
pub struct TrustCheckOptions<'a> {
    /// Package-version policy that opts specific packages out of
    /// the trust check entirely (`AnyVersion`) or for specific
    /// versions (`ExactVersions`).
    pub trust_policy_exclude: Option<&'a PackageVersionPolicy>,

    /// Maximum age, in minutes, before which the check still
    /// applies. A version older than this skips the check on the
    /// theory that any downgrade would have surfaced by now.
    /// `None` means "always check"; matches upstream's `undefined`
    /// for the same field.
    pub trust_policy_ignore_after_minutes: Option<u64>,

    /// Override for "now" when the check evaluates
    /// `trust_policy_ignore_after_minutes`. Defaults to wall-clock
    /// `Utc::now`; tests pin it for determinism.
    pub now: Option<DateTime<Utc>>,
}

/// Reject `version` of `meta` when its trust evidence is weaker
/// than the strongest evidence seen on any earlier-published
/// version. Port of upstream's
/// [`failIfTrustDowngraded`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/trustChecks.ts#L15-L80).
pub fn fail_if_trust_downgraded(
    meta: &Package,
    version: &str,
    opts: &TrustCheckOptions<'_>,
) -> Result<(), TrustViolation> {
    // Exclude policy short-circuit.
    if let Some(exclude) = opts.trust_policy_exclude {
        match exclude.matches(&meta.name) {
            PolicyMatch::AnyVersion => return Ok(()),
            PolicyMatch::ExactVersions(versions) => {
                if versions.iter().any(|exact| exact == version) {
                    return Ok(());
                }
            }
            PolicyMatch::No => {}
        }
    }

    // Pull the version's publish time. Upstream's `assertMetaHasTime`
    // throws if the whole `time` map is missing; we treat both
    // "no time map" and "no entry for this version" as the same
    // `TRUST_CHECK_FAIL` shape so the verifier surfaces a single
    // "could not be checked" reason.
    let published_at =
        meta.published_at(version).ok_or_else(|| TrustViolation::TrustCheckFailed {
            reason: format!(
                "missing time for version {version} of {name} in metadata",
                name = meta.name,
            ),
        })?;
    let version_date = parse_packument_timestamp(published_at).ok_or_else(|| {
        TrustViolation::TrustCheckFailed {
            reason: "publish timestamp is not a valid date".to_string(),
        }
    })?;

    // Ignore-after cutoff: a version old enough to be "settled"
    // gets a pass.
    if let Some(ignore_after_minutes) = opts.trust_policy_ignore_after_minutes {
        let now = opts.now.unwrap_or_else(Utc::now);
        let minutes_since_publish = (now - version_date).num_seconds().max(0) as u64 / 60;
        if minutes_since_publish > ignore_after_minutes {
            return Ok(());
        }
    }

    let manifest = meta.versions.get(version).ok_or_else(|| TrustViolation::TrustCheckFailed {
        reason: format!(
            "missing version object for version {version} of {name} in metadata",
            name = meta.name,
        ),
    })?;

    let exclude_prerelease = !is_prerelease(version);
    let Some(strongest_prior) =
        detect_strongest_trust_evidence_before(meta, version_date, exclude_prerelease)?
    else {
        return Ok(());
    };

    let current = get_trust_evidence(&manifest);
    let current_rank = current.map_or(0u8, trust_rank);
    let prior_rank = trust_rank(strongest_prior);
    if current_rank < prior_rank {
        return Err(TrustViolation::TrustDowngrade {
            name: meta.name.clone(),
            version: version.to_string(),
            past_pretty: pretty_print_trust_evidence(Some(strongest_prior)),
            current_pretty: pretty_print_trust_evidence(current),
        });
    }
    Ok(())
}

/// Map a [`TrustEvidence`] rank to upstream's numeric weight at
/// [`trustChecks.ts:10-14`](https://github.com/pnpm/pnpm/blob/372cae6a55/resolving/npm-resolver/src/trustChecks.ts#L10-L14).
/// Upstream uses `undefined` for "no evidence"; the Rust port uses
/// `Option<TrustEvidence>` so callers compare ranks via
/// `Option::map_or(0, trust_rank)`.
fn trust_rank(evidence: TrustEvidence) -> u8 {
    match evidence {
        TrustEvidence::StagedPublish => 3,
        TrustEvidence::TrustedPublisher => 2,
        TrustEvidence::Provenance => 1,
    }
}

fn pretty_print_trust_evidence(evidence: Option<TrustEvidence>) -> &'static str {
    match evidence {
        Some(TrustEvidence::StagedPublish) => "staged publish",
        Some(TrustEvidence::TrustedPublisher) => "trusted publisher",
        Some(TrustEvidence::Provenance) => "provenance attestation",
        None => "no trust evidence",
    }
}

/// Walk every version older than `before_date` and return the
/// strongest [`TrustEvidence`] seen. Prereleases are filtered out
/// when the current version is *not* itself a prerelease — matches
/// upstream's `semver.prerelease(version, true)` guard.
///
/// Fails closed: a prior version whose manifest is listed but does
/// not decode makes the scan error rather than skip — skipping could
/// hide the strongest prior evidence and let a trust downgrade pass
/// undetected.
fn detect_strongest_trust_evidence_before(
    meta: &Package,
    before_date: DateTime<Utc>,
    exclude_prerelease: bool,
) -> Result<Option<TrustEvidence>, TrustViolation> {
    let mut best: Option<TrustEvidence> = None;
    for version in meta.versions.keys() {
        if exclude_prerelease && is_prerelease(version) {
            continue;
        }
        // Skip individual versions that lack a publish timestamp
        // rather than aborting the entire history walk: a single
        // prior version with no `time` entry would otherwise mask
        // every earlier version's evidence and allow a downgrade
        // to slip through. Matches the upstream behavior of
        // checking each timestamp in isolation.
        let Some(ts) = meta.published_at(version) else {
            continue;
        };
        let Some(parsed) = parse_packument_timestamp(ts) else {
            continue;
        };
        if parsed >= before_date {
            continue;
        }
        let Some(manifest) = meta.versions.get(version) else {
            return Err(TrustViolation::TrustCheckFailed {
                reason: format!(
                    "undecodable version object for version {version} of {name} in metadata",
                    name = meta.name,
                ),
            });
        };
        let Some(evidence) = get_trust_evidence(&manifest) else {
            continue;
        };
        // Keep the highest-ranked evidence seen so far. Don't short-
        // circuit on a mid-rank hit: a later version may carry stronger
        // evidence, and missing it would let a real downgrade slip
        // through. Only `StagedPublish` — the maximum rank — ends the
        // walk early.
        if best.is_none_or(|current| trust_rank(evidence) > trust_rank(current)) {
            best = Some(evidence);
            if evidence == TrustEvidence::StagedPublish {
                return Ok(best);
            }
        }
    }
    Ok(best)
}

/// `_npmUser.approver` (a staged publish) outranks everything; failing
/// that, `_npmUser.trustedPublisher` outranks `dist.attestations.provenance`
/// only when the version also carries a provenance attestation;
/// otherwise the publisher flag is ignored and the version falls back
/// to the provenance rank or `None`. Mirrors pnpm's
/// [`getTrustEvidence`](https://github.com/pnpm/pnpm/blob/372cae6a55/resolving/npm-resolver/src/trustChecks.ts#L123-L134).
#[must_use]
pub fn get_trust_evidence(version: &PackageVersion) -> Option<TrustEvidence> {
    let has_approver = version.npm_user.as_ref().and_then(|user| user.approver.as_ref()).is_some();
    if has_approver {
        return Some(TrustEvidence::StagedPublish);
    }
    let has_provenance =
        version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).is_some();
    let has_trusted_publisher =
        version.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref()).is_some();
    if has_trusted_publisher && has_provenance {
        return Some(TrustEvidence::TrustedPublisher);
    }
    if has_provenance {
        return Some(TrustEvidence::Provenance);
    }
    None
}

fn is_prerelease(version: &str) -> bool {
    Version::parse(version).is_ok_and(|parsed| !parsed.pre_release.is_empty())
}

#[cfg(test)]
mod tests;
