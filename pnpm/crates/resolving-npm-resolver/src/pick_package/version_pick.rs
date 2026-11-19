use super::{
    Arc, HashSet, Package, PackageVersion, PackageVersionPolicy, PickPackageError,
    PickPackageFromMetaOptions, RegistryPackageSpec, RegistryPackageSpecType, SkippedTimeCheck,
    Timestamp, VersionSelectors, filter_pkg_metadata_versions, offline_store::tarball_key,
    pick_lowest_version_by_version_range, pick_package_from_meta, pick_version_by_version_range,
    warn_missing_time_once,
};
use crate::{OfflineStoreView, PickPackageFromMetaError};
use node_semver::{Identifier, Version};

/// Same fields as [`super::PickPackageOptions`] minus the dispatcher-only
/// ones (registry, `dry_run`); plus the `ignore_missing_time_field`
/// pull-up from the context.
pub(super) struct PickerOpts<'a> {
    pub(super) preferred_version_selectors: Option<&'a VersionSelectors>,
    pub(super) published_by: Option<Timestamp>,
    pub(super) fallback_published_by: Option<Timestamp>,
    pub(super) published_by_exclude: Option<&'a PackageVersionPolicy>,
    pub(super) pick_lowest_version: bool,
    pub(super) include_latest_tag: bool,
    pub(super) current_version: Option<&'a Version>,
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
                fallback_published_by: None,
                published_by_exclude: None,
                pick_lowest_version: picker_opts.pick_lowest_version,
                include_latest_tag: picker_opts.include_latest_tag,
                current_version: picker_opts.current_version,
                ignore_missing_time_field: picker_opts.ignore_missing_time_field,
            };
            pick_matching_version_fast(&fallback, spec, meta)
        }
        Err(other) => Err(other),
    }
}

/// Picks within the selection cutoff, then the release-age cutoff when it
/// is later. Only the final fallback drops the maturity filter, leaving
/// violation handling to the install layer.
pub(super) fn pick_respecting_min_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    let pick_mature = |target_spec: &RegistryPackageSpec| {
        let pick_by_range = if picker_opts.pick_lowest_version {
            pick_lowest_version_by_version_range
        } else {
            pick_version_by_version_range
        };
        let mature =
            pick_package_from_meta(pick_by_range, &meta_opts(picker_opts), meta, target_spec)?;
        if mature.is_some() {
            return Ok(mature);
        }
        pick_within_fallback_cutoff(picker_opts, target_spec, meta)
    };
    run_picker(
        picker_opts,
        meta,
        spec,
        |target_spec| match pick_mature(target_spec)? {
            Some(mature) => Ok(Some(mature)),
            None => pick_ignoring_maturity(picker_opts, target_spec, meta),
        },
        // An immature channel pick would win over a mature `latest` and
        // hide it.
        pick_mature,
    )
}

fn pick_within_fallback_cutoff(
    picker_opts: &PickerOpts<'_>,
    target_spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    if picker_opts.fallback_published_by <= picker_opts.published_by {
        return Ok(None);
    }
    let fallback_opts = PickPackageFromMetaOptions {
        published_by: picker_opts.fallback_published_by,
        ..meta_opts(picker_opts)
    };
    pick_package_from_meta(pick_lowest_version_by_version_range, &fallback_opts, meta, target_spec)
}

fn pick_ignoring_maturity(
    picker_opts: &PickerOpts<'_>,
    target_spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    let fallback_opts = PickPackageFromMetaOptions {
        preferred_version_selectors: picker_opts.preferred_version_selectors,
        published_by: None,
        published_by_exclude: None,
    };
    pick_package_from_meta(pick_lowest_version_by_version_range, &fallback_opts, meta, target_spec)
}

