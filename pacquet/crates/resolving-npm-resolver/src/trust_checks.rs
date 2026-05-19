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
//! - `trustedPublisher` (rank 2) — `_npmUser.trustedPublisher` is
//!   present.
//! - `provenance` (rank 1) — `dist.attestations.provenance` is
//!   present (and no trusted-publisher record).
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

/// Rank of supply-chain evidence on a single version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustEvidence {
    /// `dist.attestations.provenance` is set.
    Provenance,
    /// `_npmUser.trustedPublisher` is set (overrides provenance —
    /// it's a stronger signal that a known upstream pipeline
    /// published the version).
    TrustedPublisher,
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
    let version_date = DateTime::parse_from_rfc3339(published_at)
        .map_err(|err| TrustViolation::TrustCheckFailed {
            reason: format!("publish timestamp is not a valid date: {err}"),
        })?
        .with_timezone(&Utc);

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
    let strongest_prior =
        match detect_strongest_trust_evidence_before(meta, version_date, exclude_prerelease) {
            Some(rank) => rank,
            None => return Ok(()),
        };

    let current = get_trust_evidence(manifest);
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
/// [`trustChecks.ts:10-13`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/trustChecks.ts#L10-L13).
/// Upstream uses `undefined` for "no evidence"; the Rust port uses
/// `Option<TrustEvidence>` so callers compare ranks via
/// `Option::map_or(0, trust_rank)`.
fn trust_rank(evidence: TrustEvidence) -> u8 {
    match evidence {
        TrustEvidence::TrustedPublisher => 2,
        TrustEvidence::Provenance => 1,
    }
}

fn pretty_print_trust_evidence(evidence: Option<TrustEvidence>) -> &'static str {
    match evidence {
        Some(TrustEvidence::TrustedPublisher) => "trusted publisher",
        Some(TrustEvidence::Provenance) => "provenance attestation",
        None => "no trust evidence",
    }
}

/// Walk every version older than `before_date` and return the
/// strongest [`TrustEvidence`] seen. Prereleases are filtered out
/// when the current version is *not* itself a prerelease — matches
/// upstream's `semver.prerelease(version, true)` guard.
fn detect_strongest_trust_evidence_before(
    meta: &Package,
    before_date: DateTime<Utc>,
    exclude_prerelease: bool,
) -> Option<TrustEvidence> {
    let mut best: Option<TrustEvidence> = None;
    for (version, manifest) in &meta.versions {
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
        let parsed = match DateTime::parse_from_rfc3339(ts) {
            Ok(parsed) => parsed.with_timezone(&Utc),
            Err(_) => continue,
        };
        if parsed >= before_date {
            continue;
        }
        let Some(evidence) = get_trust_evidence(manifest) else {
            continue;
        };
        if matches!(evidence, TrustEvidence::TrustedPublisher) {
            return Some(TrustEvidence::TrustedPublisher);
        }
        // First provenance hit sticks; a later trusted-publisher
        // hit would have returned above.
        if best.is_none() {
            best = Some(TrustEvidence::Provenance);
        }
    }
    best
}

/// `_npmUser.trustedPublisher` outranks `dist.attestations.provenance`;
/// absence of both yields `None`. Mirrors pnpm's
/// [`getTrustEvidence`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/trustChecks.ts#L119-L127).
pub fn get_trust_evidence(version: &PackageVersion) -> Option<TrustEvidence> {
    if version.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref()).is_some() {
        return Some(TrustEvidence::TrustedPublisher);
    }
    if version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).is_some() {
        return Some(TrustEvidence::Provenance);
    }
    None
}

fn is_prerelease(version: &str) -> bool {
    Version::parse(version).map(|parsed| !parsed.pre_release.is_empty()).unwrap_or(false)
}

#[cfg(test)]
mod tests;
