use super::{
    Arc, DateTime, HashSet, Package, PackageMetaCache, PackageVersion, PackageVersionPolicy,
    PickPackageContext, PickPackageError, PickPackageFromMetaOptions, PickPackageOptions,
    RegistryPackageSpec, RegistryPackageSpecType, SkippedTimeCheck, TrustPolicy, Utc,
    VersionSelectors, filter_pkg_metadata_versions, pick_lowest_version_by_version_range,
    pick_package_from_meta, pick_stable_cached_range_version, pick_version_by_version_range,
    warn_missing_time_once,
};
use crate::OfflineStoreView;
use crate::PickPackageFromMetaError;
use pnpm_store_dir::store_index_key;

/// Whether a pick made from a registry-unverified entry can be returned as
/// is: an offline-leaning resolve, a lowest-version pick and an exact
/// version spec all answer the same from a stale mirror, and so does a range
/// whose pick is the one the mirror is already stable on.
pub(super) fn unverified_pick_is_safe<Cache: PackageMetaCache>(
    ctx: &PickPackageContext<'_, Cache>,
    spec: &RegistryPackageSpec,
    opts: &PickPackageOptions<'_>,
    meta: &Arc<Package>,
    picked: Option<&Arc<PackageVersion>>,
) -> bool {
    let Some(picked) = picked else { return false };
    if ctx.cache_policy.prefer_offline
        || opts.pick_lowest_version
        || matches!(spec.spec_type, RegistryPackageSpecType::Version)
    {
        return true;
    }
    let stable_range_pick = matches!(spec.spec_type, RegistryPackageSpecType::Range)
        && !opts.include_latest_tag
        && !opts.request.update_checksums
        && opts.policy.published_by.is_none()
        && opts.policy.trust_policy != Some(TrustPolicy::NoDowngrade)
        && opts.blocked_versions.is_none();
    if !stable_range_pick {
        return false;
    }
    pick_stable_cached_range_version(meta, &spec.fetch_spec, opts.preferred_version_selectors)
        .is_some_and(|stable| picked.version.to_string() == stable)
}

/// Same fields as [`PickPackageOptions`] minus the dispatcher-only
/// ones (registry, `dry_run`); plus the `ignore_missing_time_field`
/// pull-up from the context.
pub(super) struct PickerOpts<'a> {
    pub(super) preferred_version_selectors: Option<&'a VersionSelectors>,
    pub(super) published_by: Option<DateTime<Utc>>,
    pub(super) published_by_exclude: Option<&'a PackageVersionPolicy>,
    pub(super) pick_lowest_version: bool,
    pub(super) include_latest_tag: bool,
    pub(super) ignore_missing_time_field: bool,
}

/// Picker that may throw a recoverable
/// [`PickPackageFromMetaError::MissingTime`] — orchestrator callers
/// swallow that on the fast paths so the network fetch can replace
/// abbreviated metadata with full.
pub(super) fn pick_matching_version_fast(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    if picker_opts.published_by.is_some() {
        pick_respecting_min_release_age(picker_opts, spec, meta)
    } else {
        pick_ignoring_release_age(picker_opts, spec, meta)
    }
}

pub(super) fn pick_from_meta_fast(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: Arc<Package>,
    blocked_versions: Option<&HashSet<String>>,
) -> Result<(Arc<Package>, Option<Arc<PackageVersion>>), PickPackageFromMetaError> {
    let meta = filter_blocked_versions(meta, blocked_versions);
    if meta.versions.is_empty() && blocked_versions.is_some_and(|blocked| !blocked.is_empty()) {
        return Ok((meta, None));
    }
    let picked = pick_matching_version_fast(picker_opts, spec, &meta)?;
    Ok((meta, picked))
}

pub(super) fn pick_from_meta(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: Arc<Package>,
    blocked_versions: Option<&HashSet<String>>,
) -> Result<(Arc<Package>, Option<Arc<PackageVersion>>), PickPackageFromMetaError> {
    let meta = filter_blocked_versions(meta, blocked_versions);
    if meta.versions.is_empty() && blocked_versions.is_some_and(|blocked| !blocked.is_empty()) {
        return Ok((meta, None));
    }
    let picked = pick_matching_version_final(picker_opts, spec, &meta)?;
    Ok((meta, picked))
}

pub(super) fn filter_blocked_versions(
    meta: Arc<Package>,
    blocked_versions: Option<&HashSet<String>>,
) -> Arc<Package> {
    let Some(blocked_versions) = blocked_versions else {
        return meta;
    };
    if blocked_versions.is_empty() {
        return meta;
    }
    Arc::new(filter_pkg_metadata_versions(&meta, |version| !blocked_versions.contains(version)))
}

/// Picker used at terminal return sites where there's no further
/// fall-through. When `ignore_missing_time_field` is on, a
/// [`PickPackageFromMetaError::MissingTime`] surfaces as a one-shot
/// warning and the picker retries without `publishedBy`.
pub(super) fn pick_matching_version_final(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    match pick_matching_version_fast(picker_opts, spec, meta) {
        Ok(picked) => Ok(picked),
        Err(PickPackageFromMetaError::MissingTime { pkg_name })
            if picker_opts.ignore_missing_time_field =>
        {
            warn_missing_time_once(&pkg_name, SkippedTimeCheck::MinimumReleaseAge);
            let fallback = PickerOpts {
                preferred_version_selectors: picker_opts.preferred_version_selectors,
                published_by: None,
                published_by_exclude: None,
                pick_lowest_version: picker_opts.pick_lowest_version,
                include_latest_tag: picker_opts.include_latest_tag,
                ignore_missing_time_field: picker_opts.ignore_missing_time_field,
            };
            pick_matching_version_fast(&fallback, spec, meta)
        }
        Err(other) => Err(other),
    }
}

