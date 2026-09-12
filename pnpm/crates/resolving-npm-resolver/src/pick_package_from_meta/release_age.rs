use super::{
    Arc, DerivedPackuments, OnceCell, Package, PackageVersionPolicy, PackageVersions, PolicyMatch,
    Version, parse_packument_timestamp,
};

/// What the `publishedBy` cutoff leaves visible, as computed by
/// [`apply_published_by_policy`].
pub(crate) struct PublishedByView {
    /// The narrowed packument, or `None` when the input is already the
    /// view: the policy excludes the package wholesale, or the cutoff
    /// could not be applied.
    pub(crate) filtered: Option<Arc<Package>>,

    /// The cutoff could not be applied: the metadata is abbreviated, so
    /// there are no per-version timestamps to filter on. Whether that
    /// is fatal is the caller's call — the pick needs full metadata to
    /// honor the cutoff, while a caller reasoning about a pick that
    /// already succeeded knows the versions cleared the cutoff some
    /// other way.
    pub(crate) needs_full_metadata: bool,
}

/// Narrow `meta` to the versions the `cutoff` admits, honoring the
/// exclusion policy: a package the policy excludes wholesale keeps its
/// unfiltered metadata, and versions the policy names explicitly stay
/// in regardless of their age.
///
/// Every consumer of the cutoff goes through here so they agree on what
/// the policy admits — a baseline that filters differently from the
/// pick would misreport why a version was chosen.
#[must_use]
pub(crate) fn apply_published_by_policy(
    meta: &Package,
    cutoff: chrono::DateTime<chrono::Utc>,
    exclude: Option<&PackageVersionPolicy>,
) -> PublishedByView {
    let exclude_result = exclude.map_or(PolicyMatch::No, |policy| policy.matches(&meta.name));
    if matches!(exclude_result, PolicyMatch::AnyVersion) {
        return PublishedByView { filtered: None, needs_full_metadata: false };
    }
    if meta.time.is_none() {
        return PublishedByView { filtered: None, needs_full_metadata: true };
    }
    let trusted = match &exclude_result {
        PolicyMatch::ExactVersions(versions) => Some(versions.as_slice()),
        _ => None,
    };
    PublishedByView {
        filtered: Some(filter_pkg_metadata_by_publish_date(meta, cutoff, trusted)),
        needs_full_metadata: false,
    }
}

/// Filter a packument to versions published at or before `cutoff`,
/// then rewrite each `dist-tag` to the highest within-cutoff version
/// that still belongs to the tag's original "family" (same major
/// for non-`latest` tags, no newer than the original target, matching
/// prerelease/release status, and preferring non-deprecated versions
/// when both are present).
///
/// The result is memoized on `meta` (see [`DerivedPackuments`]) and
/// shared between callers, so it is handed back behind an [`Arc`]:
/// every caller of one cutoff and trusted-version list gets the same
/// document, whose version manifests are the ones `meta` holds.
///
/// Panics if `meta.time` is `None`. Go through
/// `apply_published_by_policy`, which reports abbreviated metadata as
/// `needs_full_metadata` rather than filtering it.
#[must_use]
pub fn filter_pkg_metadata_by_publish_date(
    meta: &Package,
    cutoff: chrono::DateTime<chrono::Utc>,
    trusted_versions: Option<&[String]>,
) -> Arc<Package> {
    let policy_key = publish_date_policy_key(cutoff, trusted_versions);
    meta.derived.get_or_derive(&policy_key, || {
        filter_pkg_metadata_by_publish_date_uncached(meta, cutoff, trusted_versions)
    })
}

/// Every input the filter's output depends on, in one string: the same
/// packument is served to installs whose cutoff or trusted versions
/// differ, and they must not read each other's view.
///
/// The cutoff goes in at the precision it is compared at: it comes from
/// `Utc::now()` and packument timestamps parse to the same resolution,
/// so a key rounded to the millisecond would let two cutoffs that keep
/// different versions share one derived packument.
pub(super) fn publish_date_policy_key(
    cutoff: chrono::DateTime<chrono::Utc>,
    trusted_versions: Option<&[String]>,
) -> String {
    let mut key = format!("{}.{}", cutoff.timestamp(), cutoff.timestamp_subsec_nanos());
    for trusted in trusted_versions.unwrap_or_default() {
        key.push('\0');
        key.push_str(trusted);
    }
    key
}

pub(super) fn filter_pkg_metadata_by_publish_date_uncached(
    meta: &Package,
    cutoff: chrono::DateTime<chrono::Utc>,
    trusted_versions: Option<&[String]>,
) -> Package {
    let time = meta.time.as_ref().expect(
        "filter_pkg_metadata_by_publish_date called without `time`; \
         caller must check before invoking",
    );

    filter_pkg_metadata_versions_with_dist_tag_bound(
        meta,
        |version| {
            let mature = time
                .get(version)
                .and_then(serde_json::Value::as_str)
                .and_then(parse_packument_timestamp)
                .is_some_and(|date| date <= cutoff);
            let trusted = trusted_versions
                .is_some_and(|allow| allow.iter().any(|allowed| allowed == version));
            mature || trusted
        },
        true,
    )
}

