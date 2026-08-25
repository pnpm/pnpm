//! Build the per-install list of [`ResolutionVerifier`]s the lockfile
//! gate fans out across. Currently only the npm-resolver verifier
//! plugs in; future resolver-side verifiers append to the same vec.
//!
//! Returning `Vec<Arc<dyn ResolutionVerifier>>` matches the runner's
//! input shape ([`pnpm_lockfile_verification::verify_lockfile_resolutions()`])
//! and lets the install path skip the call entirely when the vec is
//! empty (the runner is a no-op on `&[]`). The function never returns
//! an error; an invalid exclude pattern surfaces from
//! [`pnpm_config::version_policy::create_package_version_policy()`]
//! and propagates via [`BuildVerifiersError`].
//!
//! The verifier list is built from the install's config fields just
//! before the lockfile-resolution gate runs over it.

use std::{collections::HashMap, sync::Arc};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{
    Config, TrustPolicy,
    version_policy::{PackageVersionPolicy, VersionPolicyError, create_package_version_policy},
};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_resolving_npm_resolver::{
    CreateNpmResolutionVerifierOptions, MergeNamedRegistriesError, ObservedDistStats,
    PackageMetaCache, create_npm_resolution_verifier, merge_named_registries,
};
use pnpm_resolving_resolver_base::{PlannedCanonicalFetches, ResolutionVerifier};

use crate::retry_config::retry_opts_from_config;

/// Error from [`build_resolution_verifiers`]. Wraps the inner error so
/// the install command can route the diagnostic code without
/// re-wrapping.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum BuildVerifiersError {
    /// `minimumReleaseAgeExclude` had an invalid pattern.
    #[display("Invalid value in minimumReleaseAgeExclude: {source}")]
    #[diagnostic(code(ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE))]
    InvalidMinimumReleaseAgeExclude {
        #[error(source)]
        source: VersionPolicyError,
    },

    /// `namedRegistries` had a reserved name or an unusable URL.
    ///
    /// Surfaced here because verifiers are built before the resolver
    /// chain that also validates this, and on the frozen path that
    /// chain never runs at all.
    #[display("{source}")]
    #[diagnostic(transparent)]
    InvalidNamedRegistries {
        #[error(source)]
        source: MergeNamedRegistriesError,
    },

    /// `trustPolicyExclude` had an invalid pattern.
    #[display("Invalid value in trustPolicyExclude: {source}")]
    #[diagnostic(code(ERR_PNPM_INVALID_TRUST_POLICY_EXCLUDE))]
    InvalidTrustPolicyExclude {
        #[error(source)]
        source: VersionPolicyError,
    },
}

/// Assemble the verifier list for this install. The npm verifier is
/// always included — it enforces the tarball-URL binding regardless of
/// policy configuration — so the list is non-empty.
///
/// `meta_cache` is the optional per-install packument cache shared
/// with the resolver. When provided, the verifier reads it before
/// fetching: a `(registry, name)` the resolver already pulled
/// during the same install yields the cached document instead of a
/// fresh round-trip. Pass `None` from contexts where no resolver
/// runs alongside (the frozen-install path, unit tests).
///
/// `observed_dist_stats` is the optional [`ObservedDistStats`] sink
/// the npm verifier fills with each verified entry's `dist` work
/// statistics; pass `None` when the caller has no use for them.
pub fn build_resolution_verifiers(
    config: &Config,
    http_client: Arc<ThrottledClient>,
    meta_cache: Option<Arc<dyn PackageMetaCache>>,
    auth_override: Option<Arc<AuthHeaders>>,
    observed_dist_stats: Option<ObservedDistStats>,
    planned_canonical_fetches: Option<PlannedCanonicalFetches>,
) -> Result<Vec<Arc<dyn ResolutionVerifier>>, BuildVerifiersError> {
    let mut verifiers: Vec<Arc<dyn ResolutionVerifier>> = Vec::new();

    let min_age_exclude = build_policy(
        config.minimum_release_age_exclude.as_deref(),
        BuildVerifiersError::invalid_minimum_release_age_exclude,
    )?;
    let trust_exclude = build_policy(
        config.trust_policy_exclude.as_deref(),
        BuildVerifiersError::invalid_trust_policy_exclude,
    )?;

    let registries: HashMap<String, String> = config.resolved_registries().into_iter().collect();

    // Merged here, not inside the verifier, so its name lookup and its
    // tarball-prefix routing see the same set. Validated here too: this runs
    // before the resolver chain that also validates, and on the frozen path
    // that chain never runs.
    let registries_by_prefix = merge_named_registries(
        &config.registries_by_prefix.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    )
    .map_err(|source| BuildVerifiersError::InvalidNamedRegistries { source })?;

    let opts = CreateNpmResolutionVerifierOptions {
        minimum_release_age: config.resolved_minimum_release_age(),
        registry_supports_time_field: config.registry_supports_time_field,
        minimum_release_age_exclude: min_age_exclude,
        minimum_release_age_exclude_patterns: config
            .minimum_release_age_exclude
            .clone()
            .unwrap_or_default(),
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        trust_policy: match config.trust_policy {
            TrustPolicy::Off => None,
            TrustPolicy::NoDowngrade => Some(TrustPolicy::NoDowngrade),
        },
        trust_policy_exclude: trust_exclude,
        trust_policy_exclude_patterns: config.trust_policy_exclude.clone().unwrap_or_default(),
        trust_policy_ignore_after: config.trust_policy_ignore_after,
        registries,
        registries_by_prefix,
        http_client,
        auth_headers: auth_override.unwrap_or_else(|| Arc::clone(&config.auth_headers)),
        cache_dir: Some(config.cache_dir.clone()),
        meta_cache,
        offline: config.offline,
        retry_opts: retry_opts_from_config(config),
        now: None,
        observed_dist_stats,
        planned_canonical_fetches,
    };

    verifiers.push(Arc::new(create_npm_resolution_verifier(opts)));

    Ok(verifiers)
}

fn build_policy(
    patterns: Option<&[String]>,
    wrap_error: fn(VersionPolicyError) -> BuildVerifiersError,
) -> Result<Option<PackageVersionPolicy>, BuildVerifiersError> {
    let Some(patterns) = patterns else { return Ok(None) };
    if patterns.is_empty() {
        return Ok(None);
    }
    create_package_version_policy(patterns).map(Some).map_err(wrap_error)
}

impl BuildVerifiersError {
    fn invalid_minimum_release_age_exclude(source: VersionPolicyError) -> Self {
        BuildVerifiersError::InvalidMinimumReleaseAgeExclude { source }
    }

    fn invalid_trust_policy_exclude(source: VersionPolicyError) -> Self {
        BuildVerifiersError::InvalidTrustPolicyExclude { source }
    }
}

#[cfg(test)]
mod tests;
