use super::{
    Arc, DerivedPackuments, DistStats, JsonValue, Package, PackageDistribution, Pipe,
    PublishedAtTimeMap,
};

/// Build a [`Package`] that retains only the fields
/// [`fail_if_trust_downgraded`] reads: the package name, the per-version
/// `time` map, and per-version trust evidence
/// ([`PackageVersions::trust_projection`]). Everything else, including
/// publisher names and emails, is dropped, so the per-install trust-meta
/// cache stays bounded by the trust-evidence footprint, not the full
/// packument size.
///
/// [`fail_if_trust_downgraded`]: crate::trust_checks::fail_if_trust_downgraded
/// [`PackageVersions::trust_projection`]: pnpm_registry::PackageVersions::trust_projection
pub(super) fn project_trust_meta(meta: &Package) -> Package {
    Package {
        name: meta.name.clone(),
        dist_tags: std::collections::HashMap::new(),
        versions: meta.versions.trust_projection(),
        time: meta.time.clone(),
        modified: meta.modified.clone(),
        etag: meta.etag.clone(),
        // `homepage` is only read by `outdated --long`, never by trust
        // verification, so it is dropped here to keep the trust-meta cache
        // bounded by the trust-evidence footprint (see the fn doc).
        homepage: None,
        mutex: std::sync::Arc::new(std::sync::Mutex::new(0)),
        derived: DerivedPackuments::default(),
    }
}

/// Pull the `(modified, versionTarballs)` projection the verifier
/// needs out of a packument document. Works against either the
/// abbreviated or the full form — both carry `modified` and a
/// `versions` map with per-version `dist.tarball`.
pub(super) fn project_abbreviated_meta(
    meta: &Package,
    include_time: bool,
) -> crate::lookup_context::AbbreviatedMetaProjection {
    // Each version is read through `dist`, which decodes only that field,
    // so the versions' dependency maps are never built.
    let dists: Vec<(&String, PackageDistribution)> = meta.versions
        .keys()
        .filter_map(|version| Some((version, meta.versions.dist(version)?)))
        .collect();
    let version_artifacts = dists
        .iter()
        .map(|(version, dist)| ((*version).clone(), project_artifact_history(dist)))
        .collect();
    let version_dist_stats = dists
        .iter()
        .filter_map(|(version, dist)| {
            let stats =
                DistStats { unpacked_size: dist.unpacked_size, file_count: dist.file_count };
            (stats.unpacked_size.is_some() || stats.file_count.is_some()).then(|| {
                ((*version).clone(), stats)
            })
        })
        .collect();
    // `time` also carries package-level `created`/`modified` keys; keeping
    // them is harmless (lookups are by exact version) and cheaper than
    // filtering against the versions map.
    let version_time = include_time.then(|| {
        meta.time
            .iter()
            .flatten()
            .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
            .collect()
    });
    crate::lookup_context::AbbreviatedMetaProjection {
        modified: meta.modified.clone(),
        version_artifacts: Some(version_artifacts),
        version_dist_stats: Some(version_dist_stats),
        version_time,
    }
}

/// The per-version publish times of the scoped on-disk mirror, keyed by
/// version. `None` without a mirror or a `time` payload.
pub(super) async fn load_local_meta_time(
    cache_dir: &std::path::Path,
    scope: &pnpm_network::MetadataCacheScope,
    registry: &str,
    name: &str,
) -> Option<Arc<PublishedAtTimeMap>> {
    let meta_dir = crate::mirror::scoped_meta_dir(scope, crate::mirror::FULL_META_DIR);
    let mirror_path = crate::mirror::get_pkg_mirror_path(cache_dir, &meta_dir, registry, name).ok();
    let pkg = crate::mirror::load_meta_async(mirror_path.as_deref()).await?;
    let raw = pkg.time.as_ref()?;
    raw.iter()
        .filter_map(|(version, value)| {
            value
                .as_str()
                .map(|ts| (version.clone(), ts.to_string()))
        })
        .collect::<PublishedAtTimeMap>()
        .pipe(Arc::new)
        .pipe(Some)
}

pub(super) fn project_artifact_history(
    dist: &pnpm_registry::PackageDistribution,
) -> crate::lookup_context::RegistryArtifactHistory {
    let revisions = dist.revisions
        .as_ref()
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .map(|revision| crate::lookup_context::RegistryArtifact {
            revision: revision.get("revision").cloned(),
            integrity: revision
                .get("integrity")
                .and_then(JsonValue::as_str)
                .and_then(|integrity| integrity.parse().ok()),
            tarball: revision
                .get("tarball")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
        })
        .collect();
    crate::lookup_context::RegistryArtifactHistory {
        current: crate::lookup_context::RegistryArtifact {
            revision: dist.revision.clone(),
            integrity: dist.integrity.clone(),
            tarball: Some(dist.tarball.clone()),
        },
        revisions,
    }
}
