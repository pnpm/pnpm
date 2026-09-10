//! Npm-side implementation of the [`ResolutionVerifier`] trait.
//!
//! The factory takes the install-time policy (cutoff time, exclude
//! patterns, trust policy, named registries) and returns a verifier.
//! The verifier inspects each npm-registry-resolved lockfile entry: it
//! always requires a tarball hash and binds the recorded tarball URL to
//! the artifact the registry's metadata lists (anti-tamper checks
//! independent of any policy), and additionally applies the
//! `minimumReleaseAge` and/or `trustPolicy='no-downgrade'` checks when
//! those are configured.
//! Violations surface through [`ResolutionVerification::Err`].
//!
//! The publish-timestamp lookup walks a 4-layer fallback chain
//! (abbreviated-modified shortcut → local mirror → attestation
//! endpoint → full packument fetch); the trust check separately
//! reads the full packument to walk version history. Per-install
//! dedup of every network/disk call lives in
//! [`PublishedAtLookupContext`] so verifying many pinned versions of
//! the same package costs at most one fetch per layer.

mod registry_tarball;
use registry_tarball::npm_registry_tarball;

mod trust_check;

mod policy_snapshot;
use policy_snapshot::{
    BuildPolicySnapshot, build_policy_snapshot, cached_policy_patterns, minimum_release_age_cutoff,
    named_registries_routing_digest, sorted_unique,
};

mod registry_artifact;

mod metadata_fetch;

mod age_check;

mod artifact_binding;
use artifact_binding::{
    canonical_tarball_url, current_history_violation, current_revision_number, lockfile_revision,
    missing_artifact_violation, select_revision, tarball_url_violation,
};

mod metadata_projection;
use metadata_projection::{load_local_meta_time, project_abbreviated_meta, project_trust_meta};

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use miette::Diagnostic as _;
use pipe_trait::Pipe;
use pnpm_config::{TrustPolicy, version_policy::PackageVersionPolicy};
use pnpm_lockfile::{
    LockfileResolution, PkgName, TarballRevision, is_git_hosted_tarball_url,
    is_integrity_addressed_registry_tarball_url,
};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient, redact_url_credentials};
use pnpm_registry::{
    Approver, DerivedPackuments, NpmUser, Package, PackageDistribution, PackageVersion,
};
use pnpm_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture, parse_packument_timestamp,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::{
    FetchAttestationOptions, FetchFullMetadataCachedOptions, TrustCheckOptions, TrustViolation,
    fetch_attestation_published_at, fetch_full_metadata_cached,
    lookup_context::{
        PublishedAtLookupContext, PublishedAtTimeMap, RegistryArtifact, RegistryArtifactHistory,
        package_key, version_key,
    },
    named_registry::{named_registry_tarball_prefixes, pick_registry_for_package},
    pick_package::{PackageMetaCache, SkippedTimeCheck, warn_missing_time_once},
    registry_url::to_registry_url,
    trust_checks::fail_if_trust_downgraded,
    violation_codes::{
        MINIMUM_RELEASE_AGE_VIOLATION_CODE, MISSING_NAMED_REGISTRY_VIOLATION_CODE,
        MISSING_TARBALL_INTEGRITY_VIOLATION_CODE, TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
        TARBALL_URL_MISMATCH_VIOLATION_CODE, TRUST_DOWNGRADE_VIOLATION_CODE,
    },
};

/// Per-version `dist` statistics that estimate a tarball's pipeline
/// work: `unpackedSize` (transfer + decompress + hash bytes) and
/// `fileCount` (per-file CAS-write overhead). Either may be absent —
/// registries only publish them for packages uploaded since npm 6.
#[derive(Debug, Default, Clone, Copy)]
pub struct DistStats {
    pub unpacked_size: Option<usize>,
    pub file_count: Option<usize>,
}

/// `(package name, version) → dist` work statistics filled by the
/// verifier as a side product of the tarball-URL binding check. The
/// metadata is already in hand per entry, so collecting costs no extra
/// fetch; consumers (the pnpr server's frozen fast path) use the stats
/// to schedule the most expensive tarball downloads first. Shared as an
/// `Arc` so the caller keeps a handle while the verifier fan-out writes.
pub type ObservedDistStats = Arc<DashMap<(String, String), DistStats>>;