/// `publishedBy` is off: respect `pickLowestVersion`.
pub(super) fn pick_ignoring_release_age(
    picker_opts: &PickerOpts<'_>,
    spec: &RegistryPackageSpec,
    meta: &Package,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError> {
    let pick_by_range = if picker_opts.pick_lowest_version {
        pick_lowest_version_by_version_range
    } else {
        pick_version_by_version_range
    };
    let pick_one = |target_spec: &RegistryPackageSpec| {
        pick_package_from_meta(pick_by_range, &meta_opts(picker_opts), meta, target_spec)
    };
    run_picker(picker_opts, meta, spec, pick_one, pick_one)
}

/// `include_latest_tag` runner. When the flag is off, just delegate
/// to the inner picker. When on, additionally pick against the
/// `latest` tag and return the higher of the two. An installed prerelease
/// also adds the dist-tag that follows its prerelease channel, picked with
/// `pick_channel_tag`, which a lower stable `latest` would otherwise hide.
pub(super) fn run_picker<PickOne, PickChannelTag>(
    picker_opts: &PickerOpts<'_>,
    meta: &Package,
    spec: &RegistryPackageSpec,
    pick_one: PickOne,
    pick_channel_tag: PickChannelTag,
) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>
where
    PickOne:
        Fn(&RegistryPackageSpec) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>,
    PickChannelTag:
        Fn(&RegistryPackageSpec) -> Result<Option<Arc<PackageVersion>>, PickPackageFromMetaError>,
{
    let current = pick_one(spec)?;
    if !picker_opts.include_latest_tag {
        return Ok(current);
    }
    let tag_spec = |tag: &str| {
        let mut tag_spec = RegistryPackageSpec::latest_tag(spec.name.clone());
        tag.clone_into(&mut tag_spec.fetch_spec);
        tag_spec.normalized_bare_specifier.clone_from(&spec.normalized_bare_specifier);
        tag_spec
    };
    let picked = pick_max(current, pick_one(&tag_spec("latest"))?);
    let channel_tags = picker_opts.current_version
        .map(|current_version| find_prerelease_channel_tags(meta, current_version))
        .unwrap_or_default();
    for (tag, target) in channel_tags {
        // `latest` is already a candidate.
        if tag == "latest" {
            continue;
        }
        // The maturity filter moves an immature tag to an older version,
        // which may be on another channel. Only the tag's own target counts.
        if let Some(channel_pick) = pick_channel_tag(&tag_spec(tag))?
            && channel_pick.version == target
        {
            return Ok(pick_max(picked, Some(channel_pick)));
        }
    }
    Ok(picked)
}

/// The dist-tags that point to published versions on the same prerelease
/// channel as `current_version`, with their targets, highest version first.
/// Empty when `current_version` is not a prerelease.
fn find_prerelease_channel_tags<'meta>(
    meta: &'meta Package,
    current_version: &Version,
) -> Vec<(&'meta str, Version)> {
    let Some(channel) = prerelease_channel(current_version) else {
        return Vec::new();
    };
    let mut tags = meta
        .dist_tags()
        .filter(|(_, version)| meta.versions.contains_key(version))
        .filter_map(|(tag, version)| {
            let version = Version::parse(version).ok()?;
            (prerelease_channel(&version) == Some(channel)).then_some((tag, version))
        })
        .collect::<Vec<_>>();
    tags.sort_by(|(_, version1), (_, version2)| version2.cmp(version1));
    tags
}

/// The leading run of letters in the first prerelease identifier. Build
/// counters, dates, and commit hashes that follow it do not change the
/// channel.
fn prerelease_channel(version: &Version) -> Option<&str> {
    let Some(Identifier::AlphaNumeric(first)) = version.pre_release.first() else {
        return None;
    };
    let end = first
        .find(|char: char| !char.is_ascii_alphabetic())
        .unwrap_or(first.len());
    (end > 0).then(|| &first[..end])
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
/// The narrowing only probes the versions the re-pick could return: those
/// the range admits, or every version when the `latest` tag also competes.
///
/// `unfiltered_meta` is the packument before `blocked_versions` filtering:
/// the memo is keyed by route and range alone, so it must be derived from
/// metadata no caller has pre-narrowed, and each caller's block set applies
/// only to its own final re-pick.
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
    // offline — one presence check, no re-pick. A pick without integrity
    // cannot be verified against the store, so it falls through to the
    // narrowed re-pick, which only offers versions the store can verify.
    let picked_key =
        tarball_key(&meta.name, &picked_version.version.to_string(), &picked_version.dist);
    let picked_is_held = match picked_key {
        Some(key) => store_view.holds(&key).await,
        None => false,
    };
    if picked_is_held {
        return Ok((meta, picked));
    }
    let scan_range = if picker_opts.include_latest_tag { "*" } else { spec.fetch_spec.as_str() };
    let Some(narrowed) = store_view.narrowed(route_key, scan_range, unfiltered_meta).await else {
        return Ok((meta, picked));
    };
    let (_narrowed_meta, narrowed_pick) =
        pick_from_meta(picker_opts, spec, narrowed, blocked_versions)?;
    if let Some(adjusted) = narrowed_pick {
        return Ok((meta, Some(adjusted)));
    }
    Ok((meta, picked))
}

#[cfg(test)]
mod tests;
