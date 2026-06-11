//! Npm-side implementation of the [`ResolutionVerifier`] trait.
//!
//! Verbatim port of pnpm's
//! [`createNpmResolutionVerifier.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts).
//!
//! The factory takes the install-time policy (cutoff time, exclude
//! patterns, trust policy, named registries) and returns a verifier.
//! The verifier inspects each npm-registry-resolved lockfile entry: it
//! always binds the recorded tarball URL to the artifact the registry's
//! metadata lists (an anti-tamper check independent of any policy), and
//! additionally applies the `minimumReleaseAge` and/or
//! `trustPolicy='no-downgrade'` checks when those are configured.
//! Violations surface through [`ResolutionVerification::Err`].
//!
//! The publish-timestamp lookup walks a 4-layer fallback chain
//! (abbreviated-modified shortcut → local mirror → attestation
//! endpoint → full packument fetch); the trust check separately
//! reads the full packument to walk version history. Per-install
//! dedup of every network/disk call lives in
//! [`PublishedAtLookupContext`] so verifying many pinned versions of
//! the same package costs at most one fetch per layer.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use pacquet_config::{TrustPolicy, version_policy::PackageVersionPolicy};
use pacquet_lockfile::{LockfileResolution, PkgName};
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_registry::{Approver, NpmUser, Package, PackageDistribution, PackageVersion};
use pacquet_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture, parse_packument_timestamp,
};
use pipe_trait::Pipe;
use serde_json::Value as JsonValue;
use tokio::sync::OnceCell;

use crate::{
    FetchAttestationOptions, FetchFullMetadataCachedOptions, TrustCheckOptions, TrustViolation,
    fetch_attestation_published_at, fetch_full_metadata_cached,
    lookup_context::{PublishedAtLookupContext, PublishedAtTimeMap, package_key, version_key},
    named_registry::{build_named_registry_prefixes, pick_registry_for_package},
    pick_package::PackageMetaCache,
    trust_checks::fail_if_trust_downgraded,
    violation_codes::{
        MINIMUM_RELEASE_AGE_VIOLATION_CODE, TARBALL_URL_MISMATCH_VIOLATION_CODE,
        TRUST_DOWNGRADE_VIOLATION_CODE,
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

/// Options bundle for [`create_npm_resolution_verifier`]. Mirrors
/// upstream's
/// [`CreateNpmResolutionVerifierOptions`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L28-L84).
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
    /// `true` mirrors the resolver's
    /// `minimumReleaseAgeIgnoreMissingTime` opt-in: when the registry
    /// strips per-version `time`, the verifier passes the entry
    /// instead of failing closed. Default `false`.
    pub ignore_missing_time_field: bool,
    /// `'no-downgrade'` enables the trust check;
    /// [`TrustPolicy::Off`] disables it. Stored as an [`Option`] to
    /// mirror upstream's `trustPolicy?: 'no-downgrade'` — `None` and
    /// `Some(Off)` both disable the check, but they're snapshotted
    /// differently for `policy()` (matching upstream's
    /// `trustPolicy ?? null`).
    pub trust_policy: Option<TrustPolicy>,
    pub trust_policy_exclude: Option<PackageVersionPolicy>,
    pub trust_policy_exclude_patterns: Vec<String>,
    /// Maximum age (in minutes) before which the trust check still
    /// applies. `None` ("always check") mirrors upstream's
    /// `undefined`.
    pub trust_policy_ignore_after: Option<u64>,
    /// `default` + per-scope registry map. Keyed by `"default"` or
    /// `"@scope"`; mirrors pnpm's `Registries` shape.
    pub registries: HashMap<String, String>,
    /// User-defined named-registry aliases (e.g. `gh:` →
    /// `https://npm.pkg.github.com/`). Merged with
    /// [`crate::BUILTIN_NAMED_REGISTRIES`].
    pub named_registries: HashMap<String, String>,
    pub http_client: Arc<ThrottledClient>,
    pub auth_headers: Arc<AuthHeaders>,
    /// Root of pnpm's on-disk metadata mirror. When set, the verifier
    /// reads conditional headers from
    /// `<cache_dir>/v11/metadata-full/<registry>/<pkg>.jsonl` and
    /// writes 200 responses back; when `None`, every fetch is
    /// unconditional. Mirrors upstream's `cacheDir` option.
    pub cache_dir: Option<PathBuf>,
    /// Per-install [`PackageMetaCache`] shared with the npm resolver.
    /// When provided, the verifier reads a cached packument before
    /// fetching — a name the resolver already pulled during the same
    /// install yields the cached document instead of a fresh
    /// disk/network round-trip. Optional: frozen-install paths and
    /// unit tests don't have a resolver running alongside, in which
    /// case the verifier falls back to its own fetch chain.
    pub meta_cache: Option<Arc<dyn PackageMetaCache>>,
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
    http_client: Arc<ThrottledClient>,
    auth_headers: Arc<AuthHeaders>,
    cache_dir: Option<PathBuf>,
    meta_cache: Option<Arc<dyn PackageMetaCache>>,
    retry_opts: RetryOpts,
    now: Option<DateTime<Utc>>,
    policy_snapshot: serde_json::Map<String, JsonValue>,
    lookup_context: PublishedAtLookupContext,
    observed_dist_stats: Option<ObservedDistStats>,
}

impl std::fmt::Debug for NpmResolutionVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NpmResolutionVerifier")
            .field("minimum_release_age_minutes", &self.minimum_release_age_minutes)
            .field("cutoff", &self.cutoff)
            .field("ignore_missing_time_field", &self.ignore_missing_time_field)
            .field("trust_policy", &self.trust_policy)
            .field("trust_policy_ignore_after", &self.trust_policy_ignore_after)
            .field("sorted_min_age_excludes", &self.sorted_min_age_excludes)
            .field("sorted_trust_excludes", &self.sorted_trust_excludes)
            .field("policy_snapshot", &self.policy_snapshot)
            .finish_non_exhaustive()
    }
}

