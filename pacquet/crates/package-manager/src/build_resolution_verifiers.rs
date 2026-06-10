//! Build the per-install list of [`ResolutionVerifier`]s the lockfile
//! gate fans out across. Currently only the npm-resolver verifier
//! plugs in; future resolver-side verifiers append to the same vec.
//!
//! Returning `Vec<Arc<dyn ResolutionVerifier>>` matches the runner's
//! input shape ([`pacquet_lockfile_verification::verify_lockfile_resolutions()`])
//! and lets the install path skip the call entirely when the vec is
//! empty (the runner is a no-op on `&[]`). The function never returns
//! an error; an invalid exclude pattern surfaces from
//! [`pacquet_config::version_policy::create_package_version_policy()`]
//! and propagates via [`BuildVerifiersError`].
//!
//! Mirrors the install-site wiring at
//! [`installing/deps-installer/src/install/index.ts:355-383`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/installing/deps-installer/src/install/index.ts#L355-L383),
//! where pnpm builds the verifier list from the same set of config
//! fields just before invoking `verifyLockfileResolutions`.

use std::{collections::HashMap, sync::Arc};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::{
    Config, TrustPolicy,
    version_policy::{PackageVersionPolicy, VersionPolicyError, create_package_version_policy},
};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_resolving_npm_resolver::{
    CreateNpmResolutionVerifierOptions, ObservedDistStats, PackageMetaCache,
    create_npm_resolution_verifier,
};
use pacquet_resolving_resolver_base::ResolutionVerifier;

use crate::retry_config::retry_opts_from_config;

/// Error from [`build_resolution_verifiers`]. Today the only thing
/// that can fail is `create_package_version_policy` rejecting an
/// invalid `minimumReleaseAgeExclude` / `trustPolicyExclude`
/// pattern. Wraps the inner error so the install command can route
/// the upstream diagnostic code (`ERR_PNPM_INVALID_VERSION_UNION`,
/// `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION`) without re-wrapping.
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

    // Pacquet's `Config` carries a single registry URL; multi-scope
    // routing lives in `.npmrc` parsing pacquet doesn't surface here
    // yet. Build the minimal `{"default": registry}` map the verifier
    // expects, so scope routing degrades to "always default".
    let mut registries = HashMap::with_capacity(1);
    registries.insert("default".to_string(), config.registry.clone());

    let opts = CreateNpmResolutionVerifierOptions {
        minimum_release_age: config.resolved_minimum_release_age(),
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
        // User-defined aliases from `pnpm-workspace.yaml#namedRegistries`.
        // Built-in aliases are merged in by
        // [`pacquet_resolving_npm_resolver::build_named_registry_prefixes()`]
        // inside the verifier, so passing the user map verbatim
        // matches upstream's
        // [`createNpmResolutionVerifier`](https://github.com/pnpm/pnpm/blob/b61e268d57/resolving/npm-resolver/src/createNpmResolutionVerifier.ts)
        // call site, which forwards the same yaml value through.
        named_registries: config
            .named_registries
            .iter()
            .map(|(name, url)| (name.clone(), url.clone()))
            .collect(),
        http_client,
        auth_headers: auth_override.unwrap_or_else(|| Arc::clone(&config.auth_headers)),
        cache_dir: Some(config.cache_dir.clone()),
        meta_cache,
        retry_opts: retry_opts_from_config(config),
        now: None,
        observed_dist_stats,
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
