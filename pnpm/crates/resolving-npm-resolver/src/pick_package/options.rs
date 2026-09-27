use super::{
    DateTime, HashSet, PackageMetaCache, PackageVersionPolicy, PackumentFetchLocker, Path,
    TrustPolicy, Utc, VersionSelectors,
};

/// Process-shared context every [`super::pick_package`] call reads from.
/// One per install.
pub struct PickPackageContext<'a, Cache: PackageMetaCache> {
    /// Install-wide bias toward full metadata.
    /// `true` forces every pick to use the full packument; `false`
    /// defers to the per-call `opts.optional` flag, defaulting to
    /// abbreviated metadata. The resolver typically leaves this
    /// `false`; the verifier-time fetcher sets it `true` because
    /// it needs `time` and trust evidence for every entry.
    pub full_metadata: bool,
    /// Asked instead of [`Self::full_metadata`] when the caller can answer
    /// per registry — a registry that declares `supportsTimeField` needs no
    /// full metadata for a time-based resolution even when the others do. The
    /// mirror path and cache key below are already keyed by registry, so two
    /// registries may disagree within one install.
    pub needs_full_metadata_for: Option<&'a (dyn Fn(&str) -> bool + Send + Sync)>,
    /// When full metadata is forced, use pnpm's filtered full-metadata
    /// mirror and filtered packument shape.
    pub filter_metadata: bool,
    pub cache_policy: crate::MetadataCachePolicy,
    pub metadata: MetadataRequestContext<'a, Cache>,
}

pub struct MetadataRequestContext<'a, Cache: PackageMetaCache> {
    pub meta_cache: &'a Cache,
    /// Per-cache-key fetch serializer. See [`PackumentFetchLocker`]
    /// for the rationale. Construct once per install via
    /// [`super::shared_packument_fetch_locker`] and thread the same handle
    /// through every [`PickPackageContext`] so the npm and named-
    /// registry resolvers coalesce against the same in-flight set.
    pub fetch_locker: &'a PackumentFetchLocker,
    /// Root of the on-disk metadata mirror. `None` disables every
    /// disk path — the orchestrator goes straight to the network.
    pub cache_dir: Option<&'a Path>,
    pub http: crate::MetadataHttpClient<'a>,
}

#[derive(Default, Clone, Copy)]
pub struct MetadataCachePolicy {
    /// `offline=true` forbids any network access; the picker
    /// surfaces [`super::PickPackageError::NoOfflineMeta`] when the disk
    /// mirror is also empty.
    pub offline: bool,
    /// `prefer_offline=true` reads disk before the network *and*
    /// returns immediately if disk has a satisfying pick.
    pub prefer_offline: bool,
    /// When [`true`], a `minimumReleaseAge` check that hits an
    /// abbreviated packument (no per-version `time`) warns once and
    /// falls back to picking without the maturity filter.
    ///
    /// Reachable when the registry-served packument omits `time`
    /// even after a full-metadata fetch (rare; the official npm
    /// registry always populates `time` for full responses) — the
    /// opt-in stays for parity with the resolver option flag.
    pub ignore_missing_time_field: bool,
}

/// Per-call options the orchestrator threads to the picker.
pub struct PickPackageOptions<'a> {
    /// Default registry URL for the package (or the per-scope URL
    /// when the package is scoped). The orchestrator stitches this
    /// into the mirror path and the conditional GET URL.
    pub registry: &'a str,
    /// Per-importer version-selector bias.
    pub preferred_version_selectors: Option<&'a VersionSelectors>,
    /// Pick the lowest satisfying version instead of the highest.
    /// Honoured under `published_by` too: maturity narrows which
    /// versions are on offer, and this decides which end of what is
    /// left to take.
    pub pick_lowest_version: bool,
    /// Compare the spec-pick against a `latest`-tag pick and keep
    /// the higher of the two. Used by `pnpm add` to make sure a
    /// freshly-added range picks the same version as the
    /// implicit `@latest` would.
    pub include_latest_tag: bool,
    /// Concrete versions to ignore while picking. Used by callers that
    /// apply an external resolver-time guard: after the guard rejects a
    /// candidate, the caller asks the normal picker to try again over
    /// the same packument with that version filtered out.
    pub blocked_versions: Option<&'a HashSet<String>>,
    pub policy: crate::PackagePickPolicy<'a>,
    pub request: crate::MetadataPickRequest,
}

#[derive(Clone, Copy)]
pub struct PackagePickPolicy<'a> {
    /// `minimumReleaseAge` cutoff. `None` disables the maturity
    /// filter for this call.
    pub published_by: Option<DateTime<Utc>>,
    /// `minimumReleaseAgeExclude` policy. `None` skips exclusion.
    pub published_by_exclude: Option<&'a PackageVersionPolicy>,
    /// Trust-policy validation requires current registry metadata.
    pub trust_policy: Option<TrustPolicy>,
}

#[derive(Clone, Copy)]
pub struct MetadataPickRequest {
    /// `true` skips the cache write-back on a 200 response — used when
    /// the install is a pure dry-run (`--lockfile-only`, frozen
    /// lockfile, etc.).
    pub dry_run: bool,
    /// `pnpm update` must see versions published since the mirror was
    /// written, so it does not reuse an ETag-less mirror.
    pub refresh_metadata: bool,
    /// `true` forces this pick to use the full packument because
    /// the dependency carries `optionalDependencies`-specific
    /// fields (`libc`, `cpu`, `os`) the abbreviated form drops
    /// some of — see [pnpm/pnpm#9950](https://github.com/pnpm/pnpm/issues/9950).
    /// Combined with [`PickPackageContext::full_metadata`] via OR:
    /// either knob set to `true` makes the pick request full
    /// metadata.
    pub optional: bool,
    /// `true` forces a conditional registry request so a stale disk
    /// packument can't satisfy the call: the on-disk exact-version
    /// fast path is skipped, and the in-memory cache is bypassed too.
    /// The fast path now promotes disk-loaded packuments into the
    /// in-memory cache, so an entry there can no longer be assumed to
    /// come from this install's own fresh network fetch — on a shared
    /// resolver it might be disk-sourced, which would short-circuit the
    /// revalidation. Backs the `--update-checksums` flag.
    pub update_checksums: bool,
}
