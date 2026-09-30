use super::{
    AppState, CanonicalPackageName, FetchOutcome, Integrity, MAX_TARBALL_BYTES, RegistryError,
    Upstream, is_osv_vulnerable_packument_version, streaming, tarball_integrity_error,
    tarball_stream_error_for_package, timed, upstream_packuments::lock_upstream_package,
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

/// A downloaded tarball and its computed integrity, not yet promoted into the
/// cache nor written into the packument.
struct StagedPin {
    tarball: MissingIntegrityTarball,
    integrity: Integrity,
    blob: pnpr_storage::SealedBlob,
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
}

/// Pins a computed `dist.integrity` into every version of a packument pnpr
/// just fetched and cached that lacks a usable one, so a client that requires
/// integrity can install it. Reads of the cached copy do not retry versions that could not be
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
    bytes: Vec<u8>,
) -> Result<Vec<u8>, RegistryError> {
    PackumentIntegrityResolver { state, namespace, upstream, name }.complete_missing(bytes).await
}

impl PackumentIntegrityResolver<'_> {
    /// The downloads run without the package lock, which is striped and shared
    /// with hosted publishes. Writing the pins into the cached packument, which
    /// a concurrent refresh may have replaced meanwhile, and promoting their
    /// tarballs runs under it, so a published pin always describes the cached
    /// bytes.
    async fn complete_missing(&self, bytes: Vec<u8>) -> Result<Vec<u8>, RegistryError> {
        let candidates = {
            let doc: Value = serde_json::from_slice(&bytes)?;
            missing_integrity_tarballs(&doc, self.name, self.osv_index())
        };
        let pins = self.compute_integrities(candidates).await;
        if pins.is_empty() {
            return Ok(bytes);
        }
        let _guard = lock_upstream_package(self.state, self.namespace, self.name).await;
        let Some(doc) = self.read_cached_packument().await? else {
            abandon(pins).await;
            return Ok(bytes);
        };
        Ok(self.publish_pins(doc, pins).await.unwrap_or(bytes))
    }

    /// Writes the pins that still apply into `doc` and persists it before
    /// promoting their tarballs, so the cache never holds a tarball whose pin
    /// was not published. Returns the persisted document, or `None` when
    /// nothing was published.
    async fn publish_pins(&self, mut doc: Value, pins: Vec<StagedPin>) -> Option<Vec<u8>> {
        let mut publishable = Vec::new();
        let mut stale = Vec::new();
        for pin in pins {
            if pin_still_applies(&doc, self.name, &pin.tarball) {
                set_integrity(&mut doc, &pin.tarball.version, &pin.integrity);
                publishable.push(pin);
            } else {
                stale.push(pin);
            }
        }
        abandon(stale).await;
        if publishable.is_empty() {
            return None;
        }
        let persisted = match self.persist_packument(&doc).await {
            Ok(persisted) => persisted,
            Err(err) => {
                tracing::warn!(?err, package = %self.name.as_str(), "pinned packument cache write failed");
                abandon(publishable).await;
                return None;
            }
        };
        for pin in publishable {
            self.promote(pin).await;
        }
        Some(persisted)
    }

    /// A pin whose tarball fails to promote stays published, and any older
    /// tarball cached under that name is evicted, so the tarball route fetches
    /// the version again and verifies it against the pin.
    async fn promote(&self, pin: StagedPin) {
        let Err(err) = pin.blob.promote().await else {
            return;
        };
        tracing::warn!(
            ?err,
            package = %self.name.as_str(),
            version = %pin.tarball.version,
            "pinned tarball cache promotion failed",
        );
        let storage = &self.state.inner.storage;
        if let Err(err) =
            storage.remove_upstream_blob(self.namespace, self.name, &pin.tarball.filename).await
        {
            tracing::warn!(
                ?err,
                package = %self.name.as_str(),
                version = %pin.tarball.version,
                "failed to evict the tarball cached before the pin",
            );
        }
    }

    fn osv_index(&self) -> Option<&std::sync::Arc<pnpr_osv::OsvIndex>> {
        self.state.inner.osv_index.as_ref()
    }

    /// Any cached copy, however old, since the downloads may outlast a short
    /// `maxage`. `None` means there is nothing to pin into: the package was
    /// purged meanwhile, or its cached body does not parse.
    async fn read_cached_packument(&self) -> Result<Option<Value>, RegistryError> {
        let cached =
            self.state.inner.storage.read_upstream_document_any(self.namespace, self.name).await?;
        Ok(cached.and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }

    async fn compute_integrities(
        &self,
        candidates: Vec<MissingIntegrityTarball>,
    ) -> Vec<StagedPin> {
        let mut pins = Vec::new();
        let mut candidates = candidates.into_iter();
        for candidate in candidates.by_ref().take(MAX_PINNED_VERSIONS_PER_PACKUMENT) {
            match self.stage_pin(&candidate).await {
                Ok((blob, integrity)) => {
                    pins.push(StagedPin { tarball: candidate, integrity, blob });
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
        pins
    }

    async fn stage_pin(
        &self,
        candidate: &MissingIntegrityTarball,
    ) -> Result<(pnpr_storage::SealedBlob, Integrity), RegistryError> {
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
        let download = streaming::download_computing_sha512(
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

    async fn persist_packument(&self, doc: &Value) -> Result<Vec<u8>, RegistryError> {
        let bytes = serde_json::to_vec(doc)?;
        self.state.inner.storage.write_upstream_document(self.namespace, self.name, &bytes).await?;
        Ok(bytes)
    }
}

async fn abandon(pins: Vec<StagedPin>) {
    for pin in pins {
        pin.blob.abandon().await;
    }
}

/// Whether `doc` still declares `pinned`'s tarball, with the same basename
/// and legacy shasum, and still without a usable integrity.
fn pin_still_applies(
    doc: &Value,
    name: &CanonicalPackageName,
    pinned: &MissingIntegrityTarball,
) -> bool {
    let Some(manifest) = doc
        .get("versions")
        .and_then(|versions| versions.get(&pinned.version))
    else {
        return false;
    };
    matches!(
        packument_tarball(manifest, name, &pinned.version),
        Ok(Some(current)) if current.needs_integrity
            && current.filename == pinned.filename
            && current.expected_shasum == pinned.expected_shasum,
    )
}

fn set_integrity(doc: &mut Value, version: &str, integrity: &Integrity) {
    let dist = doc
        .get_mut("versions")
        .and_then(|versions| versions.get_mut(version))
        .and_then(|manifest| manifest.get_mut("dist"))
        .and_then(Value::as_object_mut)
        .expect("pin_still_applies found the version's dist");
    dist.insert("integrity".to_string(), Value::String(integrity.to_string()));
}

/// A version whose dist pnpr cannot classify, or whose tarball basename another
/// version also declares, is left out rather than failing the document: the
/// tarball route refuses it on its own.
fn missing_integrity_tarballs(
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
        .filter(|tarball| tarball.needs_integrity && !shared.contains(&tarball.filename))
        .collect()
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

#[cfg(test)]
mod tests;
