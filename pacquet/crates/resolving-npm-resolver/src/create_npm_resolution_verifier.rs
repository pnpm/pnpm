//! Npm-side implementation of the [`ResolutionVerifier`] trait.
//!
//! Verbatim port of pnpm's
//! [`createNpmResolutionVerifier.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts).
//!
//! The factory takes the install-time policy (cutoff time, exclude
//! patterns, trust policy, named registries) and returns a verifier
//! when at least one policy is active. The verifier inspects each
//! npm-registry-resolved lockfile entry, applies the
//! `minimumReleaseAge` and/or `trustPolicy='no-downgrade'` checks,
//! and surfaces violations through [`ResolutionVerification::Err`].
//!
//! The publish-timestamp lookup walks a 4-layer fallback chain
//! (abbreviated-modified shortcut → local mirror → attestation
//! endpoint → full packument fetch); the trust check separately
//! reads the full packument to walk version history. Per-install
//! dedup of every network/disk call lives in
//! [`PublishedAtLookupContext`] so verifying many pinned versions of
//! the same package costs at most one fetch per layer.
//!
//! Phase 4 stubs the abbreviated-shortcut and on-disk-mirror layers
//! (no cached fetcher / no mirror yet); Phase 5 ports
//! `fetchFullMetadataCached.ts` and swaps the full-meta calls behind
//! that wrapper without changing this module's call sites.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use pacquet_config::{TrustPolicy, version_policy::PackageVersionPolicy};
use pacquet_lockfile::{LockfileResolution, PkgName};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::Package;
use pacquet_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use pipe_trait::Pipe;
use serde_json::Value as JsonValue;

use crate::{
    FetchAttestationOptions, FetchFullMetadataCachedOptions, TrustCheckOptions, TrustViolation,
    fetch_attestation_published_at, fetch_full_metadata_cached,
    lookup_context::{PublishedAtLookupContext, PublishedAtTimeMap, package_key, version_key},
    named_registry::{build_named_registry_prefixes, pick_registry_for_package},
    trust_checks::fail_if_trust_downgraded,
    violation_codes::{MINIMUM_RELEASE_AGE_VIOLATION_CODE, TRUST_DOWNGRADE_VIOLATION_CODE},
};

/// Options bundle for [`create_npm_resolution_verifier`]. Mirrors
/// upstream's
/// [`CreateNpmResolutionVerifierOptions`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L28-L84).
///
/// The verifier owns the option bag once constructed — these fields
/// flow into [`NpmResolutionVerifier`] verbatim.
#[derive(Debug)]
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
    /// Override for `Utc::now()` when computing the age cutoff and
    /// the `trustPolicyIgnoreAfter` window. `None` falls back to
    /// wall-clock at construction time.
    pub now: Option<DateTime<Utc>>,
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
    now: Option<DateTime<Utc>>,
    policy_snapshot: serde_json::Map<String, JsonValue>,
    lookup_context: PublishedAtLookupContext,
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

