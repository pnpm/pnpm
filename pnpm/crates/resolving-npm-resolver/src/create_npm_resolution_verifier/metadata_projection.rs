use super::{
    Approver, Arc, DerivedPackuments, DistStats, HashMap, JsonValue, NpmUser, Package,
    PackageDistribution, PackageVersion, Pipe, PublishedAtTimeMap,
};

/// Build a [`Package`] that retains only the fields
/// [`fail_if_trust_downgraded`] reads: the package name, the per-version
/// `time` map, and per-version trust evidence (`_npmUser.approver`,
/// `_npmUser.trustedPublisher`, and `dist.attestations.provenance`).
/// Drops everything else — dependency
/// graphs, scripts, READMEs — so the per-install trust-meta cache stays
/// bounded by the trust-evidence footprint, not the full packument size.
///
/// [`fail_if_trust_downgraded`]: crate::trust_checks::fail_if_trust_downgraded
pub(super) fn project_trust_meta(meta: &Package) -> Package {
    // Borrowed `meta` so the shared-cache fast path (which only holds
    // `Arc<Package>`) doesn't pay for a full deep-clone of the
    // packument it's about to discard. Only the fields downstream
    // reads are cloned out; the bulk of the document (per-version
    // dependency maps, scripts, README) drops on the original.
    let versions = meta
        .versions
        .iter()
        .map(|(version, manifest)| (version.clone(), project_trust_package_version(&manifest)))
        .collect();
    Package {
        name: meta.name.clone(),
        dist_tags: std::collections::HashMap::new(),
        versions,
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

pub(super) fn project_trust_package_version(version: &PackageVersion) -> PackageVersion {
    let attestations =
        version.dist.attestations.as_ref().and_then(|att| att.provenance.as_ref()).map(|prov| {
            pnpm_registry::AttestationsDist { provenance: Some(prov.clone()), url: None }
        });
    // `get_trust_evidence` only reads `npm_user.approver` (presence) and
    // `npm_user.trusted_publisher`; drop the maintainer `name` / `email`
    // PII — including the approver's — so the projected cache entry
    // doesn't hold per-version publisher metadata that downstream
    // doesn't need.
    let approver = version.npm_user.as_ref().and_then(|user| user.approver.as_ref());
    let trusted_publisher =
        version.npm_user.as_ref().and_then(|user| user.trusted_publisher.as_ref());
    let npm_user = (approver.is_some() || trusted_publisher.is_some()).then(|| NpmUser {
        name: None,
        email: None,
        approver: approver.map(|_| Approver { name: None, email: None }),
        trusted_publisher: trusted_publisher.cloned(),
    });
    PackageVersion {
        // `fail_if_trust_downgraded` keys off the outer `meta.versions`
        // map and the version-level npm_user / attestations fields. The
        // per-version `name`, `version`, and `dist` non-attestation fields
        // are never read, so empty placeholders are fine — clone of the
        // parsed semver keeps the typed shape valid without paying for
        // the registry packument's dependency graph.
        name: String::new(),
        version: version.version.clone(),
        dist: PackageDistribution {
            integrity: None,
            shasum: None,
            tarball: String::new(),
            revision: None,
            revisions: None,
            file_count: None,
            unpacked_size: None,
            attestations,
        },
        dependencies: None,
        dev_dependencies: None,
        peer_dependencies: None,
        optional_dependencies: None,
        peer_dependencies_meta: None,
        npm_user,
        deprecated: None,
        other: HashMap::new(),
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
    let version_artifacts = meta
        .versions
        .iter()
        .map(|(version, manifest)| (version.clone(), project_artifact_history(&manifest.dist)))
        .collect();
    let version_dist_stats = meta
        .versions
        .iter()
        .filter_map(|(version, manifest)| {
            let stats = DistStats {
                unpacked_size: manifest.dist.unpacked_size,
                file_count: manifest.dist.file_count,
            };
            (stats.unpacked_size.is_some() || stats.file_count.is_some())
                .then(|| (version.clone(), stats))
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
        .filter_map(|(version, value)| value.as_str().map(|ts| (version.clone(), ts.to_string())))
        .collect::<PublishedAtTimeMap>()
        .pipe(Arc::new)
        .pipe(Some)
}

pub(super) fn project_artifact_history(
    dist: &pnpm_registry::PackageDistribution,
) -> crate::lookup_context::RegistryArtifactHistory {
    let recorded = dist.revisions.as_ref().and_then(JsonValue::as_array).into_iter().flatten();
    let revisions = recorded
        .map(|revision| crate::lookup_context::RegistryArtifact {
            revision: revision.get("revision").cloned(),
            integrity: revision
                .get("integrity")
                .and_then(JsonValue::as_str)
                .and_then(|integrity| integrity.parse().ok()),
            tarball: revision.get("tarball").and_then(JsonValue::as_str).map(str::to_string),
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