/// Construct a fresh sink for
/// [`CreateNpmResolutionVerifierOptions::observed_dist_stats`].
#[must_use]
pub fn observed_dist_stats_sink() -> ObservedDistStats {
    Arc::new(DashMap::new())
}

/// Options bundle for [`create_npm_resolution_verifier`].
///
/// The verifier owns the option bag once constructed — these fields
/// flow into [`NpmResolutionVerifier`] verbatim.
pub struct CreateNpmResolutionVerifierOptions {
    /// Minimum age in **minutes** a published version must reach
    /// before it is accepted. `None` disables the age check.
    pub minimum_release_age: Option<u64>,
    /// Wildcard / exact-version patterns whose packages skip the age
    /// check. `None` (or empty) means "no exclusions".
    pub minimum_release_age_exclude: Option<PackageVersionPolicy>,
    /// Raw spec strings backing [`Self::minimum_release_age_exclude`].
    /// The verifier keeps the strings — not the compiled policy — for
    /// the cache snapshot in `policy()` so the persisted record can be
    /// compared byte-for-byte across runs.
    pub minimum_release_age_exclude_patterns: Vec<String>,
    /// Backs the `minimumReleaseAgeIgnoreMissingTime` opt-in: when
    /// `true` and the registry strips per-version `time`, the verifier
    /// passes the entry instead of failing closed. Applies to the
    /// maturity cutoff and to the trust check, which has no publish
    /// order to walk without the field either. Scoped to a packument
    /// with no usable `time` map: one that dates every version it lists
    /// is saying it never published this pin, which fails closed either
    /// way. Default `false`.
    pub ignore_missing_time_field: bool,
    /// Backs `registrySupportsTimeField`: the registry serves the
    /// per-version `time` map in abbreviated metadata (Verdaccio
    /// 5.15.1+, pnpr), so the verifier can take a version's publish
    /// timestamp from the document it already fetched instead of
    /// paying an attestation round-trip and a full-packument download
    /// per cold-cache package. Default `false`.
    pub registry_supports_time_field: bool,
    /// `'no-downgrade'` enables the trust check;
    /// [`TrustPolicy::Off`] disables it. Stored as an [`Option`] so
    /// `None` and `Some(Off)` both disable the check while still
    /// snapshotting differently for `policy()` (`null` vs the explicit
    /// `off`).
    pub trust_policy: Option<TrustPolicy>,
    pub trust_policy_exclude: Option<PackageVersionPolicy>,
    pub trust_policy_exclude_patterns: Vec<String>,
    /// Maximum age (in minutes) before which the trust check still
    /// applies. `None` means "always check".
    pub trust_policy_ignore_after: Option<u64>,
    /// `default` + per-scope registry map. Keyed by `"default"` or
    /// `"@scope"`.
    pub registries: HashMap<String, String>,
    /// User-defined named-registry aliases (e.g. `gh:` →
    /// `https://npm.pkg.github.com/`). Merged with
    /// [`crate::BUILTIN_REGISTRIES_BY_PREFIX`].
    pub registries_by_prefix: HashMap<String, String>,
    pub http_client: Arc<ThrottledClient>,
    pub auth_headers: Arc<AuthHeaders>,
    /// Root of pnpm's on-disk metadata mirror. When set, the verifier
    /// reads conditional headers from
    /// `<cache_dir>/v11/metadata-full/<registry>/<pkg>.jsonl` and
    /// writes 200 responses back; when `None`, every fetch is
    /// unconditional.
    pub cache_dir: Option<PathBuf>,
    /// Per-install [`PackageMetaCache`] shared with the npm resolver.
    /// When provided, the verifier reads a cached packument before
    /// fetching — a name the resolver already pulled during the same
    /// install yields the cached document instead of a fresh
    /// disk/network round-trip. Optional: frozen-install paths and
    /// unit tests don't have a resolver running alongside, in which
    /// case the verifier falls back to its own fetch chain.
    pub meta_cache: Option<Arc<dyn PackageMetaCache>>,
    /// When true, verifier metadata lookups must use the local mirror
    /// only and never reach the registry or attestation endpoint.
    pub offline: bool,
    /// Retry budget for the verifier's metadata and attestation
    /// fetches. Sourced from the same `fetch-retries` config the
    /// resolver and tarball paths use.
    pub retry_opts: RetryOpts,
    /// Override for `Utc::now()` when computing the age cutoff and
    /// the `trustPolicyIgnoreAfter` window. `None` falls back to
    /// wall-clock at construction time.
    pub now: Option<DateTime<Utc>>,
    /// Optional sink the verifier fills with each verified entry's
    /// `dist` work statistics (see [`ObservedDistStats`]). `None`
    /// skips collection.
    pub observed_dist_stats: Option<ObservedDistStats>,
    /// Fetch evidence the materialization path fills after its
    /// warm/cold partition (see
    /// [`pnpm_resolving_resolver_base::PlannedCanonicalFetches`]).
    /// When supplied, an entry listed there passes the age check on a
    /// package-level `Last-Modified` HEAD probe alone — the planned
    /// canonical fetch fail-closes the entry's registry existence, so
    /// no metadata body is needed. `None` (paths that materialize
    /// nothing or run a resolver alongside) keeps the metadata-backed
    /// chain for every entry.
    pub planned_canonical_fetches: Option<pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
}

