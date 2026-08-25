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
    lookup_context::{PublishedAtLookupContext, PublishedAtTimeMap, package_key, version_key},
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
pub fn create_npm_resolution_verifier(
    opts: CreateNpmResolutionVerifierOptions,
) -> NpmResolutionVerifier {
    let age_check_active = opts.minimum_release_age.is_some_and(|minutes| minutes > 0);

    let cutoff = if age_check_active {
        let minutes = opts.minimum_release_age.unwrap_or(0);
        let now = opts.now.unwrap_or_else(Utc::now);
        // Checked arithmetic at every step so an absurd `u64` value
        // can't wrap on cast, overflow inside `chrono::Duration`, or
        // underflow the wall-clock subtraction. None means the cutoff
        // couldn't be represented; the verifier degrades to "no age
        // check" rather than fabricating a cutoff pointing the wrong
        // direction.
        i64::try_from(minutes)
            .ok()
            .and_then(chrono::Duration::try_minutes)
            .and_then(|duration| now.checked_sub_signed(duration))
    } else {
        None
    };

    let named_registry_prefixes = named_registry_tarball_prefixes(&opts.registries_by_prefix);
    let registries_by_prefix = opts.registries_by_prefix.clone();

    let sorted_min_age_excludes = sorted_unique(&opts.minimum_release_age_exclude_patterns);
    let sorted_trust_excludes = sorted_unique(&opts.trust_policy_exclude_patterns);
    let named_registries_routing = named_registries_routing_digest(&registries_by_prefix);

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
        registries_by_prefix,
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
        // The tarball-URL binding is unconditional today; a cached run
        // that didn't record it (e.g. written before this rule existed)
        // can't be trusted to have enforced it, so force a re-check.
        if cached_policy.get("tarballUrlBinding").and_then(JsonValue::as_bool) != Some(true) {
            return false;
        }
        if cached_policy.get("revisionHistoryBinding").and_then(JsonValue::as_bool) != Some(true) {
            return false;
        }

        // The missing-integrity check is also unconditional; a cached run
        // without the flag cannot prove it rejected unverifiable tarballs.
        if cached_policy.get("integrityRequired").and_then(JsonValue::as_bool) != Some(true) {
            return false;
        }

        if cached_policy.get("namedRegistriesRouting")
            != self.policy_snapshot.get("namedRegistriesRouting")
        {
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

        let past_min_age_excludes = cached_policy
            .get("minimumReleaseAgeExclude")
            .and_then(JsonValue::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if past_min_age_excludes != self.sorted_min_age_excludes {
            return false;
        }

        let past_trust_policy = cached_policy.get("trustPolicy").and_then(JsonValue::as_str);
        let today_trust_policy = self.trust_policy_wire_str();
        if past_trust_policy != today_trust_policy {
            return false;
        }

        let past_trust_excludes = cached_policy
            .get("trustPolicyExclude")
            .and_then(JsonValue::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
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

        if node_semver::Version::parse(ctx.version).is_err() {
            return ResolutionVerification::Ok;
        }

        // Registry-qualified entries name their registry in the dep path,
        // so routing does not depend on a recorded tarball URL (canonical
        // URLs are omitted from the lockfile in the 12.0 format). Fail
        // closed on an unknown alias: none of the metadata-backed checks
        // below could vouch for the entry without its registry URL.
        let named_registry = match ctx.registry_name {
            Some(registry_name) => match self.registries_by_prefix.get(registry_name) {
                Some(url) => Some(url.clone()),
                None => {
                    return ResolutionVerification::Err {
                        code: MISSING_NAMED_REGISTRY_VIOLATION_CODE,
                        reason: format!(
                            "has registry prefix '{registry_name}:', which is not declared by the registries setting",
                        ),
                    };
                }
            },
            None => None,
        };

        let age_applies = self.age_check_active()
            && !is_excluded(self.minimum_release_age_exclude.as_ref(), ctx.name, ctx.version);
        let trust_applies = self.trust_check_active()
            && !is_excluded(self.trust_policy_exclude.as_ref(), ctx.name, ctx.version);
        if tarball_url.is_none() && !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }
        let registry = named_registry.unwrap_or_else(|| self.pick_registry(ctx.name, tarball_url));

        // A registry entry that pins an explicit tarball URL must point at
        // the artifact the registry's own metadata lists. Otherwise a trusted
        // name@version could front bytes from an attacker-chosen URL (with a
        // matching integrity for those bytes). This binding is unconditional —
        // it does not depend on the minimum-release-age / trust policies and
        // isn't narrowed by their exclude lists, since it guards integrity
        // rather than maturity/trust.
        if (tarball_url.is_some()
            || (lockfile_revision(resolution).is_some() && (age_applies || trust_applies)))
            && let Some(violation) = self
                .run_registry_artifact_check(
                    &registry,
                    ctx.name,
                    ctx.version,
                    resolution,
                    tarball_url,
                )
                .await
        {
            return violation;
        }

        if !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }

        if age_applies
            && let Some(violation) =
                self.run_age_check(&registry, ctx.name, ctx.version, ctx.registry_name).await
        {
            return violation;
        }

        if trust_applies
            && let Some(violation) = self.run_trust_check(&registry, ctx.name, ctx.version).await
        {
            return violation;
        }

        ResolutionVerification::Ok
    }

    fn age_check_active(&self) -> bool {
        self.minimum_release_age_minutes.is_some_and(|minutes| minutes > 0)
    }

    fn trust_check_active(&self) -> bool {
        matches!(self.trust_policy, Some(TrustPolicy::NoDowngrade))
    }

    fn trust_policy_wire_str(&self) -> Option<&'static str> {
        match self.trust_policy {
            Some(TrustPolicy::NoDowngrade) => Some("no-downgrade"),
            Some(TrustPolicy::Off) | None => None,
        }
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

    /// Confirm the lockfile-pinned tarball URL is the artifact the
    /// registry's own metadata lists for this exact `name@version`.
    ///
    /// Fail-closed: the entry passes only when the registry metadata
    /// affirmatively lists this version with a matching tarball URL. If the
    /// metadata can't be fetched, doesn't list the version, or omits
    /// `dist.tarball`, the entry can't be confirmed and is rejected —
    /// otherwise a tampered lockfile could smuggle a malicious URL past the
    /// check by pointing it at a `name@version` the registry can't vouch for.
    async fn run_registry_artifact_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
        resolution: &LockfileResolution,
        lockfile_tarball: Option<&str>,
    ) -> Option<ResolutionVerification> {
        let artifact = match self.fetch_abbreviated_meta(registry, name).await {
            Ok(meta) => {
                if let Some(sink) = self.observed_dist_stats.as_ref()
                    && let Some(stats) =
                        meta.version_dist_stats.as_ref().and_then(|stats| stats.get(version))
                {
                    sink.insert((name.to_string(), version.to_string()), *stats);
                }
                meta.version_artifacts.and_then(|artifacts| artifacts.get(version).cloned())
            }
            Err(message) => {
                // Couldn't reach the registry to verify (auth/network/5xx).
                // Propagate the registry's own fetch error (already
                // credential-redacted) so the install aborts with it rather
                // than mislabeling a transport failure as a tampering-style URL
                // mismatch. Still fail-closed: the entry never reaches the
                // filesystem because the install never proceeds.
                return Some(ResolutionVerification::FetchFailed { message });
            }
        };
        let Some(artifact) = artifact else {
            if lockfile_tarball.is_none() && lockfile_revision(resolution).is_none() {
                return None;
            }
            return Some(ResolutionVerification::Err {
                code: if lockfile_tarball.is_some() {
                    TARBALL_URL_MISMATCH_VIOLATION_CODE
                } else {
                    TARBALL_REVISION_MISMATCH_VIOLATION_CODE
                },
                reason: "could not be verified against the registry's published metadata"
                    .to_string(),
            });
        };
        let revision_aware = lockfile_revision(resolution).is_some()
            || artifact.current.revision.is_some()
            || !artifact.revisions.is_empty();
        if !revision_aware {
            return match (lockfile_tarball, artifact.current.tarball) {
                (None, _) => None,
                (Some(lockfile), Some(registry)) if same_tarball_url(lockfile, &registry) => None,
                (Some(lockfile), Some(registry)) => Some(ResolutionVerification::Err {
                    code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
                    reason: format!(
                        "has a tarball URL ({lockfile}) that does not match the registry's published metadata ({registry})",
                    ),
                }),
                (Some(_), None) => Some(ResolutionVerification::Err {
                    code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
                    reason: "could not be verified against the registry's published metadata"
                        .to_string(),
                }),
            };
        }
        let current_revision = match artifact.current.revision.as_ref() {
            None => 0,
            Some(raw_revision) => match raw_revision
                .as_u64()
                .and_then(|revision| TarballRevision::try_from(revision).ok())
            {
                Some(revision) => revision.get(),
                None => {
                    return Some(ResolutionVerification::Err {
                        code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
                        reason: format!(
                            "registry metadata has an invalid current revision ({raw_revision})",
                        ),
                    });
                }
            },
        };
        if current_revision > 0 {
            let current_history: Vec<_> = artifact
                .revisions
                .iter()
                .filter(|candidate| {
                    candidate.revision.as_ref().and_then(JsonValue::as_u64)
                        == Some(current_revision)
                })
                .collect();
            if current_history.len() != 1
                || current_history[0].integrity != artifact.current.integrity
                || !matches!(
                    (
                        artifact.current.tarball.as_deref(),
                        artifact.current.integrity.as_ref(),
                    ),
                    (Some(tarball), Some(integrity))
                        if is_integrity_addressed_registry_tarball_url(
                            tarball, integrity, registry,
                        ),
                )
                || !matches!(
                    (
                        current_history[0].tarball.as_deref(),
                        artifact.current.tarball.as_deref(),
                    ),
                    (Some(history), Some(current)) if same_tarball_url(history, current),
                )
            {
                return Some(ResolutionVerification::Err {
                    code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
                    reason: format!(
                        "registry metadata revision {current_revision} does not have exactly one matching history entry",
                    ),
                });
            }
        }
        let requested = lockfile_revision(resolution).unwrap_or(0);
        let integrity = resolution.checkable_integrity().expect("checked before artifact binding");
        let current_matches = current_revision == requested;
        let historical: Vec<_> = artifact
            .revisions
            .iter()
            .filter(|candidate| {
                candidate.revision.as_ref().and_then(JsonValue::as_u64) == Some(requested)
            })
            .collect();
        if historical.len() > 1 {
            return Some(ResolutionVerification::Err {
                code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
                reason: format!(
                    "revision {requested} is advertised more than once in the registry's history",
                ),
            });
        }
        let historical = historical.first().copied();
        let selected = if current_matches { Some(&artifact.current) } else { historical };
        let Some(selected) = selected.filter(|selected| {
            selected.integrity.as_ref() == Some(integrity)
                && (!current_matches
                    || historical
                        .is_none_or(|historical| historical.integrity.as_ref() == Some(integrity)))
        }) else {
            return Some(ResolutionVerification::Err {
                code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
                reason: format!(
                    "has revision {requested} with an integrity that does not match the registry's current or historical metadata",
                ),
            });
        };
        if (requested > 0 || !current_matches)
            && !selected.tarball.as_deref().is_some_and(|tarball| {
                is_integrity_addressed_registry_tarball_url(tarball, integrity, registry)
            })
        {
            return Some(ResolutionVerification::Err {
                code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
                reason: format!(
                    "has revision {requested} that is not addressed by its complete sha512 integrity",
                ),
            });
        }
        match (lockfile_tarball, selected.tarball.as_deref()) {
            (Some(lockfile), Some(registry)) if same_tarball_url(lockfile, registry) => None,
            (Some(lockfile), Some(registry)) => Some(ResolutionVerification::Err {
                code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
                reason: format!(
                    "has a tarball URL ({lockfile}) that does not match the registry's published metadata ({registry})",
                ),
            }),
            (Some(_), None) => Some(ResolutionVerification::Err {
                code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
                reason: "could not be verified against the registry's published metadata"
                    .to_string(),
            }),
            (None, _) => None,
        }
    }

    async fn run_age_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
        registry_name: Option<&str>,
    ) -> Option<ResolutionVerification> {
        let cutoff = self.cutoff.expect("cutoff is Some when age check is active");
        // Cheapest layer: for an entry whose canonical tarball this
        // install fetches (existence fail-closed by the fetch itself),
        // a package-level `Last-Modified` older than the cutoff bounds
        // every version's publish time — no metadata body needed. The
        // evidence cell is consulted before the probe so installs that
        // never fill it (no materialization, or a resolver alongside)
        // send no extra request.
        let planned_key =
            (name.to_string(), version.to_string(), registry_name.map(str::to_string));
        if self
            .planned_canonical_fetches
            .as_ref()
            .and_then(|cell| cell.get())
            .is_some_and(|planned| planned.contains(&planned_key))
            && self.head_modified_is_before(registry, name, cutoff).await
        {
            return None;
        }
        let published = match self.fetch_published_at(registry, name, version).await {
            Ok(value) => value,
            // A transport failure propagates the registry's own fetch error so
            // the install aborts with it; a successful fetch that merely lacks a
            // timestamp is handled below.
            Err(message) => return Some(ResolutionVerification::FetchFailed { message }),
        };
        let Some(published) = published else {
            // No source surfaced a publish timestamp. What
            // `minimumReleaseAgeIgnoreMissingTime` opts out of is a
            // registry that cannot date its releases, so the skip is
            // granted only when the packument carries no usable `time`
            // map at all — the same shape the picker warns and skips on,
            // so the verifier can't be stricter than fresh resolution. A
            // packument that does date every version it lists is instead
            // telling us this pin is not one of them
            // (`Package::drop_incomplete_publish_times` leaves no partial
            // maps for that to be ambiguous), and an unpublished
            // or never-published pin must fail closed however the flag is
            // set.
            if self.ignore_missing_time_field
                // Already awaited by the lookup above, so this is a cache hit.
                && matches!(self.fetch_full_meta_time(registry, name).await, Ok(None))
            {
                warn_missing_time_once(&name.to_string(), SkippedTimeCheck::MinimumReleaseAge);
                return None;
            }
            return Some(ResolutionVerification::Err {
                code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                reason: uncheckable(
                    "minimumReleaseAge",
                    "version not present in registry manifest",
                ),
            });
        };
        let Some(parsed) = parse_packument_timestamp(&published) else {
            return Some(ResolutionVerification::Err {
                code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                reason: "publish timestamp is not a valid date".to_string(),
            });
        };
        if parsed > cutoff {
            return Some(ResolutionVerification::Err {
                code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                reason: format!(
                    "was published at {published}, within the minimumReleaseAge cutoff ({cutoff})",
                    cutoff = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                ),
            });
        }
        None
    }

    /// Run the resolver-time `failIfTrustDowngraded` check against the
    /// pinned lockfile version.
    async fn run_trust_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<ResolutionVerification> {
        let meta = match self.fetch_full_meta_for_trust(registry, name).await {
            Ok(meta) => meta,
            // A transport failure propagates the registry's own fetch error so
            // the install aborts with it rather than folding it into a policy
            // violation.
            Err(message) => return Some(ResolutionVerification::FetchFailed { message }),
        };
        let trust_opts = TrustCheckOptions {
            trust_policy_exclude: self.trust_policy_exclude.as_ref(),
            trust_policy_ignore_after_minutes: self.trust_policy_ignore_after,
            now: self.now,
            ignore_missing_time_field: self.ignore_missing_time_field,
        };
        match fail_if_trust_downgraded(&meta, version, &trust_opts) {
            Ok(()) => None,
            Err(err) => Some(ResolutionVerification::Err {
                code: TRUST_DOWNGRADE_VIOLATION_CODE,
                reason: format_trust_violation(err),
            }),
        }
    }

    /// Whether the package-level `Last-Modified` a packument `HEAD`
    /// reports is older than `cutoff` by more than the header's own
    /// one-second resolution. HTTP dates carry whole seconds, and a
    /// registry that truncates a fractional `time.modified` understates
    /// it by up to 999ms — the guard band keeps the comparison an upper
    /// bound regardless of how the server rounds. `false` when the
    /// probe fails, the header is missing or unparsable, or the
    /// registry is unreachable — the caller falls through to the
    /// metadata-backed layers, so the probe can only ever *save* a
    /// body, never widen what passes. Trust-wise the header is the same
    /// statement as the packument body's `time.modified`, served by the
    /// same registry. One probe per `(registry, name)`, queued in the
    /// background network class.
    async fn head_modified_is_before(
        &self,
        registry: &str,
        name: &PkgName,
        cutoff: DateTime<Utc>,
    ) -> bool {
        if self.offline {
            return false;
        }
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.head_modified.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        let modified = cell
            .get_or_init(|| async {
                let url = to_registry_url(registry, &name.to_string());
                let guard = self
                    .http_client
                    .acquire_for_url_with_priority(&url, pnpm_network::BACKGROUND)
                    .await;
                let mut request = guard.head(&url);
                if let Some(value) =
                    self.auth_headers.for_url_with_package(&url, Some(&name.to_string()))
                {
                    request = request.header("authorization", value);
                }
                let response = match request.send().await {
                    Ok(response) if response.status().is_success() => response,
                    _ => return None,
                };
                response
                    .headers()
                    .get("last-modified")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string)
            })
            .await;
        modified
            .as_deref()
            .and_then(|value| httpdate::parse_http_date(value).ok())
            .map(DateTime::<Utc>::from)
            .is_some_and(|parsed| parsed + chrono::Duration::seconds(1) <= cutoff)
    }

    /// Per-`(registry, name, version)` lookup with a layered fallback.
    async fn fetch_published_at(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        let key = version_key(registry, &name.to_string(), version);
        let cell = {
            let mut cache = self.lookup_context.published_at.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        cell.get_or_init(|| async { self.resolve_published_at(registry, name, version).await })
            .await
            .clone()
    }

    /// Layered publish-timestamp lookup:
    ///
    /// 1. **Abbreviated-`modified` shortcut.** Abbreviated metadata is
    ///    a small per-name document the resolver typically already
    ///    holds. Its package-level `modified` is an upper bound on
    ///    every version's publish time — if it's older than the
    ///    cutoff *and* the pinned version is still listed in
    ///    `versions`, the gate is satisfied without per-version
    ///    timestamps. Costs at most one abbreviated GET per name on
    ///    cold cache; the full-meta fallback below is hundreds of KB
    ///    bigger per package.
    /// 2. **Abbreviated per-version `time`** — only with
    ///    `registrySupportsTimeField`. Registries that serve the `time`
    ///    map in abbreviated metadata (Verdaccio 5.15.1+, pnpr) already
    ///    gave us the exact per-version timestamp in the document step 1
    ///    fetched, so a recent `modified` does not have to escalate to
    ///    the per-version fallbacks below.
    /// 3. **On-disk full-meta mirror.** If a previous verification
    ///    populated `<cache_dir>/v11/metadata-full/.../<name>.jsonl`,
    ///    take the per-version timestamp from there with no network.
    /// 4. **Npm attestation endpoint.** Small payload, just this
    ///    version's Sigstore-anchored timestamp. Wins on cold cache
    ///    when the package was published with provenance.
    /// 5. **Full metadata fetch.** Last resort.
    async fn resolve_published_at(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        if let Some(value) = self.try_abbreviated_modified_shortcut(registry, name, version).await {
            return Ok(Some(value));
        }
        if self.registry_supports_time_field
            && let Some(value) = self.abbreviated_version_time(registry, name, version).await
        {
            return Ok(Some(value));
        }
        if let Some(map) = self.read_local_meta_time(registry, name).await
            && let Some(value) = map.get(version)
        {
            return Ok(Some(value.clone()));
        }
        if let Some(value) = self.fetch_attestation_time(registry, name, version).await? {
            return Ok(Some(value));
        }
        let full_meta_time = self.fetch_full_meta_time(registry, name).await?;
        Ok(full_meta_time.and_then(|map| map.get(version).cloned()))
    }

    /// Returns the package's `modified` timestamp *iff* it proves the
    /// gate would pass — i.e. it's strictly older than the policy
    /// cutoff *and* the pinned version is still listed in the
    /// package's current versions map.
    ///
    /// The version check is the fail-closed contract: an unpublished
    /// or never-published pin must not slip through on a stale
    /// package-level `modified` timestamp.
    async fn try_abbreviated_modified_shortcut(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<String> {
        let cutoff = self.cutoff.expect("cutoff is Some when age check is active");
        // A fetch failure here is fine: ignore the error and fall back to
        // per-version lookups, the same as a successful-but-uninformative
        // metadata response.
        let Ok(meta) = self.fetch_abbreviated_meta(registry, name).await else {
            return None;
        };
        let modified = meta.modified?;
        let parsed = parse_packument_timestamp(&modified)?;
        if parsed >= cutoff {
            return None;
        }
        if !meta.version_artifacts.as_ref().is_some_and(|map| map.contains_key(version)) {
            return None;
        }
        Some(modified)
    }

    /// The pinned version's publish timestamp from the abbreviated
    /// document's `time` map. Reuses the same cached projection as the
    /// `modified` shortcut, so with `registrySupportsTimeField` the
    /// whole lookup stays within the one document the verifier already
    /// holds. A fetch failure or an absent entry falls through to the
    /// per-version fallbacks, exactly like the shortcut above.
    async fn abbreviated_version_time(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<String> {
        let meta = self.fetch_abbreviated_meta(registry, name).await.ok()?;
        meta.version_time.as_ref()?.get(version).cloned()
    }

    /// Per-`(registry, name)` abbreviated-meta lookup. The result is
    /// projected down to `(modified, versionNames)` and cached so
    /// repeat verifications of the same package within an install
    /// cost at most one disk/network round-trip.
    ///
    /// Three fetch layers:
    /// 1. The shared [`PackageMetaCache`] populated by the resolver
    ///    during its own `pick_package` pass. Either form (full or
    ///    abbreviated) carries the two fields the projection needs,
    ///    so the verifier prefers `name:full` when present and falls
    ///    back to the bare `name` key.
    /// 2. The on-disk + network cached fetcher
    ///    ([`fetch_full_metadata_cached()`] with `full_metadata: false`)
    ///    when no shared entry is available.
    /// 3. A failure (decode / network / cache-write IO) caches a
    ///    credential-safe `Err(reason)` so subsequent calls reuse the
    ///    same verdict without retrying. The tarball-URL check surfaces
    ///    this error; the age shortcut ignores it and falls through to
    ///    the next layer of [`Self::resolve_published_at`].
    async fn fetch_abbreviated_meta(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<crate::lookup_context::AbbreviatedMetaProjection, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.abbreviated_meta.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        let value = cell
            .get_or_init(|| async {
                if let Some(shared) = self.read_shared_meta(registry, name) {
                    return Ok(project_abbreviated_meta(
                        &shared,
                        self.registry_supports_time_field,
                    ));
                }
                let opts = FetchFullMetadataCachedOptions {
                    registry,
                    http_client: &self.http_client,
                    auth_headers: &self.auth_headers,
                    cache_dir: self.cache_dir.as_deref(),
                    full_metadata: false,
                    filter_metadata: false,
                    offline: self.offline,
                    priority: pnpm_network::BACKGROUND,
                    retry_opts: self.retry_opts,
                };
                // Carry a fetch failure (auth/network/5xx) as the `Err` value
                // instead of collapsing it to a missing projection: the
                // tarball-URL check needs to tell a transport failure apart
                // from a version genuinely absent from the metadata, otherwise
                // it reports a 403 as a tampering-style mismatch.
                match fetch_full_metadata_cached(&name.to_string(), &opts).await {
                    Ok(meta) => {
                        Ok(project_abbreviated_meta(&meta, self.registry_supports_time_field))
                    }
                    Err(error) => Err(render_fetch_metadata_error(&error)),
                }
            })
            .await;
        value.clone()
    }

    /// Try the resolver's shared [`PackageMetaCache`] for a packument
    /// the abbreviated projection can derive from. The resolver keys
    /// entries by registry plus name (see `metadata_cache_key`), with a
    /// `:full` / `:full:filtered` suffix depending on its own metadata
    /// mode, so try full, filtered full, then abbreviated — a full form
    /// is a strict superset of the abbreviated shape, and `clear_meta`
    /// keeps every field the projection reads. Private-scoped entries
    /// carry a descriptor prefix the verifier can't reproduce; those
    /// simply miss and fall through to the verifier's own fetch chain.
    fn read_shared_meta(&self, registry: &str, name: &PkgName) -> Option<Arc<Package>> {
        let cache = self.meta_cache.as_ref()?;
        let name_str = name.to_string();
        let key = package_key(registry, &name_str);
        cache
            .get(&format!("{key}:full"))
            .or_else(|| cache.get(&format!("{key}:full:filtered")))
            .or_else(|| cache.get(&key))
            .map(|cached| cached.meta)
            .filter(|meta| meta.name == name_str)
    }

    /// Per-`(registry, name)` on-disk mirror read of the full
    /// packument's per-version `time` map. Returns `None` when no
    /// mirror exists yet, no `cache_dir` was supplied, or the mirror
    /// has no `time` payload — the caller then falls through to the
    /// next layer of [`Self::resolve_published_at`].
    async fn read_local_meta_time(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Option<Arc<PublishedAtTimeMap>> {
        let cache_dir = self.cache_dir.as_deref()?;
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.local_meta.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        // The verifier reads the *same* scoped mirror a resolve would
        // populate. A private packument lives under its descriptor
        // namespace, so a caller who can't reproduce the descriptor can't
        // read another caller's private `time` map through the trust-check
        // path.
        let name_string = name.to_string();
        let url = crate::registry_url::to_registry_url(registry, &name_string);
        let scope = self.auth_headers.metadata_scope(&url, Some(&name_string));
        cell.get_or_init(|| async {
            let meta_dir = crate::mirror::scoped_meta_dir(&scope, crate::mirror::FULL_META_DIR);
            let mirror_path =
                crate::mirror::get_pkg_mirror_path(cache_dir, &meta_dir, registry, &name_string)
                    .ok();
            crate::mirror::load_meta_async(mirror_path.as_deref()).await.and_then(|pkg| {
                pkg.time.as_ref().map(|raw| {
                    raw.iter()
                        .filter_map(|(version, value)| {
                            value.as_str().map(|ts| (version.clone(), ts.to_string()))
                        })
                        .collect::<PublishedAtTimeMap>()
                        .pipe(Arc::new)
                })
            })
        })
        .await
        .clone()
    }

    async fn fetch_attestation_time(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        if self.offline {
            return Ok(None);
        }
        let opts = FetchAttestationOptions {
            registry,
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
        };
        fetch_attestation_published_at(&name.to_string(), version, &opts)
            .await
            .map_err(|err| redact_url_credentials(&err.to_string()))
    }

    async fn fetch_full_meta_time(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Option<Arc<PublishedAtTimeMap>>, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.full_meta.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        cell.get_or_init(|| async {
            let pkg = self.fetch_full_meta(registry, name).await?;
            let time_map = pkg.time.as_ref().map(|raw| {
                raw.iter()
                    .filter_map(|(version, value)| {
                        value.as_str().map(|ts| (version.clone(), ts.to_string()))
                    })
                    .collect::<PublishedAtTimeMap>()
                    .pipe(Arc::new)
            });
            Ok(time_map)
        })
        .await
        .clone()
    }

    async fn fetch_full_meta_for_trust(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Arc<Package>, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.full_meta_for_trust.lock().await;
            Arc::clone(cache.entry(key.clone()).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        cell.get_or_init(|| async {
            // Fast path: if the resolver already pulled the full packument
            // during the same install (`{registry}\x00{name}:full` or
            // `...:full:filtered` key in the shared metaCache, populated
            // when `pick_package` upgrades for `minimumReleaseAge`),
            // reuse it. The filtered form is accepted: `clear_meta`
            // keeps `time`, per-version `_npmUser`, and `dist`, which is
            // everything `fail_if_trust_downgraded` reads. Abbreviated
            // entries are rejected — they lack per-version `time` and
            // trust evidence.
            let shared = self.meta_cache.as_ref().and_then(|cache| {
                cache
                    .get(&format!("{key}:full"))
                    .or_else(|| cache.get(&format!("{key}:full:filtered")))
            });
            if let Some(cached) = shared {
                return Ok(Arc::new(project_trust_meta(cached.meta.as_ref())));
            }
            // Project the packument to just the fields `fail_if_trust_downgraded`
            // reads before stashing in the cache. The full document — dependency
            // graphs, dist-tags, scripts, READMEs for every version — would
            // otherwise stay resident in this map for the entire install, which
            // on multi-thousand-entry workspaces OOMs CI runners with a 2GB heap
            // cap (see [#11860]).
            //
            // [#11860]: <https://github.com/pnpm/pnpm/issues/11860>
            self.fetch_full_meta(registry, name)
                .await
                .map(|meta| project_trust_meta(&meta))
                .map(Arc::new)
        })
        .await
        .clone()
    }

    async fn fetch_full_meta(&self, registry: &str, name: &PkgName) -> Result<Package, String> {
        let opts = FetchFullMetadataCachedOptions {
            registry,
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            cache_dir: self.cache_dir.as_deref(),
            // The verifier reads `time` and trust evidence per-version,
            // both of which the abbreviated form drops. Always full.
            full_metadata: true,
            filter_metadata: false,
            offline: self.offline,
            priority: pnpm_network::BACKGROUND,
            retry_opts: self.retry_opts,
        };
        fetch_full_metadata_cached(&name.to_string(), &opts)
            .await
            .map_err(|error| render_fetch_metadata_error(&error))
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

/// Tarball URL recorded on an npm-registry resolution. The verifier
/// uses it for prefix-matching against named registries; absence
/// alone doesn't disqualify the entry (Registry / Tarball variants
/// without a URL still go through scope routing).
fn npm_registry_tarball(resolution: &LockfileResolution) -> Option<Option<&str>> {
    match resolution {
        // Registry-resolved entries carry only `integrity`; the tarball
        // URL is reconstructed at fetch time. They still qualify for
        // verification.
        LockfileResolution::Registry(_) => Some(None),
        LockfileResolution::Tarball(t) => {
            // Git-hosted tarballs (codeload / gitlab / bitbucket) are
            // not subject to the release-age policy and don't have a
            // packument lookup; skip them. The exemption is decided from
            // the URL alone, never from the recorded `gitHosted` flag: the
            // flag is lockfile input, so a tampered entry could otherwise
            // set it on an attacker-hosted URL and buy itself the same
            // exemption.
            if is_git_hosted_tarball_url(&t.tarball) {
                return None;
            }
            if let Ok(parsed) = reqwest::Url::parse(&t.tarball) {
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    return None;
                }
            }
            Some(Some(t.tarball.as_str()))
        }
        // Custom resolutions have no packument lookup — the pnpmfile
        // custom resolver, not the npm registry, is their authority.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => None,
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

fn sorted_unique(values: &[String]) -> Vec<String> {
    let mut deduped: Vec<String> = values.to_vec();
    deduped.sort();
    deduped.dedup();
    deduped
}

fn named_registries_routing_digest(registries_by_prefix: &HashMap<String, String>) -> String {
    let sorted: BTreeMap<&str, &str> = registries_by_prefix
        .iter()
        .map(|(alias, registry)| (alias.as_str(), registry.as_str()))
        .collect();
    let encoded = serde_json::to_vec(&sorted).expect("named registry mappings are serializable");
    format!("{:x}", Sha256::digest(encoded))
}

/// Argument bundle for [`build_policy_snapshot`].
#[derive(Clone, Copy)]
struct BuildPolicySnapshot<'a> {
    minimum_release_age: u64,
    sorted_min_age_excludes: &'a [String],
    ignore_missing_time_field: bool,
    trust_policy: Option<TrustPolicy>,
    sorted_trust_excludes: &'a [String],
    trust_policy_ignore_after: Option<u64>,
    named_registries_routing: &'a str,
}

fn build_policy_snapshot(opts: &BuildPolicySnapshot<'_>) -> serde_json::Map<String, JsonValue> {
    let &BuildPolicySnapshot {
        minimum_release_age,
        sorted_min_age_excludes,
        ignore_missing_time_field,
        trust_policy,
        sorted_trust_excludes,
        trust_policy_ignore_after,
        named_registries_routing,
    } = opts;
    let mut map = serde_json::Map::new();
    // Marks runs that enforced the (unconditional) tarball-URL binding so
    // `can_trust_past_check` rejects pre-rule cache records and re-verifies.
    map.insert("tarballUrlBinding".to_string(), JsonValue::Bool(true));
    map.insert("revisionHistoryBinding".to_string(), JsonValue::Bool(true));
    // Same cache identity rule for the missing-integrity structural check.
    map.insert("integrityRequired".to_string(), JsonValue::Bool(true));
    map.insert(
        "namedRegistriesRouting".to_string(),
        JsonValue::String(named_registries_routing.to_string()),
    );
    map.insert("minimumReleaseAge".to_string(), JsonValue::from(minimum_release_age));
    map.insert(
        "minimumReleaseAgeExclude".to_string(),
        JsonValue::Array(
            sorted_min_age_excludes.iter().map(|spec| JsonValue::String(spec.clone())).collect(),
        ),
    );
    map.insert(
        "trustPolicy".to_string(),
        match trust_policy {
            Some(TrustPolicy::NoDowngrade) => JsonValue::String("no-downgrade".to_string()),
            Some(TrustPolicy::Off) | None => JsonValue::Null,
        },
    );
    map.insert(
        "trustPolicyExclude".to_string(),
        JsonValue::Array(
            sorted_trust_excludes.iter().map(|spec| JsonValue::String(spec.clone())).collect(),
        ),
    );
    map.insert(
        "trustPolicyIgnoreAfter".to_string(),
        match trust_policy_ignore_after {
            Some(value) => JsonValue::from(value),
            None => JsonValue::Null,
        },
    );
    map.insert(
        "minimumReleaseAgeIgnoreMissingTime".to_string(),
        JsonValue::Bool(ignore_missing_time_field),
    );
    map
}

/// Build a [`Package`] that retains only the fields
/// [`fail_if_trust_downgraded`] reads: the package name, the per-version
/// `time` map, and per-version trust evidence (`_npmUser.approver`,
/// `_npmUser.trustedPublisher`, and `dist.attestations.provenance`).
/// Drops everything else — dependency
/// graphs, scripts, READMEs — so the per-install trust-meta cache stays
/// bounded by the trust-evidence footprint, not the full packument size.
///
/// [`fail_if_trust_downgraded`]: crate::trust_checks::fail_if_trust_downgraded
fn project_trust_meta(meta: &Package) -> Package {
    // Borrowed `meta` so the shared-cache fast path (which only holds
    // `Arc<Package>`) doesn't pay for a full deep-clone of the
    // packument it's about to discard. Only the fields downstream
    // reads are cloned out; the bulk of the document (per-version
    // dependency maps, scripts, README) drops on the original.
    let versions = meta
        .versions
        .iter()
        .map(|(version, manifest)| (version.clone(), project_trust_package_version(&manifest)))
        .collect();
    Package {
        name: meta.name.clone(),
        dist_tags: std::collections::HashMap::new(),
        versions,
        time: meta.time.clone(),
        modified: meta.modified.clone(),
        etag: meta.etag.clone(),
        // `homepage` is only read by `outdated --long`, never by trust
        // verification, so it is dropped here to keep the trust-meta cache
        // bounded by the trust-evidence footprint (see the fn doc).
        homepage: None,
        mutex: std::sync::Arc::new(std::sync::Mutex::new(0)),
        derived: DerivedPackuments::default(),
    }
}

fn project_trust_package_version(version: &PackageVersion) -> PackageVersion {
    let attestations =
        version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).map(|prov| {
            pnpm_registry::AttestationsDist { provenance: Some(prov.clone()), url: None }
        });
    // `get_trust_evidence` only reads `npm_user.approver` (presence) and
    // `npm_user.trusted_publisher`; drop the maintainer `name` / `email`
    // PII — including the approver's — so the projected cache entry
    // doesn't hold per-version publisher metadata that downstream
    // doesn't need.
    let approver = version.npm_user.as_ref().and_then(|user| user.approver.as_ref());
    let trusted_publisher =
        version.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref());
    let npm_user = (approver.is_some() || trusted_publisher.is_some()).then(|| NpmUser {
        name: None,
        email: None,
        approver: approver.map(|_| Approver { name: None, email: None }),
        trusted_publisher: trusted_publisher.cloned(),
    });
    PackageVersion {
        // `fail_if_trust_downgraded` keys off the outer `meta.versions`
        // map and the version-level npm_user / attestations fields. The
        // per-version `name`, `version`, and `dist` non-attestation fields
        // are never read, so empty placeholders are fine — clone of the
        // parsed semver keeps the typed shape valid without paying for
        // the registry packument's dependency graph.
        name: String::new(),
        version: version.version.clone(),
        dist: PackageDistribution {
            integrity: None,
            shasum: None,
            tarball: String::new(),
            revision: None,
            revisions: None,
            file_count: None,
            unpacked_size: None,
            attestations,
        },
        dependencies: None,
        dev_dependencies: None,
        peer_dependencies: None,
        optional_dependencies: None,
        peer_dependencies_meta: None,
        npm_user,
        deprecated: None,
        other: HashMap::new(),
    }
}

/// Pull the `(modified, versionTarballs)` projection the verifier
/// needs out of a packument document. Works against either the
/// abbreviated or the full form — both carry `modified` and a
/// `versions` map with per-version `dist.tarball`.
fn project_abbreviated_meta(
    meta: &Package,
    include_time: bool,
) -> crate::lookup_context::AbbreviatedMetaProjection {
    let version_artifacts = meta
        .versions
        .iter()
        .map(|(version, manifest)| {
            let revisions = manifest
                .dist
                .revisions
                .as_ref()
                .and_then(JsonValue::as_array)
                .into_iter()
                .flatten()
                .map(|revision| crate::lookup_context::RegistryArtifact {
                    revision: revision.get("revision").cloned(),
                    integrity: revision
                        .get("integrity")
                        .and_then(JsonValue::as_str)
                        .and_then(|integrity| integrity.parse().ok()),
                    tarball: revision
                        .get("tarball")
                        .and_then(JsonValue::as_str)
                        .map(str::to_string),
                })
                .collect();
            (
                version.clone(),
                crate::lookup_context::RegistryArtifactHistory {
                    current: crate::lookup_context::RegistryArtifact {
                        revision: manifest.dist.revision.clone(),
                        integrity: manifest.dist.integrity.clone(),
                        tarball: Some(manifest.dist.tarball.clone()),
                    },
                    revisions,
                },
            )
        })
        .collect();
    let version_dist_stats = meta
        .versions
        .iter()
        .filter_map(|(version, manifest)| {
            let stats = DistStats {
                unpacked_size: manifest.dist.unpacked_size,
                file_count: manifest.dist.file_count,
            };
            (stats.unpacked_size.is_some() || stats.file_count.is_some())
                .then(|| (version.clone(), stats))
        })
        .collect();
    // `time` also carries package-level `created`/`modified` keys; keeping
    // them is harmless (lookups are by exact version) and cheaper than
    // filtering against the versions map.
    let version_time = include_time.then(|| {
        meta.time
            .iter()
            .flatten()
            .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
            .collect()
    });
    crate::lookup_context::AbbreviatedMetaProjection {
        modified: meta.modified.clone(),
        version_artifacts: Some(version_artifacts),
        version_dist_stats: Some(version_dist_stats),
        version_time,
    }
}

fn same_tarball_url(left: &str, right: &str) -> bool {
    canonical_tarball_url(left) == canonical_tarball_url(right)
}

fn lockfile_revision(resolution: &LockfileResolution) -> Option<u64> {
    match resolution {
        LockfileResolution::Registry(registry) => registry.revision.map(TarballRevision::get),
        LockfileResolution::Tarball(tarball) => tarball.revision.map(TarballRevision::get),
        _ => None,
    }
}

/// Canonicalize a tarball URL: parse-and-reserialize to drop default
/// ports (`:443`/`:80`), decode the `%2f` scoped-name separator, then
/// ignore the scheme — so a benign http/https, default-port, or
/// encoding difference between the lockfile URL and the registry
/// metadata isn't read as tampering.
fn canonical_tarball_url(url: &str) -> String {
    let normalized = reqwest::Url::parse(url)
        .map_or_else(|_error| url.to_string(), |parsed| parsed.to_string())
        // `%2f` may survive re-serialization in either case; normalize both.
        .replace("%2F", "/")
        .replace("%2f", "/");
    match normalized.split_once("://") {
        Some((_scheme, rest)) => rest.to_string(),
        None => normalized,
    }
}

#[cfg(test)]
mod tests;
