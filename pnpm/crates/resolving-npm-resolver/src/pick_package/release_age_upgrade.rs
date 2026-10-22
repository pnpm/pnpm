use super::{
    Arc, FetchMetadataError, Package, PackageMetaCache, PackumentFetchLocker, PickPackageContext,
    PickPackageError, PickPackageOptions, RegistryPackageSpec, Semaphore,
    parse_packument_timestamp,
};
use crate::{
    FetchFullMetadataCachedOptions, fetch_full_metadata_cached::fetch_release_age_upgrade,
};

/// Outcome of [`maybe_upgrade_abbreviated_meta_for_release_age`]. Its `meta`
/// is marked as checked whether or not the upgrade happened: a registry
/// whose full form is no more complete than its abbreviated one still
/// answers `200`, and without the mark every later pick in the install
/// would ask for the same full document again.
pub(super) struct UpgradeOutcome {
    /// The packument the orchestrator should pick from. Either the
    /// original meta (no-upgrade arm — same `Arc` as the input) or
    /// the full meta the upgrade fetched, already in the abbreviated mirror
    /// unless this is a dry run.
    pub(super) meta: Arc<Package>,
    upgraded: bool,
}

impl UpgradeOutcome {
    fn unchanged(meta: Arc<Package>) -> Self {
        UpgradeOutcome { meta, upgraded: false }
    }

    /// `true` when the orchestrator should write `meta` back to the
    /// in-memory cache.
    pub(super) fn upgraded(&self) -> bool {
        self.upgraded
    }
}

pub(super) async fn maybe_upgrade_abbreviated_meta_for_release_age<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    full_metadata: bool,
    cache_key: &str,
    mut meta: Arc<Package>,
) -> Result<UpgradeOutcome, PickPackageError> {
    if !release_age_upgrade_needed(ctx, spec, opts, full_metadata, cache_key, &meta) {
        return Ok(UpgradeOutcome::unchanged(meta));
    }
    let limit = release_age_upgrade_limit(ctx.metadata.fetch_locker, cache_key);
    let _permit =
        limit.acquire().await.expect("release-age upgrade semaphore should not be closed");
    // Waiting for the permit may have handed the winner's work to us: pick up
    // whatever it left in the cache and re-run the guards before spending a
    // round trip of our own. A checksum refresh skips the cache on purpose —
    // it is the one caller holding a document fresher than the cached one.
    if !opts.request.update_checksums
        && let Some(cached) = ctx.metadata.meta_cache.get(cache_key)
    {
        meta = cached.meta;
    }
    if ctx.metadata.fetch_locker.release_age_upgrade_was_checked(cache_key, &meta)
        || meta.time.is_some()
    {
        return Ok(UpgradeOutcome::unchanged(meta));
    }
    // The cached fetcher sends the abbreviated mirror's validators only when
    // it recorded the full document: an entity tag and a `Last-Modified` date
    // describe one representation.
    let fetch_opts = FetchFullMetadataCachedOptions {
        registry: opts.registry,
        cache_dir: ctx.metadata.cache_dir,
        full_metadata: false,
        filter_metadata: false,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        http: ctx.metadata.http,
    };
    match fetch_release_age_upgrade(&spec.name, &fetch_opts, !opts.request.dry_run).await {
        Ok(upgraded) => {
            let meta = Arc::new(upgraded);
            ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
            Ok(UpgradeOutcome { meta, upgraded: true })
        }
        Err(error) if !matches!(error, FetchMetadataError::NotModifiedWithoutCache { .. }) => {
            Err(error.into())
        }
        // The registry declined to hand over a body. To a request without
        // validators it says so by repeating an unsolicited `304` until the
        // fetcher gives up with `NotModifiedWithoutCache`. It has no fuller
        // form of this document, and an upgrade that cannot happen must not
        // fail an install that would otherwise succeed: the maturity check
        // falls back to the warn-or-error gate
        // `minimum_release_age_ignore_missing_time` already governs.
        Err(_) => declined_upgrade(ctx, opts, cache_key, meta),
    }
}