/// Verifier returned by [`create_npm_resolution_verifier`]. Stores
/// the resolved cutoff, the named-registry prefix list, the dedup
/// caches, and the pre-built policy snapshot the cache reads via
/// [`ResolutionVerifier::policy`].
pub struct NpmResolutionVerifier {
    minimum_release_age_minutes: Option<u64>,
    cutoff: Option<DateTime<Utc>>,
    minimum_release_age_exclude: Option<PackageVersionPolicy>,
    ignore_missing_time_field: bool,
    registry_supports_time_field: bool,
    trust_policy: Option<TrustPolicy>,
    trust_policy_exclude: Option<PackageVersionPolicy>,
    trust_policy_ignore_after: Option<u64>,
    /// Saved copy of the trust-exclude patterns so [`TrustCheckOptions`]
    /// can borrow them per-call without reconstructing the policy.
    /// Kept in sync with `trust_policy_exclude`.
    sorted_min_age_excludes: Vec<String>,
    sorted_trust_excludes: Vec<String>,
    registries: HashMap<String, String>,
    named_registry_prefixes: Vec<String>,
    /// Alias → URL map (built-ins merged with the user's setting) for
    /// routing registry-qualified lockfile keys, which carry no tarball
    /// URL for the prefix list to match.
    registries_by_prefix: HashMap<String, String>,
    http_client: Arc<ThrottledClient>,
    auth_headers: Arc<AuthHeaders>,
    cache_dir: Option<PathBuf>,
    meta_cache: Option<Arc<dyn PackageMetaCache>>,
    offline: bool,
    retry_opts: RetryOpts,
    now: Option<DateTime<Utc>>,
    policy_snapshot: serde_json::Map<String, JsonValue>,
    lookup_context: PublishedAtLookupContext,
    observed_dist_stats: Option<ObservedDistStats>,
    planned_canonical_fetches: Option<pnpm_resolving_resolver_base::PlannedCanonicalFetches>,
}

impl std::fmt::Debug for NpmResolutionVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NpmResolutionVerifier")
            .field("minimum_release_age_minutes", &self.minimum_release_age_minutes)
            .field("cutoff", &self.cutoff)
            .field("ignore_missing_time_field", &self.ignore_missing_time_field)
            .field("registry_supports_time_field", &self.registry_supports_time_field)
            .field("trust_policy", &self.trust_policy)
            .field("trust_policy_ignore_after", &self.trust_policy_ignore_after)
            .field("offline", &self.offline)
            .field("sorted_min_age_excludes", &self.sorted_min_age_excludes)
            .field("sorted_trust_excludes", &self.sorted_trust_excludes)
            .field("policy_snapshot", &self.policy_snapshot)
            .finish_non_exhaustive()
    }
}

