use super::{AppState, CanonicalPackageName, ProjectFile, RegistryError, not_found, sha256_hex};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use pnpm_python_resolver::metadata_url;
use pnpr_pypi::{CoreMetadata, archive::MAX_METADATA_BYTES};
use pnpr_upstream::{FetchOutcome, Upstream};
use ssri::{Algorithm, Integrity};

pub(super) async fn serve(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    key: &CanonicalPackageName,
    entry: &ProjectFile,
    distribution_url: &url::Url,
) -> Response {
    let Some(metadata) = entry.metadata().filter(|metadata| metadata.is_available()) else {
        return not_found();
    };
    let url = metadata_url(distribution_url);
    let descriptor = serde_json::to_string(metadata).expect("metadata serializes");
    let namespace =
        format!("{namespace}-metadata-{}", sha256_hex(format!("{url}\n{descriptor}").as_bytes()));
    match load(state, upstream, &namespace, key, metadata, &url).await {
        Ok(Some(bytes)) => {
            ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], bytes).into_response()
        }
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

async fn load(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    key: &CanonicalPackageName,
    metadata: &CoreMetadata,
    url: &url::Url,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let storage = &state.inner.storage;
    let ttl = upstream.maxage().unwrap_or(state.inner.config.http.packument_ttl);
    if upstream.caches()
        && let Some(bytes) = storage.read_upstream_document(namespace, key, ttl).await?
    {
        return Ok(Some(bytes));
    }
    let FetchOutcome::Ok(document) =
        upstream.fetch_artifact_document(url.as_str(), None, MAX_METADATA_BYTES).await?
    else {
        return Ok(None);
    };
    if let CoreMetadata::Hashes(_) = metadata
        && !matches_metadata_digest(&document.bytes, metadata)
    {
        return Err(RegistryError::UpstreamResponse {
            url: url.to_string(),
            reason: "metadata has no matching supported digest".to_string(),
        });
    }
    if upstream.caches() {
        storage.write_upstream_document(namespace, key, &document.bytes).await?;
    }
    Ok(Some(document.bytes))
}

fn matches_metadata_digest(bytes: &[u8], metadata: &CoreMetadata) -> bool {
    metadata
        .digest()
        .is_some_and(|(algorithm, digest)| {
            algorithm
                .parse::<Algorithm>()
                .and_then(|algorithm| Integrity::from_hex(digest, algorithm))
                .is_ok_and(|integrity| integrity.check(bytes).is_ok())
        })
}
