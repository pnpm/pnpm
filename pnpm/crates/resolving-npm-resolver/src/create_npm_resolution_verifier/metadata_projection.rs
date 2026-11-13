use super::{
    Arc, DistStats, HashMap, JsonValue, Package, PublishedAtTimeMap, TrustEvidence, TrustHistory,
    TrustHistoryProjection,
};
use crate::{
    attested_time::{ATTESTED_TIME_DIR, load_attested_times},
    trust_checks::get_policy_trust_evidence,
};

/// Project a packument to what [`fail_if_trust_downgraded`] reads from it.
///
/// The walk leaves `meta`'s hydration cache alone: `meta` may be the
/// resolver's shared packument, which lives for the whole install.
///
/// [`fail_if_trust_downgraded`]: crate::trust_checks::fail_if_trust_downgraded
pub(super) fn project_trust_meta(meta: &Package) -> TrustHistoryProjection {
    let time = meta.time
        .as_ref()
        .map(|time| {
            time.iter()
                .filter_map(|(version, value)| Some((version.clone(), value.as_str()?.to_owned())))
                .collect()
        });
    let evidence = meta.versions
        .iter_policy_fields()
        .map(|(version, fields)| (version.clone(), get_policy_trust_evidence(&fields)))
        .collect();
    TrustHistoryProjection { name: meta.name.clone(), time, evidence }
}

impl TrustHistory for TrustHistoryProjection {
    fn package_name(&self) -> &str {
        &self.name
    }

    fn has_publish_times(&self) -> bool {
        self.time.is_some()
    }

    fn published_at(&self, version: &str) -> Option<&str> {
        self.time
            .as_ref()?
            .get(version)
            .map(String::as_str)
    }

    fn versions(&self) -> impl Iterator<Item = &str> {
        self.evidence.iter().map(|(version, _)| version.as_str())
    }

    fn version_evidence(&self, version: &str) -> Option<Option<TrustEvidence>> {
        let index = self.evidence
            .binary_search_by(|(candidate, _)| candidate.as_str().cmp(version))
            .ok()?;
        Some(self.evidence[index].1)
    }

    fn prior_evidence(&self, version: &str) -> Option<Option<TrustEvidence>> {
        self.version_evidence(version)
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
    // One uncached pass: `meta` may be the resolver's shared packument.
    let mut version_artifacts = HashMap::new();
    let mut version_dist_stats = HashMap::new();
    for (version, fields) in meta.versions.iter_policy_fields() {
        let stats = DistStats {
            unpacked_size: fields.dist.unpacked_size,
            file_count: fields.dist.file_count,
            requires_build: fields.dist.pnpr.as_ref().and_then(|pnpr| pnpr.requires_build),
        };
        if stats.unpacked_size.is_some()
            || stats.file_count.is_some()
            || stats.requires_build.is_some()
        {
            version_dist_stats.insert(version.clone(), stats);
        }
        version_artifacts.insert(version.clone(), project_artifact_history(&fields.dist));
    }
    // `time` also carries package-level `created`/`modified` keys; keeping
    // them is harmless (lookups are by exact version) and cheaper than
    // filtering against the versions map.
    let version_time = meta.time
        .as_ref()
        .filter(|_| include_time)
        .map(|time| {
            time.iter()
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

/// The per-version publish times the scoped cache holds, keyed by version:
/// those of the full mirror, and the [attested] ones for the versions it
/// does not date. `None` when the cache holds neither.
///
/// [attested]: crate::attested_time
pub(super) async fn load_local_meta_time(
    cache_dir: &std::path::Path,
    scope: &pnpm_network::MetadataCacheScope,
    registry: &str,
    name: &str,
) -> Option<Arc<PublishedAtTimeMap>> {
    let mut times = match attested_time_path(cache_dir, scope, registry, name) {
        Some(path) => tokio::task::spawn_blocking(move || load_attested_times(&path))
            .await
            .unwrap_or_default(),
        None => PublishedAtTimeMap::new(),
    };
    let meta_dir = crate::mirror::scoped_meta_dir(scope, crate::mirror::FULL_META_DIR);
    let mirror_path = crate::mirror::get_pkg_mirror_path(cache_dir, &meta_dir, registry, name).ok();
    let mirrored = crate::mirror::load_meta_async(mirror_path.as_deref()).await;
    if let Some(raw) = mirrored.as_ref().and_then(|pkg| pkg.time.as_ref()) {
        times.extend(
            raw.iter()
                .filter_map(|(version, value)| {
                    Some((version.clone(), value.as_str()?.to_string()))
                }),
        );
    }
    (!times.is_empty()).then(|| Arc::new(times))
}

/// Where the scoped cache keeps the [attested publish times] of `name`.
///
/// [attested publish times]: crate::attested_time
pub(super) fn attested_time_path(
    cache_dir: &std::path::Path,
    scope: &pnpm_network::MetadataCacheScope,
    registry: &str,
    name: &str,
) -> Option<std::path::PathBuf> {
    let meta_dir = crate::mirror::scoped_meta_dir(scope, ATTESTED_TIME_DIR);
    crate::mirror::get_pkg_mirror_path(cache_dir, &meta_dir, registry, name).ok()
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
            tarball: (!dist.tarball.is_empty()).then(|| dist.tarball.clone()),
        },
        revisions,
        install_v2: dist.install_v2_entry(),
    }
}