/// Builds the [`NpmResolutionVerifier`]. It always requires a tarball
/// hash and binds each entry's recorded tarball URL to the artifact the
/// registry's metadata lists (anti-tamper checks independent of any
/// policy), and additionally applies the `minimum_release_age` /
/// `trust_policy='no-downgrade'` checks when those are configured.
#[must_use]
pub fn create_npm_resolution_verifier(
    opts: CreateNpmResolutionVerifierOptions,
) -> NpmResolutionVerifier {
    let cutoff = minimum_release_age_cutoff(&opts);

    let named_registry_prefixes = named_registry_tarball_prefixes(&opts.registries_by_prefix);

    let sorted_min_age_excludes = sorted_unique(&opts.minimum_release_age_exclude_patterns);
    let sorted_trust_excludes = sorted_unique(&opts.trust_policy_exclude_patterns);
    let named_registries_routing = named_registries_routing_digest(&opts.registries_by_prefix);

    let policy_snapshot = build_policy_snapshot(&BuildPolicySnapshot {
        minimum_release_age: opts.minimum_release_age.unwrap_or(0),
        sorted_min_age_excludes: &sorted_min_age_excludes,
        ignore_missing_time_field: opts.ignore_missing_time_field,
        trust_policy: opts.trust_policy,
        sorted_trust_excludes: &sorted_trust_excludes,
        trust_policy_ignore_after: opts.trust_policy_ignore_after,
        named_registries_routing: &named_registries_routing,
    });

    NpmResolutionVerifier {
        minimum_release_age_minutes: opts.minimum_release_age,
        cutoff,
        minimum_release_age_exclude: opts.minimum_release_age_exclude,
        ignore_missing_time_field: opts.ignore_missing_time_field,
        registry_supports_time_field: opts.registry_supports_time_field,
        trust_policy: opts.trust_policy,
        trust_policy_exclude: opts.trust_policy_exclude,
        trust_policy_ignore_after: opts.trust_policy_ignore_after,
        sorted_min_age_excludes,
        sorted_trust_excludes,
        registries: opts.registries,
        named_registry_prefixes,
        registries_by_prefix: opts.registries_by_prefix,
        http_client: opts.http_client,
        auth_headers: opts.auth_headers,
        cache_dir: opts.cache_dir,
        meta_cache: opts.meta_cache,
        offline: opts.offline,
        retry_opts: opts.retry_opts,
        now: opts.now,
        policy_snapshot,
        lookup_context: PublishedAtLookupContext::new(),
        observed_dist_stats: opts.observed_dist_stats,
        planned_canonical_fetches: opts.planned_canonical_fetches,
    }
}

impl ResolutionVerifier for NpmResolutionVerifier {
    fn might_verify(&self, resolution: &LockfileResolution, ctx: VerifyCtx<'_>) -> bool {
        let Some(tarball_url) = npm_registry_tarball(resolution) else {
            return false;
        };
        if tarball_url.is_some() || resolution.checkable_integrity().is_none() {
            return true;
        }
        self.age_check_active()
            && !is_excluded(self.minimum_release_age_exclude.as_ref(), ctx.name, ctx.version)
            || self.trust_check_active()
                && !is_excluded(self.trust_policy_exclude.as_ref(), ctx.name, ctx.version)
    }