/// `publishedBy` is active: it narrows which versions are on offer, and
/// `pick_lowest_version` decides which end of what is left to take. The
/// fallback deliberately drops the maturity filter so a range no mature
/// version satisfies still yields a pick, which the install layer
/// reports as a violation.
pub(super) fn pick_respecting_min_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    run_picker(picker_opts, spec, |target_spec| {
        let pick_mature = if picker_opts.pick_lowest_version {
            pick_lowest_version_by_version_range
        } else {
            pick_version_by_version_range
        };
        let mature =
            pick_package_from_meta(pick_mature, &meta_opts(picker_opts), meta, target_spec)?;
        if mature.is_some() {
            return Ok(mature);
        }
        let fallback_opts = PickPackageFromMetaOptions {
            preferred_version_selectors: picker_opts.preferred_version_selectors,
            published_by: None,
            published_by_exclude: None,
        };
        pick_package_from_meta(
            pick_lowest_version_by_version_range,
            &fallback_opts,
            meta,
            target_spec,
        )
    })
}

/// `publishedBy` is off: respect `pickLowestVersion`.
pub(super) fn pick_ignoring_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    run_picker(picker_opts, spec, |target_spec| {
        if picker_opts.pick_lowest_version {
            pick_package_from_meta(
                pick_lowest_version_by_version_range,
                &meta_opts(picker_opts),
                meta,
                target_spec,
            )
        } else {
            pick_package_from_meta(
                pick_version_by_version_range,
                &meta_opts(picker_opts),
                meta,
                target_spec,
            )
        }
    })
}

/// `include_latest_tag` runner. When the flag is off, just delegate
/// to the inner picker. When on, additionally pick against the
/// `latest` tag and return the higher of the two.
pub(super) fn run_picker<PickOne>(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    pick_one: PickOne,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>
where
    PickOne:
        Fn(&RegistryPackageSpec) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>,
{
    let current = pick_one(spec)?;
    if !picker_opts.include_latest_tag {
        return Ok(current);
    }
    let mut latest_spec = RegistryPackageSpec::latest_tag(spec.name.clone());
    latest_spec.normalized_bare_specifier.clone_from(&spec.normalized_bare_specifier);
    let latest = pick_one(&latest_spec)?;
    Ok(pick_max(current, latest))
}

/// Higher-version-wins between two optional picks. Treats `None`
/// as "no pick" so a single satisfying option wins by default.
pub(super) fn pick_max(
    lhs: Option<Arc<PackageVersion>>,
    rhs: Option<Arc<PackageVersion>>,
) -> Option<Arc<PackageVersion>> {
    match (lhs, rhs) {
        (None, rhs) => rhs,
        (lhs, None) => lhs,
        (Some(lhs), Some(rhs)) => {
            if lhs.version < rhs.version {
                Some(rhs)
            } else {
                Some(lhs)
            }
        }
    }
}

pub(super) fn meta_opts<'a>(picker_opts: &'a PickerOpts<'_>) -> PickPackageFromMetaOptions<'a> {
    PickPackageFromMetaOptions {
        preferred_version_selectors: picker_opts.preferred_version_selectors,
        published_by: picker_opts.published_by,
        published_by_exclude: picker_opts.published_by_exclude,
    }
}

/// The offline adjustment: when the pick the preferences made names a version
/// the store does not hold, the fetcher could only reject it with
/// `ERR_PNPM_NO_OFFLINE_TARBALL`, so the pick is redone over the packument
/// narrowed to the store-held versions; when nothing store-held satisfies the
/// spec, the original pick returns so the existing failure surfaces unchanged
/// ([pnpm/pnpm#10715](https://github.com/pnpm/pnpm/issues/10715)).
/// An exact-version or tag pick names its target outright — only a range has
/// older alternatives worth falling back to.
///
/// `unfiltered_meta` is the packument before `blocked_versions` filtering:
/// the memo is keyed by route alone, so it must be derived from metadata no
/// caller has pre-narrowed, and each caller's block set applies only to its
/// own final re-pick.
#[expect(
    clippy::too_many_arguments,
    reason = "the inputs are independent pick-time values; bundling them moves the fields into a wrapper without removing work"
)]
pub(super) async fn pick_from_meta_offline(
    store_view: Option<&OfflineStoreView>,
    route_key: &str,
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    unfiltered_meta: &Arc<Package>,
    meta: Arc<Package>,
    picked: Option<Arc<PackageVersion>>,
    blocked_versions: Option<&HashSet<String>>,
) -> Result<(Arc<Package>, Option<Arc<PackageVersion>>), PickPackageError> {
    let Some(picked_version) = picked.as_ref() else {
        return Ok((meta, None));
    };
    if !matches!(spec.spec_type, RegistryPackageSpecType::Range) {
        return Ok((meta, picked));
    }
    let Some(store_view) = store_view else {
        return Ok((meta, picked));
    };
    // Fast path: the pick the preferences already made is installable
    // offline — one presence check, no re-pick.
    let Some(integrity) = picked_version.dist.integrity.as_ref() else {
        return Ok((meta, picked));
    };
    let picked_key = store_index_key(
        &integrity.to_string(),
        &format!("{}@{}", meta.name, picked_version.version),
    );
    if store_view.holds(&picked_key) {
        return Ok((meta, picked));
    }
    let Some(narrowed) = store_view.narrowed(route_key, unfiltered_meta).await else {
        return Ok((meta, picked));
    };
    let (narrowed_meta, narrowed_pick) =
        pick_from_meta(picker_opts, spec, narrowed, blocked_versions)?;
    if let Some(adjusted) = narrowed_pick {
        return Ok((narrowed_meta, Some(adjusted)));
    }
    Ok((meta, picked))
}
