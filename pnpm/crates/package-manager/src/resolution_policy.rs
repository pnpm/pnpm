//! The config-derived version-pick policy shared by the install resolver
//! chain and command-level registry lookups, so they pick byte-identical
//! versions (`minimumReleaseAge` and `resolutionMode` included).

use crate::retry_config::retry_opts_from_config;
use chrono::{DateTime, Utc};
use pnpm_config::{
    Config, NeedsFullMetadataFor, ResolutionMode,
    version_policy::{PackageVersionPolicy, VersionPolicyError, create_package_version_policy},
};
use pnpm_network::ThrottledClient;
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, MergeNamedRegistriesError, NpmResolver, PackumentFetchLocker,
    PickPackageContext, merge_named_registries, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use std::sync::Arc;

/// The version-pick knobs derived purely from [`Config`]. Computed once per
/// command so every lookup in that run shares the same cutoff and metadata
/// policy.
pub struct PickPolicy {
    /// `resolutionMode: time-based`.
    pub time_based: bool,
    /// `resolutionMode` picks the lowest satisfying direct version.
    pub pick_lowest_direct: bool,
    /// Force full packument metadata (per-version `time`) so the
    /// time-based cutoff and the no-downgrade trust check have publication
    /// dates. Mirrors pnpm's `(time-based || no-downgrade) &&
    /// !registrySupportsTimeField`.
    pub full_metadata: bool,
    /// The same question asked of one registry, so a registry that declares
    /// `supportsTimeField` is not charged for full metadata because another
    /// one needs it.
    pub needs_full_metadata_for: NeedsFullMetadataFor,
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
    /// The cutoff is anchored to "now" at the moment of the call. Share the
    /// returned policy across an operation so every lookup uses the same
    /// cutoff instant.
    pub fn from_config(config: &Config) -> Result<Self, VersionPolicyError> {
        Self::from_config_at(config, chrono::Utc::now())
    }

    /// [`Self::from_config`] with extra `minimumReleaseAgeExclude` specs
    /// merged on top of the config value before the exclude policy is
    /// compiled. `pacquet audit --fix update` uses this to let the resolver
    /// install patched versions that the maturity cutoff would otherwise
    /// block. With no extra specs this is exactly [`Self::from_config`].
    pub(crate) fn from_config_with_extra_excludes(
        config: &Config,
        extra_excludes: Option<&[String]>,
    ) -> Result<Self, VersionPolicyError> {
        let mut policy = Self::from_config(config)?;
        let Some(extra) = extra_excludes.filter(|extra| !extra.is_empty()) else {
            return Ok(policy);
        };
        let mut merged = config.minimum_release_age_exclude.clone().unwrap_or_default();
        merged.extend(extra.iter().cloned());
        policy.published_by_exclude = Some(create_package_version_policy(&merged)?);
        Ok(policy)
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
        let full_metadata = config.requires_full_metadata_for_resolution();
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
            needs_full_metadata_for: config.requires_full_metadata_for_registry_fn(),
            published_by,
            published_by_exclude,
        })
    }
}

/// Constructs the npm resolver used by config-driven, command-level version
/// lookups with the same registry, authentication, cache, network, and metadata
/// settings as an install. The supplied [`PickPolicy`] keeps version selection
/// aligned with the install operation that derived it.
///
/// # Errors
///
/// Returns an error when the configured named-registry prefixes cannot be
/// merged into an unambiguous resolver map.
pub fn create_configured_npm_resolver(
    config: &Config,
    http_client: Arc<ThrottledClient>,
    policy: &PickPolicy,
) -> Result<NpmResolver<InMemoryPackageMetaCache>, MergeNamedRegistriesError> {
    let registries_by_prefix =
        merge_named_registries(&config.registries_by_prefix.clone().into_iter().collect())?;
    Ok(NpmResolver {
        registries: config.resolved_registries().into_iter().collect(),
        registries_by_prefix,
        http_client,
        auth_headers: Arc::clone(&config.auth_headers),
        meta_cache: Arc::<InMemoryPackageMetaCache>::default(),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata: policy.full_metadata,
        needs_full_metadata_for: Some(Arc::clone(&policy.needs_full_metadata_for)),
        filter_metadata: config.requires_filtered_full_metadata(),
        retry_opts: retry_opts_from_config(config),
    })
}

/// Build the [`PickPackageContext`] shared by every config-driven pick in a
/// `pacquet add`/`update` pre-resolution, so every pre-resolution derives
/// byte-identical context.
///
/// `meta_cache` and `fetch_locker` are borrowed from caller-owned locals: each
/// pre-resolution runs its own short-lived cache rather than sharing the
/// install's.
pub(crate) fn pick_package_context<'a>(
    http_client: &'a ThrottledClient,
    config: &'a Config,
    policy: &'a PickPolicy,
    meta_cache: &'a InMemoryPackageMetaCache,
    fetch_locker: &'a PackumentFetchLocker,
) -> PickPackageContext<'a, InMemoryPackageMetaCache> {
    PickPackageContext {
        http_client,
        auth_headers: &config.auth_headers,
        meta_cache,
        fetch_locker,
        cache_dir: Some(&config.cache_dir),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata: policy.full_metadata,
        needs_full_metadata_for: Some(policy.needs_full_metadata_for.as_ref()),
        filter_metadata: config.requires_filtered_full_metadata(),
        retry_opts: retry_opts_from_config(config),
    }
}