/// Filter a packument's versions by string while keeping dist-tags
/// usable. Tags that still point at a kept version are preserved; tags
/// whose target was removed are rewritten using the generic dist-tag
/// rules: `latest` may move to the best remaining version across any
/// major, while other tags stay within the removed target's
/// major/prerelease lane.
#[must_use]
pub fn filter_pkg_metadata_versions(meta: &Package, keep: impl FnMut(&str) -> bool) -> Package {
    filter_pkg_metadata_versions_with_dist_tag_bound(meta, keep, false)
}

pub(super) fn filter_pkg_metadata_versions_with_dist_tag_bound(
    meta: &Package,
    mut keep: impl FnMut(&str) -> bool,
    bound_dist_tags: bool,
) -> Package {
    // Decide on version strings alone; slots move as raw fragments, so
    // the filter never hydrates a manifest.
    let filtered_versions = meta.versions.filtered(|version| keep(version));
    let dist_tags = repopulate_dist_tags(meta, &filtered_versions, bound_dist_tags);

    Package {
        name: meta.name.clone(),
        dist_tags,
        versions: filtered_versions,
        time: meta.time.clone(),
        modified: meta.modified.clone(),
        etag: meta.etag.clone(),
        homepage: meta.homepage.clone(),
        mutex: std::sync::Arc::clone(&meta.mutex),
        derived: DerivedPackuments::default(),
    }
}

pub(super) fn repopulate_dist_tags(
    meta: &Package,
    filtered_versions: &PackageVersions,
    bound_dist_tags: bool,
) -> std::collections::HashMap<String, String> {
    let mut dist_tags_within_date = std::collections::HashMap::new();
    // Candidate versions parsed once per filter call and shared by
    // every repopulated tag, with the deprecation flag resolved
    // lazily per candidate.
    // Deprecation goes through [`PackageVersions::is_deprecated`]
    // instead of hydrating each candidate's manifest; the hydration
    // per comparison dominated warm-resolve CPU on packuments with
    // out-of-cutoff dist-tags.
    let mut parsed_candidates: Option<Vec<TagCandidate<'_>>> = None;
    for (tag, version) in &meta.dist_tags {
        if filtered_versions.contains_key(version) {
            dist_tags_within_date.insert(tag.clone(), version.clone());
            continue;
        }
        let Ok(original) = Version::parse(version) else { continue };
        let candidates = parsed_candidates.get_or_insert_with(|| {
            filtered_versions
                .keys()
                .filter_map(|raw| {
                    let parsed = Version::parse(raw).ok()?;
                    Some((parsed, raw, OnceCell::new()))
                })
                .collect()
        });
        if let Some(best) =
            best_tag_candidate(candidates, filtered_versions, tag, &original, bound_dist_tags)
        {
            dist_tags_within_date.insert(tag.clone(), best.clone());
        }
    }
    dist_tags_within_date
}

/// A version the filter kept, its raw spelling, and its deprecation flag
/// once something asks for it.
pub(super) type TagCandidate<'a> = (Version, &'a String, OnceCell<bool>);

/// The version a dropped dist-tag moves to: the highest candidate of the
/// tag's own major and prerelease-ness, preferring a non-deprecated one.
/// `bound_dist_tags` keeps the tag from moving forward past the version it
/// pointed at.
pub(super) fn best_tag_candidate<'a>(
    candidates: &'a [TagCandidate<'a>],
    filtered_versions: &PackageVersions,
    tag: &str,
    original: &Version,
    bound_dist_tags: bool,
) -> Option<&'a String> {
    let original_is_prerelease = !original.pre_release.is_empty();
    let deprecated = |slot: &TagCandidate<'a>| -> bool {
        *slot.2.get_or_init(|| filtered_versions.is_deprecated(slot.1))
    };
    let eligible = candidates.iter().filter(|(candidate, _, _)| {
        !(bound_dist_tags && candidate > original)
            && (tag == "latest" || candidate.major == original.major)
            && candidate.pre_release.is_empty() != original_is_prerelease
    });
    let mut best: Option<&TagCandidate<'a>> = None;
    for slot in eligible {
        let (candidate, _, _) = slot;
        let Some(best_slot) = best else {
            best = Some(slot);
            continue;
        };
        let best_deprecated = deprecated(best_slot);
        let candidate_deprecated = deprecated(slot);
        let candidate_wins = (*candidate > best_slot.0 && best_deprecated == candidate_deprecated)
            || (best_deprecated && !candidate_deprecated);
        if candidate_wins {
            best = Some(slot);
        }
    }
    best.map(|slot| slot.1)
}