    fn verify<'a>(
        &'a self,
        resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(self.verify_impl(resolution, ctx))
    }

    fn policy(&self) -> &serde_json::Map<String, JsonValue> {
        &self.policy_snapshot
    }

    fn can_trust_past_check(&self, cached_policy: &serde_json::Map<String, JsonValue>) -> bool {
        if !self.past_check_has_structural_rules(cached_policy) {
            return false;
        }

        // Maturity: a previously cached run under a larger cutoff
        // (stricter window) is trustworthy under a smaller current one
        // — the set of accepted versions is a subset of today's.
        // Tightening the cutoff invalidates the cached run.
        let past_min_age =
            cached_policy.get("minimumReleaseAge").and_then(JsonValue::as_u64).unwrap_or(0);
        if past_min_age < self.minimum_release_age_minutes.unwrap_or(0) {
            return false;
        }

        let past_min_age_excludes =
            cached_policy_patterns(cached_policy, "minimumReleaseAgeExclude");
        if past_min_age_excludes != self.sorted_min_age_excludes {
            return false;
        }

        let past_trust_policy = cached_policy.get("trustPolicy").and_then(JsonValue::as_str);
        let today_trust_policy = self.trust_policy_wire_str();
        if past_trust_policy != today_trust_policy {
            return false;
        }

        let past_trust_excludes = cached_policy_patterns(cached_policy, "trustPolicyExclude");
        if past_trust_excludes != self.sorted_trust_excludes {
            return false;
        }

        let past_ignore_after =
            cached_policy.get("trustPolicyIgnoreAfter").and_then(JsonValue::as_u64);
        if past_ignore_after != self.trust_policy_ignore_after {
            return false;
        }

        // Missing-time tolerance: a cached run that failed closed on an
        // absent `time` field accepted a subset of what today's tolerant
        // policy accepts, so it stays trustworthy. Turning the tolerance
        // off invalidates it — entries the past run waved through are the
        // ones today's policy exists to reject. Older records (no field)
        // read as intolerant, which is the safe direction.
        let past_ignore_missing_time = cached_policy
            .get("minimumReleaseAgeIgnoreMissingTime")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        if past_ignore_missing_time && !self.ignore_missing_time_field {
            return false;
        }

        true
    }
}

impl NpmResolutionVerifier {
    async fn verify_impl(
        &self,
        resolution: &LockfileResolution,
        ctx: VerifyCtx<'_>,
    ) -> ResolutionVerification {
        let Some(tarball_url) = npm_registry_tarball(resolution) else {
            return ResolutionVerification::Ok;
        };

        // Network-free structural check, so it runs before the registry
        // metadata shortcuts below. An entry that pins no hash cannot be
        // verified against anything once fetched, whatever its version
        // shape — a URL-keyed dep is refused here too.
        if resolution.checkable_integrity().is_none() {
            return ResolutionVerification::Err {
                code: MISSING_TARBALL_INTEGRITY_VIOLATION_CODE,
                reason: r#"has no "integrity" field, so its downloaded tarball cannot be verified"#
                    .to_string(),
            };
        }

        // A key that is not a plain semver version names no registry
        // artifact to check against.
        if node_semver::Version::parse(ctx.version).is_err() {
            return ResolutionVerification::Ok;
        }

        let named_registry = match self.named_registry_url(ctx.registry_name) {
            Ok(named_registry) => named_registry,
            Err(violation) => return violation,
        };

        let (age_applies, trust_applies) = self.policies_for(&ctx);
        if tarball_url.is_none() && !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }
        let registry = named_registry.unwrap_or_else(|| self.pick_registry(ctx.name, tarball_url));

        if let Some(violation) = self
            .run_artifact_binding(
                &registry,
                &ctx,
                resolution,
                tarball_url,
                age_applies || trust_applies,
            )
            .await
        {
            return violation;
        }

