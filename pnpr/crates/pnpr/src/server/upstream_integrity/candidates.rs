use super::{
    CanonicalPackageName,
    Integrity,
    RegistryError,
    UpstreamTarballRoute,
    is_osv_vulnerable_packument_version,
    tarball_cache_name,
    tarball_integrity_error,
};
use serde_json::Value;
use std::collections::HashSet;

pub(super) struct MissingIntegrityTarball {
    pub(super) version: String,
    pub(super) filename: String,
    pub(super) url: String,
    pub(super) route: UpstreamTarballRoute,
    pub(super) needs_integrity: bool,
    pub(super) expected_shasum: Option<Integrity>,
}

/// A version whose dist pnpr cannot classify, or whose tarball basename another
/// version also declares, is left out rather than failing the document: the
/// tarball route refuses it on its own.
pub(super) fn missing_integrity_tarballs(
    doc: &Value,
    name: &CanonicalPackageName,
    osv_index: Option<&std::sync::Arc<pnpr_osv::OsvIndex>>,
) -> Vec<MissingIntegrityTarball> {
    let Some(versions) = doc.get("versions").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut tarballs = Vec::new();
    let mut unclassified = 0usize;
    for (version, manifest) in versions {
        let screened = osv_index.is_some_and(|index| {
            is_osv_vulnerable_packument_version(doc, name.as_str(), version, index)
        });
        if screened {
            continue;
        }
        match packument_tarball(manifest, name, version) {
            Ok(Some(tarball)) => tarballs.push(tarball),
            Ok(None) => {}
            Err(_) => unclassified += 1,
        }
    }
    if unclassified > 0 {
        tracing::warn!(
            package = %name.as_str(),
            unclassified,
            "leaving versions with an unusable dist unpinned",
        );
    }
    unambiguous_candidates(tarballs)
}

fn unambiguous_candidates(tarballs: Vec<MissingIntegrityTarball>) -> Vec<MissingIntegrityTarball> {
    let mut filenames = HashSet::new();
    let shared: HashSet<String> = tarballs
        .iter()
        .filter(|tarball| !filenames.insert(tarball.filename.as_str()))
        .map(|tarball| tarball.filename.clone())
        .collect();
    tarballs
        .into_iter()
        .filter(|tarball| tarball.needs_integrity)
        .filter(|tarball| !shared.contains(&tarball.filename))
        .collect()
}

pub(super) fn packument_tarball(
    manifest: &Value,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<Option<MissingIntegrityTarball>, RegistryError> {
    let Some(dist) = manifest.get("dist").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(tarball) = dist.get("tarball").and_then(Value::as_str) else {
        return Ok(None);
    };
    let filename = packument_tarball_filename(tarball, name, version)?;
    let route = UpstreamTarballRoute::from_dist_tarball(Some(tarball), name);
    let needs_integrity = needs_integrity(dist, name, &filename, version)?;
    let expected_shasum = if needs_integrity {
        legacy_shasum_integrity(dist, name, &filename, version)?
    } else {
        None
    };
    Ok(Some(MissingIntegrityTarball {
        version: version.to_string(),
        filename,
        url: tarball.to_string(),
        route,
        needs_integrity,
        expected_shasum,
    }))
}

pub(super) fn packument_tarball_filename(
    tarball: &str,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<String, RegistryError> {
    let basename = pnpr_upstream::tarball_basename_for_package(tarball, name);
    let fallback = name.tarball_name_for_version(version);
    let raw_filename = basename.as_deref().unwrap_or(&fallback);
    let (filename, _) = tarball_cache_name(name, raw_filename)
        .map_err(|err| {
            tarball_integrity_error(
                name.as_str(),
                raw_filename,
                format!("invalid tarball name for version {version:?}: {err}"),
            )
        })?;
    Ok(filename)
}

fn needs_integrity(
    dist: &serde_json::Map<String, Value>,
    name: &CanonicalPackageName,
    filename: &str,
    version: &str,
) -> Result<bool, RegistryError> {
    match dist.get("integrity") {
        Some(Value::String(value)) => {
            let parsed = value
                .parse::<ssri::Integrity>()
                .map_err(|err| {
                    tarball_integrity_error(
                        name.as_str(),
                        filename,
                        format!("malformed dist.integrity for version {version:?}: {err}"),
                    )
                })?;
            // An SRI that parses to no hashes pins nothing, which is the shape
            // a client reads as a missing integrity, so it is computed like an
            // absent one rather than refused.
            Ok(parsed.hashes.is_empty())
        }
        Some(Value::Null) | None => Ok(true),
        Some(_) => Err(tarball_integrity_error(
            name.as_str(),
            filename,
            format!("dist.integrity for version {version:?} is not a string"),
        )),
    }
}

fn legacy_shasum_integrity(
    dist: &serde_json::Map<String, Value>,
    name: &CanonicalPackageName,
    filename: &str,
    version: &str,
) -> Result<Option<Integrity>, RegistryError> {
    match dist.get("shasum") {
        Some(Value::String(shasum)) => Integrity::from_hex(shasum, ssri::Algorithm::Sha1)
            .map(Some)
            .map_err(|err| {
                tarball_integrity_error(
                    name.as_str(),
                    filename,
                    format!("malformed dist.shasum for version {version:?}: {err}"),
                )
            }),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(tarball_integrity_error(
            name.as_str(),
            filename,
            format!("dist.shasum for version {version:?} is not a string"),
        )),
    }
}