/// Returns an [`NpmResolutionVerifier`] when at least one policy is
/// active, [`None`] otherwise. The empty case lets the install side
/// skip building a verifier list, which collapses the fan-out to a
/// straight pass — every lockfile entry yields `Ok`.
///
/// Mirrors upstream's
/// [`createNpmResolutionVerifier`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L98-L253).
pub fn create_npm_resolution_verifier(
    opts: CreateNpmResolutionVerifierOptions,
) -> Option<NpmResolutionVerifier> {
    let age_check_active = opts.minimum_release_age.is_some_and(|minutes| minutes > 0);
    let trust_check_active = matches!(opts.trust_policy, Some(TrustPolicy::NoDowngrade));
    if !age_check_active && !trust_check_active {
        return None;
    }

    let cutoff = if age_check_active {
        let minutes = opts.minimum_release_age.unwrap_or(0);
        let now = opts.now.unwrap_or_else(Utc::now);
        Some(now - chrono::Duration::minutes(minutes as i64))
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

    Some(NpmResolutionVerifier {
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
        now: opts.now,
        policy_snapshot,
        lookup_context: PublishedAtLookupContext::new(),
    })
}

impl ResolutionVerifier for NpmResolutionVerifier {
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
        if !age_applies && !trust_applies {
            return ResolutionVerification::Ok;
        }

        let registry = self.pick_registry(ctx.name, tarball_url);

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
        if let Some(url) = tarball_url
            && let Ok(parsed) = reqwest::Url::parse(url)
        {
            let normalized = parsed.as_str();
            for prefix in &self.named_registry_prefixes {
                if normalized.starts_with(prefix) {
                    return prefix.clone();
                }
            }
        }
        pick_registry_for_package(&self.registries, &name.to_string())
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
        let Ok(parsed) = DateTime::parse_from_rfc3339(&published) else {
            return Some(ResolutionVerification::Err {
                code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
                reason: "publish timestamp is not a valid date".to_string(),
            });
        };
        let parsed = parsed.with_timezone(&Utc);
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
    /// have shipped earlier versions under a `trustedPublisher` (the
    /// higher-rank evidence) and then dropped to plain provenance —
    /// `fail_if_trust_downgraded` correctly flags that.
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
        {
            let cache = self.lookup_context.published_at.lock().await;
            if let Some(value) = cache.get(&key) {
                return Ok(value.clone());
            }
        }
        let value = self.resolve_published_at(registry, name, version).await?;
        let mut cache = self.lookup_context.published_at.lock().await;
        Ok(cache.entry(key).or_insert(value).clone())
    }

    /// Phase 4 walks two of the four upstream layers: the attestation
    /// endpoint, then the full packument. The abbreviated-`modified`
    /// shortcut and the on-disk-mirror read land in Phase 5 alongside
    /// the cached fetchers (`fetchAbbreviatedMetadataCached` /
    /// `loadMeta`) they depend on. Mirrors upstream's
    /// [`resolvePublishedAt`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/createNpmResolutionVerifier.ts#L471-L491)
    /// with the first two `if` blocks skipped.
    async fn resolve_published_at(
        &self,
        registry: &str,
        name: &PkgName,
        version: &str,
    ) -> Result<Option<String>, String> {
        if let Some(value) = self.fetch_attestation_time(registry, name, version).await? {
            return Ok(Some(value));
        }
        let full_meta_time = self.fetch_full_meta_time(registry, name).await?;
        Ok(full_meta_time.and_then(|map| map.get(version).cloned()))
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
        {
            let cache = self.lookup_context.full_meta.lock().await;
            if let Some(entry) = cache.get(&key) {
                return Ok(entry.clone());
            }
        }
        let pkg = match self.fetch_full_meta(registry, name).await {
            Ok(pkg) => pkg,
            Err(reason) => return Err(reason),
        };
        let time_map = pkg.time.as_ref().map(|raw| {
            raw.iter()
                .filter_map(|(version, value)| {
                    value.as_str().map(|ts| (version.clone(), ts.to_string()))
                })
                .collect::<PublishedAtTimeMap>()
                .pipe(Arc::new)
        });
        let mut cache = self.lookup_context.full_meta.lock().await;
        Ok(cache.entry(key).or_insert(time_map).clone())
    }

    async fn fetch_full_meta_for_trust(
        &self,
        registry: &str,
        name: &PkgName,
    ) -> Result<Arc<Package>, String> {
        let key = package_key(registry, &name.to_string());
        {
            let cache = self.lookup_context.full_meta_for_trust.lock().await;
            if let Some(entry) = cache.get(&key) {
                return entry.clone();
            }
        }
        let result = self.fetch_full_meta(registry, name).await.map(Arc::new);
        let mut cache = self.lookup_context.full_meta_for_trust.lock().await;
        cache.entry(key).or_insert(result).clone()
    }

    async fn fetch_full_meta(&self, registry: &str, name: &PkgName) -> Result<Package, String> {
        let opts = FetchFullMetadataCachedOptions {
            registry,
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            cache_dir: self.cache_dir.as_deref(),
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

#[cfg(test)]
mod tests;