fn declined_upgrade<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    opts: &PickPackageOptions<'_>,
    cache_key: &str,
    meta: Arc<Package>,
) -> Result<UpgradeOutcome, PickPackageError> {
    ctx.metadata.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
    if !opts.request.dry_run {
        ctx.metadata.meta_cache.set(cache_key.to_string(), Arc::clone(&meta));
    }
    Ok(UpgradeOutcome::unchanged(meta))
}

/// Upgrade abbreviated metadata to full when the maturity check needs
/// per-version timestamps.
///
/// When the resolver default-fetched abbreviated metadata but
/// `published_by` is active, the per-version `time` map is missing
/// so the maturity check would silently degrade to the warn-and-skip
/// fallback. This function detects that and re-fetches full metadata
/// when the package's top-level `modified` field shows it was
/// touched after the maturity cutoff. Returns the original meta
/// untouched in every other case.
///
/// The early returns are guard rails:
///
/// - a policy that forbids the registry: no fetch is available, so what
///   we have stands.
/// - `opts.published_by.is_none()`: maturity check disabled.
/// - `meta.time.is_some()`: the packument carries per-version publish
///   timestamps. Documents whose `time` could not decide maturity are
///   normalized to `None` at the parse boundary (see
///   [`Package::drop_incomplete_publish_times`]), so a map that is here
///   is complete. Nothing to upgrade.
/// - an upgrade fetch already ran for this document earlier in the
///   install (see [`PackumentFetchState`](super::metadata_cache::PackumentFetchState)): its outcome stands for
///   every later pick, so asking again is pure waste.
/// - `opts.published_by_exclude` matches the package: caller has
///   opted this package out of the policy.
/// - `meta.modified.is_some()` and parses as a date `<= cutoff`:
///   every version in the packument was published at or before the
///   cutoff, so the abbreviated form is enough. Inclusive at the
///   boundary on purpose, matching the per-version `<=` filter in
///   [`filter_pkg_metadata_by_publish_date`](crate::filter_pkg_metadata_by_publish_date).
///
/// On upgrade the call fetches through [`fetch_release_age_upgrade`],
/// which stores the full document in the *abbreviated* mirror, so the next
/// install sees `time` populated and skips the upgrade fetch.
pub(super) fn release_age_upgrade_needed<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    full_metadata: bool,
    cache_key: &str,
    meta: &Arc<Package>,
) -> bool {
    if ctx.cache_policy.network.forbids_registry()
        || full_metadata
        || meta.time.is_some()
        || ctx.metadata.fetch_locker.release_age_upgrade_was_checked(cache_key, meta)
    {
        return false;
    }
    if opts.policy.trust_policy == Some(super::TrustPolicy::NoDowngrade) {
        return true;
    }
    let Some(cutoff) = opts.policy.published_by else { return false };
    if !opts.policy.release_age_applies_to(&spec.name) {
        return false;
    }
    // Inclusive `<=` at the boundary: matches the per-version `<=` filter in
    // `filter_pkg_metadata_by_publish_date`. When `modified` is missing or
    // unparsable this falls through to the upgrade — better to spend one
    // extra fetch than to silently bypass the maturity check.
    let modified_before_cutoff = meta.modified
        .as_deref()
        .and_then(parse_packument_timestamp)
        .is_some_and(|modified| modified <= cutoff);
    !modified_before_cutoff
}

/// Coalesce concurrent upgrades separately from the packument fetch permit,
/// which is already held by callers during this upgrade.
pub(super) fn release_age_upgrade_limit(
    fetch_locker: &PackumentFetchLocker,
    cache_key: &str,
) -> Arc<Semaphore> {
    Arc::clone(
        fetch_locker.limits
            .entry(format!("{cache_key}#release-age-upgrade"))
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .value(),
    )
}
