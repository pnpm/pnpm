pub(super) mod cached;

pub(super) use background::{
    BackgroundIntegrity,
    schedule,
    wait_for_pin,
};
pub(super) use refresh::{
    RefreshOutcome,
    refresh_cached_packument,
};

mod background;
mod candidates;
mod refresh;

use self::candidates::{
    MissingIntegrityTarball,
    missing_integrity_tarballs,
    packument_tarball,
    packument_tarball_filename,
};
use super::{
    AppState,
    CanonicalPackageName,
    FetchOutcome,
    Integrity,
    RegistryError,
    Upstream,
    UpstreamTarballRoute,
    is_osv_vulnerable_packument_version,
    streaming,
    tarball_integrity_error,
    tarball_stream_error_for_package,
    timed,
    upstream_packuments::lock_upstream_package,
    upstream_tarballs::tarball_cache_name,
};
use futures_util::{
    StreamExt,
    stream,
};
use serde_json::Value;

/// A computed integrity and an optional downloaded blob awaiting publication.
struct StagedPin {
    tarball: MissingIntegrityTarball,
    integrity: Integrity,
    blob: Option<pnpr_storage::SealedBlob>,
}

/// How many versions of one packument a single upstream fetch will download to
/// pin integrity. The candidate set is the upstream's to choose, so without a
/// cap one client request fans out into a tarball fetch per version it
/// declares unpinned.
const MAX_PINNED_VERSIONS_PER_PACKUMENT: usize = 64;
const MAX_CONCURRENT_PIN_DOWNLOADS: usize = 4;

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
        let name = self.name.clone();
        let osv_index = self.state.inner.osv_index.clone();
        let (bytes, candidates) = self.state.inner.packuments.workers.run(move || {
            let doc: Value = serde_json::from_slice(&bytes)?;
            let candidates = missing_integrity_tarballs(&doc, &name, osv_index.as_ref());
            Ok::<_, RegistryError>((bytes, candidates))
        })
        .await??;
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
        let persisted = match self.persist_packument(doc).await {
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
        let Some(blob) = pin.blob else {
            return;
        };
        let Err(err) = blob.promote().await else {
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

    /// Any cached copy, however old, since the downloads may outlast a short
    /// `packument_ttl`. `None` means there is nothing to pin into: the package was
    /// purged meanwhile, or its cached body does not parse.
    async fn read_cached_packument(&self) -> Result<Option<Value>, RegistryError> {
        let cached =
            self.state.inner.storage.read_upstream_document_any(self.namespace, self.name).await?;
        self.state.inner.packuments.workers.run(move || {
            cached.and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
        .await
    }

    async fn compute_integrities(
        &self,
        candidates: Vec<MissingIntegrityTarball>,
    ) -> Vec<StagedPin> {
        let beyond_cap = candidates.len().saturating_sub(MAX_PINNED_VERSIONS_PER_PACKUMENT);
        if beyond_cap > 0 {
            tracing::warn!(package = %self.name.as_str(), beyond_cap, "leaving versions past the cap unpinned");
        }
        stream::iter(candidates.into_iter().take(MAX_PINNED_VERSIONS_PER_PACKUMENT))
            .map(|candidate| async move {
                match self.stage_pin(&candidate).await {
                    Ok((blob, integrity)) => {
                        Some(StagedPin { tarball: candidate, integrity, blob })
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = %err.log_message(),
                            package = %self.name.as_str(),
                            version = %candidate.version,
                            "leaving the version unpinned",
                        );
                        None
                    }
                }
            })
            .buffer_unordered(MAX_CONCURRENT_PIN_DOWNLOADS)
            .filter_map(|pin| async { pin })
            .collect()
            .await
    }

    async fn stage_pin(
        &self,
        candidate: &MissingIntegrityTarball,
    ) -> Result<(Option<pnpr_storage::SealedBlob>, Integrity), RegistryError> {
        let _permit = self.state.inner.proxy.integrity_backfill.downloads.acquire().await.unwrap();
        if let Some(integrity) = self.cached_integrity(candidate).await? {
            return Ok((None, integrity));
        }
        self.download_pin(candidate).await
            .map(|(blob, integrity)| (Some(blob), integrity))
    }

    fn ensure_pin_route_allowed(
        &self,
        candidate: &MissingIntegrityTarball,
    ) -> Result<(), RegistryError> {
        if let UpstreamTarballRoute::Url(url) = &candidate.route
            && candidate.expected_shasum.is_none()
            && !self.upstream.is_same_origin(url)
        {
            return Err(tarball_integrity_error(
                self.name.as_str(),
                &candidate.filename,
                format!(
                    "off-origin dist.tarball for version {:?} has no declared shasum or integrity",
                    candidate.version,
                ),
            ));
        }
        Ok(())
    }

    async fn download_pin(
        &self,
        candidate: &MissingIntegrityTarball,
    ) -> Result<(pnpr_storage::SealedBlob, Integrity), RegistryError> {
        self.ensure_pin_route_allowed(candidate)?;
        let fetched = timed(
            "tarball:integrity_fetch",
            self.name.as_str(),
            self.upstream.fetch_declared_tarball_response(
                self.name,
                &candidate.filename,
                &candidate.route,
            ),
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
            self.state.max_tarball_bytes(),
        )
        .await;
        download.map_err(|err| {
            tarball_stream_error_for_package(err, self.name.as_str(), &candidate.filename)
        })
    }

    async fn persist_packument(&self, doc: Value) -> Result<Vec<u8>, RegistryError> {
        let bytes =
            self.state.inner.packuments.workers.run(move || serde_json::to_vec(&doc)).await??;
        self.state.inner.storage.update_upstream_document(self.namespace, self.name, &bytes).await?;
        Ok(bytes)
    }
}

async fn abandon(pins: Vec<StagedPin>) {
    for pin in pins {
        if let Some(blob) = pin.blob {
            blob.abandon().await;
        }
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
            && current.url == pinned.url
            && current.route == pinned.route
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

#[cfg(test)]
mod tests;