/// Builds the [`NpmResolutionVerifier`]. It always binds each entry's
/// recorded tarball URL to the artifact the registry's metadata lists (an
/// anti-tamper check independent of any policy), and additionally applies
/// the `minimum_release_age` / `trust_policy='no-downgrade'` checks when
/// those are configured.
///
/// Mirrors upstream's
/// [`createNpmResolutionVerifier`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L98-L253).
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

    let named_registry_prefixes = build_named_registry_prefixes(&opts.named_registries);

    let sorted_min_age_excludes = sorted_unique(&opts.minimum_release_age_exclude_patterns);
    let sorted_trust_excludes = sorted_unique(&opts.trust_policy_exclude_patterns);

    let policy_snapshot = build_policy_snapshot(
        opts.minimum_release_age.unwrap_or(0),
        &sorted_min_age_excludes,
        opts.trust_policy,
        &sorted_trust_excludes,
        opts.trust_policy_ignore_after,
    );

    NpmResolutionVerifier {
        minimum_release_age_minutes: opts.minimum_release_age,
        cutoff,
        minimum_release_age_exclude: opts.minimum_release_age_exclude,
        ignore_missing_time_field: opts.ignore_missing_time_field,
        trust_policy: opts.trust_policy,
        trust_policy_exclude: opts.trust_policy_exclude,
        trust_policy_ignore_after: opts.trust_policy_ignore_after,
        sorted_min_age_excludes,
        sorted_trust_excludes,
        registries: opts.registries,
        named_registry_prefixes,
        http_client: opts.http_client,
        auth_headers: opts.auth_headers,
        cache_dir: opts.cache_dir,
        meta_cache: opts.meta_cache,
        retry_opts: opts.retry_opts,
        now: opts.now,
        policy_snapshot,
        lookup_context: PublishedAtLookupContext::new(),
        observed_dist_stats: opts.observed_dist_stats,
    }
}

