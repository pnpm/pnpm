use super::{
    Arc, FetchFullMetadataOptions, FetchFullMetadataOutcome, Package, PackageMetaCache,
    PackumentFetchLocker, Path, PickPackageContext, PickPackageError, PickPackageOptions,
    PolicyMatch, RegistryPackageSpec, Semaphore, clear_meta, fetch_full_metadata, load_meta,
    parse_packument_timestamp, save_meta_indexed, save_meta_ndjson,
};

/// Outcome of [`maybe_upgrade_abbreviated_meta_for_release_age`].
pub(super) struct UpgradeOutcome {
    /// The packument the orchestrator should pick from. Either the
    /// original meta (no-upgrade arm — same `Arc` as the input) or
    /// a freshly fetched full meta wrapped in a new `Arc`.
    pub(super) meta: Arc<Package>,
    /// `true` when the orchestrator should persist `meta` to the
    /// abbreviated mirror and write it back to the in-memory cache.
    pub(super) upgraded: bool,
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
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    let limit = release_age_upgrade_limit(ctx.fetch_locker, cache_key);
    let _permit =
        limit.acquire().await.expect("release-age upgrade semaphore should not be closed");
    // Waiting for the permit may have handed the winner's work to us: pick up
    // whatever it left in the cache and re-run the guards before spending a
    // round trip of our own. A checksum refresh skips the cache on purpose —
    // it is the one caller holding a document fresher than the cached one.
    if !opts.update_checksums
        && let Some(cached) = ctx.meta_cache.get(cache_key)
    {
        meta = cached.meta;
    }
    if ctx.fetch_locker.release_age_upgrade_was_checked(cache_key, &meta) || meta.time.is_some() {
        return Ok(UpgradeOutcome { meta, upgraded: false });
    }
    let fetch_opts = FetchFullMetadataOptions {
        registry: opts.registry,
        http_client: ctx.http_client,
        auth_headers: ctx.auth_headers,
        full_metadata: true,
        etag: meta.etag.as_deref(),
        modified: meta.modified.as_deref(),
        retry_opts: ctx.retry_opts,
    };
    match fetch_full_metadata(&spec.name, &fetch_opts).await? {
        FetchFullMetadataOutcome::Modified(upgraded) => {
            Ok(UpgradeOutcome { meta: Arc::new(*upgraded), upgraded: true })
        }
        // 304: the full-form representation matched the conditional
        // headers, so the abbreviated meta is still the freshest
        // signal we have. Keep it (the downstream picker falls through
        // to its warn-and-skip path on the missing `time` map) and
        // mark it so no later pick in this install repeats the round trip.
        // The 304 also registry-validated the document, so it may enter the
        // shared metadata cache as verified.
        FetchFullMetadataOutcome::NotModified => {
            ctx.fetch_locker.mark_release_age_upgrade_checked(cache_key, &meta);
            // A `Modified` outcome is marked by the caller instead: it persists
            // the response to the mirror and may hand back a reloaded document,
            // so only the caller knows the `Arc` that ends up in the cache.
            // Both outcomes must be marked — a registry whose full form is no
            // more complete than its abbreviated one would otherwise be
            // re-asked once per dependency edge.
            if !opts.dry_run {
                ctx.meta_cache.set(cache_key.to_string(), Arc::clone(&meta));
            }
            Ok(UpgradeOutcome { meta, upgraded: false })
        }
    }
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
/// - `ctx.offline`: no network allowed. Stick with what we have.
/// - `opts.published_by.is_none()`: maturity check disabled.
/// - `meta.time.is_some()`: the packument carries per-version publish
///   timestamps. Documents whose `time` could not decide maturity are
///   normalized to `None` at the parse boundary (see
///   [`Package::drop_incomplete_publish_times`]), so a map that is here
///   is complete. Nothing to upgrade.
/// - this document already got a `304` from an upgrade fetch earlier in
///   the install (see [`PackumentFetchState`](super::metadata_cache::PackumentFetchState)): the registry has no
///   fuller form of it, so asking again is pure waste.
/// - `opts.published_by_exclude` matches the package: caller has
///   opted this package out of the policy.
/// - `meta.modified.is_some()` and parses as a date `<= cutoff`:
///   every version in the packument was published at or before the
///   cutoff, so the abbreviated form is enough. Inclusive at the
///   boundary on purpose, matching the per-version `<=` filter in
///   [`filter_pkg_metadata_by_publish_date`](crate::filter_pkg_metadata_by_publish_date).
///
/// On upgrade the call uses the network-only [`fetch_full_metadata()`]
/// (not the cached variant) so the response writes back to the
/// abbreviated mirror via [`persist_upgraded_to_mirror`], which
/// intentionally updates the *abbreviated* cache file with full data so
/// the next install sees `time` populated and skips the upgrade fetch.
///
/// The upgrade fetch forwards `meta.etag` and `meta.modified` as
/// conditional headers. When the registry's full-form representation
/// hasn't changed it answers `304 Not Modified` and the abbreviated
/// meta is returned untouched.
/// Whether a `minimumReleaseAge` check needs the full packument this
/// abbreviated one cannot answer from.
pub(super) fn release_age_upgrade_needed<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    full_metadata: bool,
    cache_key: &str,
    meta: &Arc<Package>,
) -> bool {
    if ctx.offline || full_metadata {
        return false;
    }
    let Some(cutoff) = opts.published_by else { return false };
    if meta.time.is_some() || ctx.fetch_locker.release_age_upgrade_was_checked(cache_key, meta) {
        return false;
    }
    let fully_excluded = opts
        .published_by_exclude
        .is_some_and(|policy| matches!(policy.matches(&spec.name), PolicyMatch::AnyVersion));
    if fully_excluded {
        return false;
    }
    // Inclusive `<=` at the boundary: matches the per-version `<=` filter in
    // `filter_pkg_metadata_by_publish_date`. When `modified` is missing or
    // unparsable this falls through to the upgrade — better to spend one
    // extra fetch than to silently bypass the maturity check.
    let modified_before_cutoff = meta
        .modified
        .as_deref()
        .and_then(parse_packument_timestamp)
        .is_some_and(|modified| modified <= cutoff);
    !modified_before_cutoff
}

