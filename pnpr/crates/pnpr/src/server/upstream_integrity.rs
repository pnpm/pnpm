use super::{
    AppState, CanonicalPackageName, Duration, FetchOutcome, Integrity, MAX_TARBALL_BYTES,
    RegistryError, Upstream, is_osv_vulnerable_packument_version, streaming,
    tarball_integrity_error, tarball_stream_error_for_package, timed,
    upstream_tarballs::tarball_cache_name,
};
use serde_json::Value;
use std::collections::HashSet;

struct MissingIntegrityTarball {
    version: String,
    filename: String,
    needs_integrity: bool,
    expected_shasum: Option<Integrity>,
}

/// How many versions of one packument a single upstream fetch will download to
/// pin integrity. The candidate set is the upstream's to choose, so without a
/// cap one client request fans out into a tarball fetch per version it
/// declares unpinned.
const MAX_PINNED_VERSIONS_PER_PACKUMENT: usize = 64;

struct PackumentIntegrityResolver<'a> {
    state: &'a AppState,
    namespace: &'a str,
    upstream: &'a Upstream,
    name: &'a CanonicalPackageName,
    ttl: Duration,
}

/// Pins a computed `dist.integrity` into every version of a freshly fetched
/// packument that lacks a usable one, so a client that requires integrity can
/// install it. Reads of the cached copy do not retry versions that could not be
/// pinned; the next refresh does.
///
/// Pinning is per version and never fails the document. A version the upstream
/// cannot serve, or whose bytes do not match what it declared, keeps the
/// metadata the upstream published for it: a client still cannot install that
/// version, but it can install every other version of the package, which
/// withholding the whole packument would prevent. The tarball route verifies
/// those bytes against whatever the upstream did declare.
pub(super) async fn complete_missing_tarball_integrities(
    state: &AppState,
    namespace: &str,
    upstream: &Upstream,
    name: &CanonicalPackageName,
    ttl: Duration,
    bytes: Vec<u8>,
) -> Result<Vec<u8>, RegistryError> {
    PackumentIntegrityResolver { state, namespace, upstream, name, ttl }.complete_missing(bytes)
        .await
}

impl PackumentIntegrityResolver<'_> {
    async fn complete_missing(&self, bytes: Vec<u8>) -> Result<Vec<u8>, RegistryError> {
        if !self.upstream.caches() {
            return Ok(bytes);
        }
        if missing_integrity_tarballs(&bytes, self.name, self.osv_index())?.is_empty() {
            return Ok(bytes);
        }
        let _guard = self.package_lock().await;
        let bytes = self.read_current_packument(bytes).await?;
        let mut doc: Value = serde_json::from_slice(&bytes)?;
        let candidates = missing_integrity_tarballs_in_document(&doc, self.name, self.osv_index());
        if candidates.is_empty() {
            return Ok(bytes);
        }
        if !self.compute_integrities(&mut doc, candidates).await {
            return Ok(bytes);
        }
        self.persist_packument(doc).await
    }

    fn osv_index(&self) -> Option<&std::sync::Arc<pnpr_osv::OsvIndex>> {
        self.state.inner.osv_index.as_ref()
    }

    async fn package_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        let lock_key = format!("upstream:{}:{}", self.namespace, self.name.as_str());
        self.state.inner.locks.packages.lock(&lock_key).await
    }

    async fn read_current_packument(&self, fallback: Vec<u8>) -> Result<Vec<u8>, RegistryError> {
        Ok(self.state.inner.storage
            .read_upstream_document(self.namespace, self.name, self.ttl)
            .await?
            .unwrap_or(fallback))
    }

    /// Reports whether any candidate was pinned, which is also whether the
    /// document is worth writing back.
    async fn compute_integrities(
        &self,
        doc: &mut Value,
        candidates: Vec<MissingIntegrityTarball>,
    ) -> bool {
        let mut pinned = false;
        let mut candidates = candidates.into_iter();
        for candidate in candidates.by_ref().take(MAX_PINNED_VERSIONS_PER_PACKUMENT) {
            match self.compute_integrity(&candidate).await {
                Ok(integrity) => {
                    let dist = doc
                        .get_mut("versions")
                        .and_then(Value::as_object_mut)
                        .and_then(|versions| versions.get_mut(&candidate.version))
                        .and_then(|manifest| manifest.get_mut("dist"))
                        .and_then(Value::as_object_mut)
                        .expect("candidate came from a version dist object");
                    dist.insert("integrity".to_string(), Value::String(integrity.to_string()));
                    pinned = true;
                }
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        package = %self.name.as_str(),
                        version = %candidate.version,
                        "leaving the version unpinned",
                    );
                }
            }
        }
        let beyond_cap = candidates.count();
        if beyond_cap > 0 {
            tracing::warn!(
                package = %self.name.as_str(),
                beyond_cap,
                "leaving versions past the cap unpinned",
            );
        }
        pinned
    }

    async fn compute_integrity(
        &self,
        candidate: &MissingIntegrityTarball,
    ) -> Result<Integrity, RegistryError> {
        let fetched = timed(
            "tarball:integrity_fetch",
            self.name.as_str(),
            self.upstream.fetch_tarball_response(self.name, &candidate.filename),
        )
        .await?;
        let response = match fetched {
            FetchOutcome::Ok(response) => response,
            FetchOutcome::NotFound => {
                return Err(tarball_integrity_error(
                    self.name.as_str(),
                    &candidate.filename,
                    format!("upstream has no tarball for version {:?}", candidate.version),
                ));
            }
        };
        let write = self.state.inner.storage.open_upstream_blob_tmp(
            self.namespace,
            self.name,
            &candidate.filename,
        )
        .await?;
        let download = streaming::download_to_cache_computing_sha512(
            response,
            write,
            candidate.expected_shasum.as_ref(),
            MAX_TARBALL_BYTES,
        )
        .await;
        download.map_err(|err| {
            tarball_stream_error_for_package(err, self.name.as_str(), &candidate.filename)
        })
    }

    async fn persist_packument(&self, doc: Value) -> Result<Vec<u8>, RegistryError> {
        let bytes = serde_json::to_vec(&doc)?;
        self.state.inner.storage.write_upstream_document(self.namespace, self.name, &bytes).await?;
        Ok(bytes)
    }
}

