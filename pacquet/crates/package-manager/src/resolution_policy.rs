//! The config-derived version-pick policy shared by the install resolver
//! chain and `pacquet add`'s explicit-spec pre-resolution, so both pick
//! byte-identical versions (an `add foo@^1` pins the manifest to the same
//! version the install locks, `minimumReleaseAge` and `resolutionMode`
//! included).

use chrono::{DateTime, Utc};
use pacquet_config::{
    Config, ResolutionMode, TrustPolicy,
    version_policy::{PackageVersionPolicy, VersionPolicyError, create_package_version_policy},
};

/// The version-pick knobs derived purely from [`Config`]. Computed once and
/// fed to both the resolver chain and the `add` pre-resolution so a single
/// definition can't drift between them.
pub(crate) struct PickPolicy {
    /// `resolutionMode: time-based`.
    pub time_based: bool,
    /// `resolutionMode` picks the lowest satisfying direct version.
    pub pick_lowest_direct: bool,
    /// Force full packument metadata (per-version `time`) so the
    /// time-based cutoff and the no-downgrade trust check have publication
    /// dates. Mirrors pnpm's `(time-based || no-downgrade) &&
    /// !registrySupportsTimeField`.
    pub full_metadata: bool,
    /// `minimumReleaseAge` cutoff: only versions published at or before
    /// this instant are eligible. `None` disables the maturity filter.
    pub published_by: Option<DateTime<Utc>>,
    /// `minimumReleaseAgeExclude` policy, exempting matching packages from
    /// the cutoff.
    pub published_by_exclude: Option<PackageVersionPolicy>,
}

impl PickPolicy {
    /// Derive the policy from config, sampling the wall clock for the
    /// `minimumReleaseAge` cutoff. Errors only when
    /// `minimumReleaseAgeExclude` contains an invalid rule.
    ///
    /// The cutoff is anchored to "now" at the moment of the call. When
    /// `minimumReleaseAge` is set (off by default) and two derivations run
    /// at slightly different instants — e.g. `pacquet add`'s explicit-spec
    /// pre-resolution and its follow-up install — a version published in
    /// that sub-second window could in theory flip eligibility. To pin both
    /// to the same instant, derive once via [`Self::from_config_at`] and
    /// share the `now`.
    pub(crate) fn from_config(config: &Config) -> Result<Self, VersionPolicyError> {
        Self::from_config_at(config, chrono::Utc::now())
    }

    /// [`Self::from_config`] with an explicit `now`, so callers that derive
    /// the policy more than once within an operation can anchor every
    /// `minimumReleaseAge` cutoff to the same instant.
    pub(crate) fn from_config_at(
        config: &Config,
        now: DateTime<Utc>,
    ) -> Result<Self, VersionPolicyError> {
        let time_based = config.resolution_mode == ResolutionMode::TimeBased;
        let pick_lowest_direct = config.resolution_mode.picks_lowest_direct();
        let full_metadata = (time_based || config.trust_policy == TrustPolicy::NoDowngrade)
            && !config.registry_supports_time_field;
        // On overflow we leave the policy inactive for this run — better
        // than silently producing a cutoff in the wrong direction.
        let published_by = config.resolved_minimum_release_age().and_then(|minutes| {
            let duration = chrono::Duration::try_minutes(i64::try_from(minutes).ok()?)?;
            now.checked_sub_signed(duration)
        });
        let published_by_exclude = config
            .minimum_release_age_exclude
            .as_deref()
            .filter(|patterns| !patterns.is_empty())
            .map(create_package_version_policy)
            .transpose()?;
        Ok(PickPolicy {
            time_based,
            pick_lowest_direct,
            full_metadata,
            published_by,
            published_by_exclude,
        })
    }
}
