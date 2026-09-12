use super::{
    AppState, CanonicalPackageName, DistBlock, Ecosystem, FetchOutcome, Identity, IndexMap,
    Integrity, MAX_TARBALL_BYTES, Registry, RegistryError, Response, RevisionScan, RevisionSource,
    Storage, TarballRevision, Value, authorized_revision_upstream, ecosystem::hosted_sources,
    hosted_revision_refs, integrity_addressed_tarball_integrity, integrity_addressed_tarball_path,
    not_found, private_no_cache, revision_registry_is_private, revision_tarball_response,
    serve_private_revision_refs, serve_revision_refs, streaming, tarball_integrity_error,
    tarball_stream_error_for_package, upstream_cache_namespace,
};
use axum::response::IntoResponse;

pub(super) async fn serve_revision_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    digest: &str,
) -> Response {
    let Some(integrity) = integrity_addressed_tarball_integrity(digest) else {
        return not_found();
    };
    if matches!(state.inner.config.registries.get(registry), Some(Registry::Upstream { .. })) {
        let response =
            serve_upstream_revision_tarball(state, identity, registry, digest, &integrity).await;
        return if revision_registry_is_private(state, registry) {
            private_no_cache(response)
        } else {
            response
        };
    }
    serve_hosted_revision_tarball(state, identity, registry, digest, &integrity).await
}

pub(super) async fn serve_upstream_revision_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let upstream = match authorized_revision_upstream(state, identity, registry) {
        Ok(upstream) => upstream,
        Err(err) => return err.into_response(),
    };
    let namespace = upstream_cache_namespace(state, registry);
    if upstream.caches()
        && let Some(response) =
            cached_revision_tarball(state, &namespace, registry, digest, integrity).await
    {
        return response;
    }
    let response = match upstream.fetch_revision_tarball_response(digest).await {
        Ok(FetchOutcome::Ok(response)) => response,
        Ok(FetchOutcome::NotFound) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let write = match state.inner.storage.open_upstream_revision_blob_tmp(&namespace, digest).await
    {
        Ok(write) => write,
        Err(err) => return err.into_response(),
    };
    if !upstream.caches() {
        return uncached_revision_tarball(response, write, digest, integrity).await;
    }
    match streaming::stream_verified_to_cache(response, write, integrity, MAX_TARBALL_BYTES) {
        Ok(body) => revision_tarball_response(body, None, digest, integrity),
        Err(err) => {
            tarball_stream_error_for_package(err, "registry revision", digest).into_response()
        }
    }
}

pub(super) async fn serve_hosted_revision_tarball(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let sources = hosted_sources(state, registry, Ecosystem::Npm);
    if sources.is_empty() {
        return not_found();
    }

    let mut scan = RevisionScan::default();
    for source in sources {
        let Some(hosted) = state.inner.config.hosted.get(&source) else {
            continue;
        };
        let storage = state.inner.storage.for_hosted(&hosted.org);
        let refs = match hosted_revision_refs(&storage, digest).await {
            Ok(refs) => refs,
            Err(err) => return private_no_cache(err.into_response()),
        };
        let served = serve_revision_refs(
            state,
            identity,
            RevisionSource { registry, source: &source, storage: &storage, digest, integrity },
            refs,
            &mut scan,
        )
        .await;
        if let Some(response) = served {
            return response;
        }
    }

    serve_private_revision_refs(scan, digest, integrity).await
}

pub(super) async fn hosted_original_is_current(
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
    digest: &str,
) -> Result<bool, RegistryError> {
    let Some(bytes) = storage.read_hosted_document(package).await? else {
        return Ok(false);
    };
    let packument = serde_json::from_slice::<HostedRevisionPackument>(&bytes)?;
    Ok(packument
        .versions
        .get(version)
        .and_then(|manifest| manifest.dist.as_ref())
        .and_then(original_integrity)
        .and_then(|integrity| integrity_addressed_tarball_path(&integrity))
        .is_some_and(|path| path == format!("-/tarballs/sha512/{digest}")))
}