        self.run_policy_checks(&registry, &ctx, age_applies, trust_applies).await
    }

    /// A registry entry that pins an explicit tarball URL must point at the
    /// artifact the registry's own metadata lists. Otherwise a trusted
    /// name@version could front bytes from an attacker-chosen URL (with a
    /// matching integrity for those bytes). This binding is unconditional —
    /// it does not depend on the minimum-release-age / trust policies and
    /// isn't narrowed by their exclude lists, since it guards integrity
    /// rather than maturity/trust.
    async fn run_artifact_binding(
        &self,
        registry: &str,
        ctx: &VerifyCtx<'_>,
        resolution: &LockfileResolution,
        tarball_url: Option<&str>,
        policies_apply: bool,
    ) -> Option<ResolutionVerification> {
        let binds_an_artifact =
            tarball_url.is_some() || (lockfile_revision(resolution).is_some() && policies_apply);
        if !binds_an_artifact {
            return None;
        }
        self.run_registry_artifact_check(registry, ctx.name, ctx.version, resolution, tarball_url)
            .await
    }

    /// Whether the maturity and trust policies apply to this entry.
    fn policies_for(&self, ctx: &VerifyCtx<'_>) -> (bool, bool) {
        let age_applies = self.age_check_active()
            && !is_excluded(self.minimum_release_age_exclude.as_ref(), ctx.name, ctx.version);
        let trust_applies = self.trust_check_active()
            && !is_excluded(self.trust_policy_exclude.as_ref(), ctx.name, ctx.version);
        (age_applies, trust_applies)
    }

    /// The maturity and trust policies, each skipped when it does not apply
    /// to this entry.
    async fn run_policy_checks(
        &self,
        registry: &str,
        ctx: &VerifyCtx<'_>,
        age_applies: bool,
        trust_applies: bool,
    ) -> ResolutionVerification {
        if age_applies
            && let Some(violation) =
                self.run_age_check(registry, ctx.name, ctx.version, ctx.registry_name).await
        {
            return violation;
        }
        if trust_applies
            && let Some(violation) = self.run_trust_check(registry, ctx.name, ctx.version).await
        {
            return violation;
        }
        ResolutionVerification::Ok
    }

    /// The URL a registry-qualified entry routes to.
    ///
    /// Registry-qualified entries name their registry in the dep path, so
    /// routing does not depend on a recorded tarball URL (canonical URLs are
    /// omitted from the lockfile in the 12.0 format). This fails closed on
    /// an unknown alias: none of the metadata-backed checks could vouch for
    /// the entry without its registry URL.
    fn named_registry_url(
        &self,
        registry_name: Option<&str>,
    ) -> Result<Option<String>, ResolutionVerification> {
        let Some(registry_name) = registry_name else { return Ok(None) };
        match self.registries_by_prefix.get(registry_name) {
            Some(url) => Ok(Some(url.clone())),
            None => Err(ResolutionVerification::Err {
                code: MISSING_NAMED_REGISTRY_VIOLATION_CODE,
                reason: format!(
                    "has registry prefix '{registry_name}:', which is not declared by the registries setting",
                ),
            }),
        }
    }

    fn age_check_active(&self) -> bool {
        self.minimum_release_age_minutes.is_some_and(|minutes| minutes > 0)
    }

    fn pick_registry(&self, name: &PkgName, tarball_url: Option<&str>) -> String {
        if let Some(url) = tarball_url {
            // Match on the same canonical form the tarball comparison uses, so
            // a named-registry tarball that differs from the configured base
            // only by scheme or `%2f` encoding still routes to its registry
            // instead of falling back (and then failing closed against the
            // wrong packument).
            let normalized = canonical_tarball_url(url);
            for prefix in &self.named_registry_prefixes {
                if normalized.starts_with(&canonical_tarball_url(prefix)) {
                    return prefix.clone();
                }
            }
        }
        pick_registry_for_package(&self.registries, &name.to_string(), None)
    }
}

fn render_fetch_metadata_error(error: &crate::FetchMetadataError) -> String {
    let code = error.code().map(|code| code.to_string());
    let message = redact_url_credentials(&error.to_string());
    match code {
        Some(code) => format!("{code}: {message}"),
        None => message,
    }
}

fn is_excluded(policy: Option<&PackageVersionPolicy>, name: &PkgName, version: &str) -> bool {
    let Some(policy) = policy else { return false };
    match policy.matches(&name.to_string()) {
        pnpm_config::version_policy::PolicyMatch::No => false,
        pnpm_config::version_policy::PolicyMatch::AnyVersion => true,
        pnpm_config::version_policy::PolicyMatch::ExactVersions(versions) => {
            versions.iter().any(|exact| exact == version)
        }
    }
}

fn uncheckable(policy: &str, why: &str) -> String {
    format!("could not be checked against {policy} ({why})")
}

fn format_trust_violation(err: TrustViolation) -> String {
    match err {
        TrustViolation::TrustCheckFailed { reason } => uncheckable("trustPolicy", &reason),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