impl ResolutionVerifier for NpmResolutionVerifier {
    fn might_verify(&self, resolution: &LockfileResolution, ctx: VerifyCtx<'_>) -> bool {
        let Some(tarball_url) = npm_registry_tarball(resolution) else {
            return false;
        };
        if tarball_url.is_some() {
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
        // Non-semver versions identify URL tarballs, file: refs, git refs,
        // etc. Neither policy applies, and a registry lookup would 404.
        if node_semver::Version::parse(ctx.version).is_err() {
            return ResolutionVerification::Ok;
        }

        let age_applies = self.age_check_active()
            && !is_excluded(self.minimum_release_age_exclude.as_ref(), ctx.name, ctx.version);
        let trust_applies = self.trust_check_active()
            && !is_excluded(self.trust_policy_exclude.as_ref(), ctx.name, ctx.version);
        if tarball_url.is_none() && !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }

        let registry = self.pick_registry(ctx.name, tarball_url);

        // A registry entry that pins an explicit tarball URL must point at
        // the artifact the registry's own metadata lists. Otherwise a trusted
        // name@version could front bytes from an attacker-chosen URL (with a
        // matching integrity for those bytes). This binding is unconditional —
        // it does not depend on the minimum-release-age / trust policies and
        // isn't narrowed by their exclude lists, since it guards integrity
        // rather than maturity/trust.
        if let Some(url) = tarball_url
            && let Some(violation) =
                self.run_tarball_url_check(&registry, ctx.name, ctx.version, url).await
        {
            return violation;
        }

        if !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }

