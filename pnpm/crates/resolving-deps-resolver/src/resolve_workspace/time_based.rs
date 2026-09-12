use super::{
    BTreeMap, DateTime, DependencyGroup, Duration, PassSettings, ResolveImporterOptions, Resolver,
    SortedImporters, Utc, WorkspaceImporter, importer_direct_wanted_specs,
    parse_packument_timestamp,
};

/// The `minimumReleaseAge` cutoff is set uniformly on every importer's
/// `base_opts.published_by` by the install layer; it is the upper bound
/// on the time-based cutoff.
pub(super) async fn time_cutoff<Chain>(
    resolver: &Chain,
    sorted: &SortedImporters<'_, '_>,
    dependency_groups: &[DependencyGroup],
    settings: &PassSettings,
) -> TimeBasedCutoff
where
    Chain: Resolver + ?Sized,
{
    let maximum_published_by = sorted.opts.first().and_then(|opts| opts.base_opts.published_by);
    if !settings.time_based {
        return TimeBasedCutoff { published_by: maximum_published_by, time: BTreeMap::new() };
    }
    compute_time_based_cutoff(resolver, sorted, dependency_groups, settings, maximum_published_by)
        .await
}

/// What a `time-based` pre-pass learned about the direct dependencies.
pub(super) struct TimeBasedCutoff {
    /// The ceiling every transitive dependency's publish date must
    /// respect.
    pub(super) published_by: Option<DateTime<Utc>>,
    /// Publish date per direct dependency, for the lockfile's `time:`
    /// section.
    pub(super) time: BTreeMap<String, String>,
}

/// Resolve every importer's direct dependencies and derive the
/// `time-based` publish-date cutoff for transitive deps.
///
/// Each direct dependency's publish date comes from its packument, or —
/// against a registry whose abbreviated metadata omits publish times —
/// from `recorded_time`, the date the lockfile's `time:` section
/// recorded for it. The cutoff is the newest of those dates plus an
/// hour, clamped by `maximum_published_by`.
///
/// Only the direct deps' publish date is read here, so the throwaway
/// resolves warm the resolver's packument cache for the real walk that
/// follows. Resolver errors are ignored here — the real walk surfaces
/// them.
pub(super) async fn compute_time_based_cutoff<Chain>(
    resolver: &Chain,
    sorted: &SortedImporters<'_, '_>,
    dependency_groups: &[DependencyGroup],
    settings: &PassSettings,
    maximum_published_by: Option<DateTime<Utc>>,
) -> TimeBasedCutoff
where
    Chain: Resolver + ?Sized,
{
    let mut time = BTreeMap::new();
    for (importer, opts) in sorted.importers.iter().zip(&sorted.opts) {
        record_direct_publish_dates(
            resolver,
            importer,
            opts,
            dependency_groups,
            settings,
            &mut time,
        )
        .await;
    }

    let newest =
        time.values().filter_map(|published_at| parse_packument_timestamp(published_at)).max();
    let candidate = newest.and_then(|date| date.checked_add_signed(Duration::hours(1)));
    let published_by = match (candidate, maximum_published_by) {
        (Some(candidate), Some(maximum)) => Some(candidate.min(maximum)),
        (Some(candidate), None) => Some(candidate),
        (None, maximum) => maximum,
    };
    TimeBasedCutoff { published_by, time }
}

/// Resolve one importer's direct deps and record each one's publish
/// date, falling back to the date the lockfile recorded for it.
pub(super) async fn record_direct_publish_dates<Chain>(
    resolver: &Chain,
    importer: &WorkspaceImporter<'_>,
    opts: &ResolveImporterOptions,
    dependency_groups: &[DependencyGroup],
    settings: &PassSettings,
    time: &mut BTreeMap<String, String>,
) where
    Chain: Resolver + ?Sized,
{
    let Ok(specs) = importer_direct_wanted_specs(
        importer.manifest,
        dependency_groups.iter().copied(),
        opts.auto_install_peers,
        &opts.catalogs,
    ) else {
        return;
    };
    let mut direct_opts = opts.base_opts.clone();
    direct_opts.pick_lowest_version = settings.pick_lowest_direct;
    for spec in specs {
        let Ok(Some(result)) = resolver
            .resolve(&crate::resolve_dependency_tree::wanted_from_spec(spec), &direct_opts)
            .await
        else {
            continue;
        };
        let published_at = result
            .published_at
            .or_else(|| settings.recorded_time.as_ref()?.get(result.id.as_str()).cloned());
        if let Some(published_at) = published_at {
            time.insert(result.id.into_inner(), published_at);
        }
    }
}