/// Write the upgraded full metadata back to `pkg_mirror` (which
/// points at the abbreviated cache because the picker is in
/// abbreviated mode). A write failure logs at debug and the install
/// proceeds — the next install simply re-triggers the upgrade fetch.
///
/// On a successful indexed save, returns the just-persisted mirror
/// reloaded in its file-backed form so the caller can cache *it*
/// instead of the response-body-backed document — upgraded packuments
/// are the largest documents an install handles, and caching the
/// in-memory form kept every full body resident for the rest of the
/// resolution.
pub(super) fn persist_upgraded_to_mirror(
    pkg_mirror: &Path,
    meta: &Package,
    filter_metadata: bool,
) -> Option<Package> {
    let save_result = if filter_metadata {
        let meta_for_cache = match clear_meta(meta) {
            Ok(meta_for_cache) => meta_for_cache,
            Err(error) => {
                tracing::debug!(
                    target: "pnpm_resolving_npm_resolver::pick_package",
                    ?error,
                    path = %pkg_mirror.display(),
                    "could not filter upgraded mirror metadata",
                );
                return None;
            }
        };
        save_meta_ndjson(pkg_mirror, &meta_for_cache, meta.etag.as_deref())
    } else {
        save_meta_indexed(pkg_mirror, meta, meta.etag.as_deref())
    };
    match save_result {
        Ok(()) if !filter_metadata => load_meta(pkg_mirror),
        Ok(()) => None,
        Err(error) => {
            tracing::debug!(
                target: "pnpm_resolving_npm_resolver::pick_package",
                ?error,
                path = %pkg_mirror.display(),
                "could not write upgraded meta to mirror; skipping persist",
            );
            None
        }
    }
}

/// Coalesce concurrent upgrades separately from the packument fetch permit,
/// which is already held by callers during this upgrade.
pub(super) fn release_age_upgrade_limit(
    fetch_locker: &PackumentFetchLocker,
    cache_key: &str,
) -> Arc<Semaphore> {
    Arc::clone(
        fetch_locker
            .limits
            .entry(format!("{cache_key}#release-age-upgrade"))
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .value(),
    )
}