        if age_applies
            && let Some(violation) = self.run_age_check(&registry, ctx.name, ctx.version).await
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
    async fn run_tarball_url_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
        lockfile_tarball: &str,
    ) -> Option<ResolutionVerification> {
        let registry_tarball = match self.fetch_abbreviated_meta(registry, name).await {
            Ok(Some(meta)) => {
                if let Some(sink) = self.observed_dist_stats.as_ref()
                    && let Some(stats) =
                        meta.version_dist_stats.as_ref().and_then(|stats| stats.get(version))
                {
                    sink.insert((name.to_string(), version.to_string()), *stats);
                }
                meta.version_tarballs.and_then(|tarballs| tarballs.get(version).cloned())
            }
            Ok(None) | Err(_) => None,
        };
        match registry_tarball {
            Some(url) if same_tarball_url(lockfile_tarball, &url) => None,
            Some(url) => Some(ResolutionVerification::Err {
                code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
                reason: format!(
                    "has a tarball URL ({lockfile_tarball}) that does not match the registry's published metadata ({url})",
                ),
            }),
            None => Some(ResolutionVerification::Err {
                code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
                reason: "could not be verified against the registry's published metadata"
                    .to_string(),
            }),
        }
    }

    async fn run_age_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<ResolutionVerification> {
        let cutoff = self.cutoff.expect("cutoff is Some when age check is active");
        let published = match self.fetch_published_at(registry, name, version).await {
            Ok(value) => value,
            Err(reason) => {
                return Some(ResolutionVerification::Err {
                    code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                    reason: uncheckable("minimumReleaseAge", &reason),
                });
            }
        };
        let Some(published) = published else {
            // No source surfaced a publish timestamp; mirror the
            // resolver's `minimumReleaseAgeIgnoreMissingTime` opt-in.
            if self.ignore_missing_time_field {
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
    /// pinned lockfile version. Mirrors upstream's
    /// [`runTrustCheck`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L325-L359).
    ///
    /// No attestation fast-path: presence of provenance on the current
    /// version is not sufficient to clear a downgrade. The package may
    /// have shipped earlier versions under a `trustedPublisher` with
    /// provenance (the higher-rank evidence) and then dropped to plain
    /// provenance — `fail_if_trust_downgraded` correctly flags that.
    async fn run_trust_check(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Option<ResolutionVerification> {
        let meta = match self.fetch_full_meta_for_trust(registry, name).await {
            Ok(meta) => meta,
            Err(reason) => {
                return Some(ResolutionVerification::Err {
                    code: TRUST_DOWNGRADE_VIOLATION_CODE,
                    reason: uncheckable("trustPolicy", &reason),
                });
            }
        };
        let trust_opts = TrustCheckOptions {
            trust_policy_exclude: self.trust_policy_exclude.as_ref(),
            trust_policy_ignore_after_minutes: self.trust_policy_ignore_after,
            now: self.now,
        };
        match fail_if_trust_downgraded(&meta, version, &trust_opts) {
            Ok(()) => None,
            Err(err) => Some(ResolutionVerification::Err {
                code: TRUST_DOWNGRADE_VIOLATION_CODE,
                reason: format_trust_violation(err),
            }),
        }
    }

    /// Per-`(registry, name, version)` lookup with a layered fallback.
    /// Ports upstream's
    /// [`fetchPublishedAt`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L456-L491).
    ///
    /// Phase 4 stubs the abbreviated-shortcut and on-disk-mirror layers
    /// (return `None`); Phase 5 swaps the full-meta call behind the
    /// cached fetcher and ports the abbreviated/mirror layers.
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

    /// Layered publish-timestamp lookup. Ports upstream's
    /// [`resolvePublishedAt`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L471-L491):
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
    /// 2. **On-disk full-meta mirror.** If a previous verification
    ///    populated `<cache_dir>/v11/metadata-full/.../<name>.jsonl`,
    ///    take the per-version timestamp from there with no network.
    /// 3. **Npm attestation endpoint.** Small payload, just this
    ///    version's Sigstore-anchored timestamp. Wins on cold cache
    ///    when the package was published with provenance.
    /// 4. **Full metadata fetch.** Last resort.
    async fn resolve_published_at(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        if let Some(value) = self.try_abbreviated_modified_shortcut(registry, name, version).await?
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
    ///
    /// Mirrors upstream's
    /// [`tryAbbreviatedModifiedShortcut`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L606-L624).
    async fn try_abbreviated_modified_shortcut(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        let cutoff = self.cutoff.expect("cutoff is Some when age check is active");
        let Some(meta) = self.fetch_abbreviated_meta(registry, name).await? else {
            return Ok(None);
        };
        let Some(modified) = meta.modified else { return Ok(None) };
        let Some(parsed) = parse_packument_timestamp(&modified) else { return Ok(None) };
        if parsed >= cutoff {
            return Ok(None);
        }
        if !meta.version_tarballs.as_ref().is_some_and(|map| map.contains_key(version)) {
            return Ok(None);
        }
        Ok(Some(modified))
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
    /// 3. A failure (decode / network / cache-write IO) caches
    ///    `None` so subsequent calls fall through to the next layer
    ///    of [`Self::resolve_published_at`] without retrying.
    ///
    /// Mirrors upstream's
    /// [`fetchAbbreviatedMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L626-L653).
    async fn fetch_abbreviated_meta(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Option<crate::lookup_context::AbbreviatedMetaProjection>, String> {
        let key = package_key(registry, &name.to_string());
        let cell = {
            let mut cache = self.lookup_context.abbreviated_meta.lock().await;
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::new(OnceCell::new())))
        };
        let value = cell
            .get_or_init(|| async {
                if let Some(shared) = self.read_shared_meta(name) {
                    return Some(project_abbreviated_meta(&shared));
                }
                let opts = FetchFullMetadataCachedOptions {
                    registry,
                    http_client: &self.http_client,
                    auth_headers: &self.auth_headers,
                    cache_dir: self.cache_dir.as_deref(),
                    full_metadata: false,
                    retry_opts: self.retry_opts,
                };
                match fetch_full_metadata_cached(&name.to_string(), &opts).await {
                    Ok(meta) => Some(project_abbreviated_meta(&meta)),
                    Err(_) => None,
                }
            })
            .await;
        Ok(value.clone())
    }

    /// Try the resolver's shared [`PackageMetaCache`] for a packument
    /// the abbreviated projection can derive from. Prefer the
    /// `name:full` entry: it's a strict superset of the abbreviated
    /// shape, so a hit there subsumes the bare `name` entry.
    /// Mirrors upstream's
    /// [`readSharedMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L655-L668).
    fn read_shared_meta(&self, name: &PkgName) -> Option<Arc<Package>> {
        let cache = self.meta_cache.as_ref()?;
        let name_str = name.to_string();
        cache
            .get(&format!("{name_str}:full"))
            .or_else(|| cache.get(&name_str))
            .filter(|meta| meta.name == name_str)
    }

    /// Per-`(registry, name)` on-disk mirror read of the full
    /// packument's per-version `time` map. Returns `None` when no
    /// mirror exists yet, no `cache_dir` was supplied, or the mirror
    /// has no `time` payload — the caller then falls through to the
    /// next layer of [`Self::resolve_published_at`].
    ///
    /// Mirrors upstream's
    /// [`readLocalMetaTime`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L714-L727).
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
        cell.get_or_init(|| async {
            let mirror_path = crate::mirror::get_pkg_mirror_path(
                cache_dir,
                crate::mirror::FULL_META_DIR,
                registry,
                &name.to_string(),
            )
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
        let opts = FetchAttestationOptions {
            registry,
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
        };
        fetch_attestation_published_at(&name.to_string(), version, &opts)
            .await
            .map_err(|err| err.to_string())
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
            // during the same install (`{registry}\x00{name}:full` key in
            // the shared metaCache, populated when `pickPackage` upgrades
            // for `minimumReleaseAge`), reuse it. Abbreviated entries are
            // rejected here — `fail_if_trust_downgraded` needs per-version
            // `time` and per-version trust evidence, both of which only
            // the full form carries.
            let shared =
                self.meta_cache.as_ref().and_then(|cache| cache.get(&format!("{key}:full")));
            if let Some(meta) = shared {
                return Ok(Arc::new(project_trust_meta(meta.as_ref())));
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
            retry_opts: self.retry_opts,
        };
        fetch_full_metadata_cached(&name.to_string(), &opts).await.map_err(|err| err.to_string())
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
            // packument lookup; skip them.
            if t.git_hosted.unwrap_or(false) {
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
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_) => None,
    }
}

fn is_excluded(policy: Option<&PackageVersionPolicy>, name: &PkgName, version: &str) -> bool {
    let Some(policy) = policy else { return false };
    match policy.matches(&name.to_string()) {
        pacquet_config::version_policy::PolicyMatch::No => false,
        pacquet_config::version_policy::PolicyMatch::AnyVersion => true,
        pacquet_config::version_policy::PolicyMatch::ExactVersions(versions) => {
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

fn build_policy_snapshot(
    minimum_release_age: u64,
    sorted_min_age_excludes: &[String],
    trust_policy: Option<TrustPolicy>,
    sorted_trust_excludes: &[String],
    trust_policy_ignore_after: Option<u64>,
) -> serde_json::Map<String, JsonValue> {
    let mut map = serde_json::Map::new();
    // Marks runs that enforced the (unconditional) tarball-URL binding so
    // `can_trust_past_check` rejects pre-rule cache records and re-verifies.
    map.insert("tarballUrlBinding".to_string(), JsonValue::Bool(true));
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
/// Mirrors pnpm's `projectTrustMeta` in
/// [`createNpmResolutionVerifier.ts`](https://github.com/pnpm/pnpm/blob/main/resolving/npm-resolver/src/createNpmResolutionVerifier.ts).
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
    }
}

fn project_trust_package_version(version: &PackageVersion) -> PackageVersion {
    let attestations =
        version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).map(|prov| {
            pacquet_registry::AttestationsDist { provenance: Some(prov.clone()), url: None }
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
        // the upstream dependency graph.
        name: String::new(),
        version: version.version.clone(),
        dist: PackageDistribution {
            integrity: None,
            shasum: None,
            tarball: String::new(),
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
///
/// Mirrors upstream's
/// [`projectAbbreviatedMeta`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L702-L707).
fn project_abbreviated_meta(meta: &Package) -> crate::lookup_context::AbbreviatedMetaProjection {
    let version_tarballs = meta
        .versions
        .iter()
        .map(|(version, manifest)| (version.clone(), manifest.dist.tarball.clone()))
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
    crate::lookup_context::AbbreviatedMetaProjection {
        modified: meta.modified.clone(),
        version_tarballs: Some(version_tarballs),
        version_dist_stats: Some(version_dist_stats),
    }
}

fn same_tarball_url(left: &str, right: &str) -> bool {
    canonical_tarball_url(left) == canonical_tarball_url(right)
}

/// Mirror upstream's `canonicalTarballUrl`: parse-and-reserialize to drop
/// default ports (`:443`/`:80`, what pnpm's `normalizeRegistryUrl` does via
/// `new URL(...).toString()`), decode the `%2f` scoped-name separator, then
/// ignore the scheme — so a benign http/https, default-port, or encoding
/// difference between the lockfile URL and the registry metadata isn't read
/// as tampering.
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
