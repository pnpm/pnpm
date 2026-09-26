use super::{
    Arc, FetchFullMetadataOptions, FetchFullMetadataOutcome, FetchMetadataError, Package,
    PackageMetaCache, PackumentFetchLocker, Path, PickPackageContext, PickPackageError,
    PickPackageOptions, PolicyMatch, RegistryPackageSpec, Semaphore, clear_meta, load_meta,
    parse_packument_timestamp, save_meta_indexed, save_meta_ndjson,
};
use crate::fetch_full_metadata::fetch_metadata_document;

/// Outcome of [`maybe_upgrade_abbreviated_meta_for_release_age`].
pub(super) struct UpgradeOutcome {
    /// The packument the orchestrator should pick from. Either the
    /// original meta (no-upgrade arm — same `Arc` as the input) or
    /// a freshly fetched full meta wrapped in a new `Arc`.
    pub(super) meta: Arc<Package>,
    /// `true` when the orchestrator should persist `meta` to the
    /// abbreviated mirror and write it back to the in-memory cache.
    pub(super) upgraded: bool,
    /// `true` when the upgraded response forbade caching. Meaningless
    /// unless `upgraded` is set; the caller already knows the
    /// abbreviated response's policy.
    pub(super) uncacheable: bool,
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
        return Ok(UpgradeOutcome { meta, upgraded: false, uncacheable: false });
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
        return Ok(UpgradeOutcome { meta, upgraded: false, uncacheable: false });
    }
    // An entity tag and a `Last-Modified` date describe one representation, and
    // `meta` holds the abbreviated one. A registry that reuses them across both
    // forms answers `304`, leaving the maturity check without its `time` map.
    let fetch_opts = FetchFullMetadataOptions {
        registry: opts.registry,
        full_metadata: true,
        etag: None,
        modified: None,
        http: ctx.metadata.http,
    };
    match fetch_metadata_document(&spec.name, &fetch_opts).await {
        Ok(fetched) => match fetched.outcome {
            FetchFullMetadataOutcome::Modified(upgraded) => Ok(UpgradeOutcome {
                meta: Arc::new(*upgraded),
                upgraded: true,
                uncacheable: fetched.uncacheable,
            }),
            FetchFullMetadataOutcome::NotModified => declined_upgrade(ctx, opts, cache_key, meta),
        },
        Err(error) if !matches!(error, FetchMetadataError::NotModifiedWithoutCache { .. }) => {
            Err(error.into())
        }
        // The registry declined to hand over a body. Since the request carries
        // no validators, it says so by repeating an unsolicited `304` until
        // `fetch_full_metadata` gives up, which surfaces as
        // `NotModifiedWithoutCache` rather than the `NotModified` outcome.
        // Either way it has no fuller form of this document, and an upgrade
        // that cannot happen must not fail an install that would otherwise
        // succeed: the maturity check falls back to the warn-or-error gate
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
    // A `Modified` outcome is marked by the caller instead: it persists
    // the response to the mirror and may hand back a reloaded document,
    // so only the caller knows the `Arc` that ends up in the cache.
    // Both outcomes must be marked — a registry whose full form is no
    // more complete than its abbreviated one would otherwise be
    // re-asked once per dependency edge.
    if !opts.request.dry_run {
        ctx.metadata.meta_cache.set(cache_key.to_string(), Arc::clone(&meta));
    }
    Ok(UpgradeOutcome { meta, upgraded: false, uncacheable: false })
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
/// On upgrade the call uses the network-only [`crate::fetch_full_metadata()`]
/// (not the cached variant) so the response writes back to the
/// abbreviated mirror via [`persist_upgraded_to_mirror`], which
/// intentionally updates the *abbreviated* cache file with full data so
/// the next install sees `time` populated and skips the upgrade fetch.
pub(super) fn release_age_upgrade_needed<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    full_metadata: bool,
    cache_key: &str,
    meta: &Arc<Package>,
) -> bool {
    if ctx.cache_policy.offline || full_metadata {
        return false;
    }
    let Some(cutoff) = opts.policy.published_by else { return false };
    if meta.time.is_some()
        || ctx.metadata.fetch_locker.release_age_upgrade_was_checked(cache_key, meta)
    {
        return false;
    }
    let fully_excluded = opts.policy.published_by_exclude.is_some_and(|policy| {
        matches!(policy.matches(&spec.name), PolicyMatch::AnyVersion)
    });
    if fully_excluded {
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

/// Write the upgraded full metadata back to `pkg_mirror` (which
/// points at the abbreviated cache because the picker is in
/// abbreviated mode). A write failure logs at debug and the install
/// proceeds. An uncacheable failure also deletes the previous mirror,
/// so the next lookup cannot revalidate that older header.
///
/// An `ETag` identifies one representation, so the full document's tag
/// cannot describe the abbreviated slot this writes into and is dropped.
/// `modified` is kept: it comes from the packument's own `time.modified`,
/// which both representations report identically, so the next abbreviated
/// request is still conditional through `If-Modified-Since`.
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
    uncacheable: bool,
) -> Option<Package> {
    let save_result = if filter_metadata {
        match clear_meta(meta) {
            Ok(meta_for_cache) => save_meta_ndjson(pkg_mirror, &meta_for_cache, None, uncacheable),
            Err(error) => {
                return failed_uncacheable_upgrade_persist(pkg_mirror, uncacheable, &error);
            }
        }
    } else {
        save_meta_indexed(pkg_mirror, meta, None, uncacheable)
    };
    match save_result {
        Ok(()) if !filter_metadata => load_meta(pkg_mirror),
        Ok(()) => None,
        Err(error) => failed_uncacheable_upgrade_persist(pkg_mirror, uncacheable, &error),
    }
}

/// A failed uncacheable upgrade must not leave the previous header, whose
/// validators the next online fetch would send.
fn failed_uncacheable_upgrade_persist(
    pkg_mirror: &Path,
    uncacheable: bool,
    error: &dyn std::fmt::Debug,
) -> Option<Package> {
    tracing::debug!(
        target: "pnpm_resolving_npm_resolver::pick_package",
        ?error,
        path = %pkg_mirror.display(),
        "could not persist upgraded meta to the mirror",
    );
    if uncacheable {
        let _ = std::fs::remove_file(pkg_mirror);
    }
    None
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

#[cfg(test)]
mod tests;