fn missing_integrity_tarballs(
    bytes: &[u8],
    name: &CanonicalPackageName,
    osv_index: Option<&std::sync::Arc<pnpr_osv::OsvIndex>>,
) -> Result<Vec<MissingIntegrityTarball>, RegistryError> {
    let doc: Value = serde_json::from_slice(bytes)?;
    Ok(missing_integrity_tarballs_in_document(&doc, name, osv_index))
}

/// A version whose dist pnpr cannot classify, or whose tarball basename another
/// version also declares, is left out rather than failing the document: the
/// tarball route refuses it on its own.
fn missing_integrity_tarballs_in_document(
    doc: &Value,
    name: &CanonicalPackageName,
    osv_index: Option<&std::sync::Arc<pnpr_osv::OsvIndex>>,
) -> Vec<MissingIntegrityTarball> {
    let Some(versions) = doc.get("versions").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut filenames = HashSet::new();
    let mut duplicate_filenames = HashSet::new();
    let mut candidates = Vec::new();
    for (version, manifest) in versions {
        if osv_index.is_some_and(|index| {
            is_osv_vulnerable_packument_version(doc, name.as_str(), version, index)
        }) {
            continue;
        }
        let candidate = match packument_tarball(manifest, name, version) {
            Ok(Some(candidate)) => candidate,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(
                    ?err,
                    package = %name.as_str(),
                    %version,
                    "leaving the version unpinned",
                );
                continue;
            }
        };
        if !filenames.insert(candidate.filename.clone()) {
            duplicate_filenames.insert(candidate.filename.clone());
        }
        if candidate.needs_integrity {
            candidates.push(candidate);
        }
    }
    candidates.retain(|candidate| !duplicate_filenames.contains(&candidate.filename));
    candidates
}

fn packument_tarball(
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
    let needs_integrity = needs_integrity(dist, name, &filename, version)?;
    let expected_shasum = if needs_integrity {
        legacy_shasum_integrity(dist, name, &filename, version)?
    } else {
        None
    };
    Ok(Some(MissingIntegrityTarball {
        version: version.to_string(),
        filename,
        needs_integrity,
        expected_shasum,
    }))
}

fn packument_tarball_filename(
    tarball: &str,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<String, RegistryError> {
    let basename = pnpr_upstream::tarball_basename(tarball);
    let fallback = name.tarball_name_for_version(version);
    let raw_filename = basename.unwrap_or(&fallback);
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