pub(super) async fn open_hosted_revision_tarball(
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    let filename = package.tarball_name_for_version(version);
    match storage.open_hosted_blob(package, &filename).await {
        Ok(Some((body, len))) => revision_tarball_response(body, len, digest, integrity),
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(super) struct HostedRevisionPackument {
    #[serde(default)]
    pub(super) versions: IndexMap<String, HostedRevisionManifest>,
}

#[derive(serde::Deserialize)]
pub(super) struct HostedRevisionManifest {
    #[serde(default)]
    pub(super) dist: Option<HostedRevisionDist>,
}

#[derive(serde::Deserialize)]
pub(super) struct HostedRevisionDist {
    #[serde(default)]
    pub(super) integrity: Option<String>,
    #[serde(default)]
    pub(super) revision: RevisionField,
    #[serde(default)]
    pub(super) revisions: Vec<HostedRevisionRecord>,
}

#[derive(serde::Deserialize)]
pub(super) struct HostedRevisionRecord {
    #[serde(default)]
    pub(super) revision: Value,
    #[serde(default)]
    pub(super) integrity: Option<String>,
}

#[derive(Default)]
pub(super) enum RevisionField {
    #[default]
    Missing,
    Present(Value),
}

impl<'de> serde::Deserialize<'de> for RevisionField {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        <Value as serde::Deserialize>::deserialize(deserializer).map(Self::Present)
    }
}

pub(super) fn original_integrity(dist: &HostedRevisionDist) -> Option<Integrity> {
    let RevisionField::Present(revision) = &dist.revision else {
        return dist.integrity.as_deref()?.parse().ok();
    };
    let selected_revision =
        revision.as_u64().and_then(|revision| TarballRevision::try_from(revision).ok())?.get();
    let selected: Vec<_> = dist
        .revisions
        .iter()
        .filter(|record| record.revision.as_u64() == Some(selected_revision))
        .collect();
    if selected.len() != 1 || selected[0].integrity.as_deref() != dist.integrity.as_deref() {
        return None;
    }
    let originals: Vec<_> =
        dist.revisions.iter().filter(|record| record.revision.as_u64() == Some(0)).collect();
    if originals.len() != 1 {
        return None;
    }
    originals[0].integrity.as_deref()?.parse().ok()
}

/// Prefer SRI and fall back to legacy SHA-1, while refusing unverified bytes.
pub(super) fn declared_tarball_integrity(
    dist: &DistBlock,
    name: &CanonicalPackageName,
    filename: &str,
    version: &str,
) -> Result<Integrity, RegistryError> {
    let integrity = if let Some(declared) = dist.integrity.as_deref() {
        streaming::parse_integrity(declared).map_err(|err| {
            tarball_integrity_error(
                name.as_str(),
                filename,
                format!("malformed dist.integrity: {err}"),
            )
        })?
    } else {
        let shasum = dist.shasum.as_deref().ok_or_else(|| {
            tarball_integrity_error(
                name.as_str(),
                filename,
                format!("packument has no dist.integrity or dist.shasum for {version:?}"),
            )
        })?;
        Integrity::from_hex(shasum, ssri::Algorithm::Sha1).map_err(|err| {
            tarball_integrity_error(
                name.as_str(),
                filename,
                format!("malformed dist.shasum: {err}"),
            )
        })?
    };
    Ok(integrity)
}

pub(super) async fn cached_revision_tarball(
    state: &AppState,
    namespace: &str,
    registry: &str,
    digest: &str,
    integrity: &Integrity,
) -> Option<Response> {
    match state.inner.storage.open_upstream_revision_blob(namespace, digest).await {
        Ok(Some((file, len))) => {
            return Some(revision_tarball_response(
                streaming::stream_file(file),
                Some(len),
                digest,
                integrity,
            ));
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(?err, %registry, %digest, "revision tarball cache open failed");
        }
    }
    None
}

pub(super) async fn uncached_revision_tarball(
    response: pnpm_network::ThrottledResponse,
    write: pnpr_storage::BlobWrite,
    digest: &str,
    integrity: &Integrity,
) -> Response {
    match streaming::download_verified_to_temp(response, write, integrity, MAX_TARBALL_BYTES).await
    {
        Ok((file, len, tmp_path)) => revision_tarball_response(
            streaming::stream_file_and_remove(file, tmp_path),
            Some(len),
            digest,
            integrity,
        ),
        Err(err) => {
            tarball_stream_error_for_package(err, "registry revision", digest).into_response()
        }
    }
}
