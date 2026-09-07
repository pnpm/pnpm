use super::{
    DOCKER_CONTENT_DIGEST, Digest, ErrorCode, Manifest, Request, api_version, error,
    registry_error, server_error,
};
use crate::server::{
    RegistrySource,
    ecosystem::{addressed_registry, sha256_hex, sha256_integrity, upstream_for},
    resolve_ecosystem_source, tarball_response, tarball_stream_error,
};
use axum::{
    body::Body,
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt as _;
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::streaming;
use pnpr_upstream::FetchOutcome;
use std::time::Duration;

impl Request {
    pub(super) fn upstream_source(
        &self,
        name: &str,
    ) -> Option<(CanonicalPackageName, RegistrySource)> {
        let key = CanonicalPackageName::parse(name, Ecosystem::Oci).ok()?;
        let target = addressed_registry(&self.state, self.registry.as_deref())?;
        let source = resolve_ecosystem_source(&self.state, &target, Ecosystem::Oci, key.as_str());
        matches!(source, RegistrySource::Upstream(_)).then_some((key, source))
    }

    pub(super) async fn proxy_manifest(
        &self,
        key: &CanonicalPackageName,
        source: &RegistrySource,
        reference: &str,
    ) -> Response {
        let result = self.load_proxy_manifest(key, source, reference).await;
        let response = match result {
            Ok(Some(response)) => response,
            Ok(None) => error(ErrorCode::ManifestUnknown, "upstream manifest does not exist"),
            Err(err) => registry_error(err),
        };
        self.caller_scoped(Some(key.as_str()), api_version(response))
    }

    async fn load_proxy_manifest(
        &self,
        key: &CanonicalPackageName,
        source: &RegistrySource,
        reference: &str,
    ) -> Result<Option<Response>, RegistryError> {
        if Digest::parse(reference).is_err() && !pnpr_oci::is_valid_tag(reference) {
            return Err(RegistryError::BadRequest {
                reason: "invalid manifest reference".to_string(),
            });
        }
        let (upstream, namespace) = upstream_for(&self.state, &self.identity, source, key)?;
        let namespace = format!("{namespace}-oci-manifest-{}", sha256_hex(reference.as_bytes()));
        let storage = &self.state.inner.storage;
        let ttl = if Digest::parse(reference).is_ok() {
            Duration::MAX
        } else {
            upstream.maxage().unwrap_or(self.state.inner.config.packument_ttl)
        };
        if upstream.caches()
            && let Some(bytes) = storage.read_upstream_document(&namespace, key, ttl).await?
        {
            return manifest_response(bytes, self.method == Method::HEAD).map(Some);
        }
        if self.method == Method::HEAD {
            let fetched = upstream
                .head_oci(
                    key.as_str(),
                    &format!("manifests/{reference}"),
                    &pnpr_oci::MANIFEST_MEDIA_TYPES.join(", "),
                )
                .await?;
            return match fetched {
                FetchOutcome::NotFound => Ok(None),
                FetchOutcome::Ok(upstream_response) => {
                    if let Ok(expected) = Digest::parse(reference) {
                        let declared = upstream_response
                            .headers()
                            .get(DOCKER_CONTENT_DIGEST)
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| Digest::parse(value).ok());
                        if declared.as_ref() != Some(&expected) {
                            return Err(RegistryError::BadRequest {
                                reason: "upstream manifest digest mismatch".to_string(),
                            });
                        }
                    }
                    let mut response = Response::new(Body::empty());
                    for name in [
                        header::CONTENT_TYPE,
                        header::CONTENT_LENGTH,
                        header::HeaderName::from_static(DOCKER_CONTENT_DIGEST),
                    ] {
                        if let Some(value) = upstream_response.headers().get(&name) {
                            response.headers_mut().insert(name, value.clone());
                        }
                    }
                    Ok(Some(response))
                }
            };
        }
        let fetched = upstream
            .fetch_oci(
                key.as_str(),
                &format!("manifests/{reference}"),
                &pnpr_oci::MANIFEST_MEDIA_TYPES.join(", "),
            )
            .await?;
        let response = match fetched {
            FetchOutcome::NotFound => return Ok(None),
            FetchOutcome::Ok(response) => response,
        };
        let declared = response
            .headers()
            .get(DOCKER_CONTENT_DIGEST)
            .map(|value| value.to_str().unwrap_or_default().to_string());
        let limit = self.state.inner.config.oci.max_manifest_bytes;
        let mut bytes = Vec::new();
        let mut stream = Box::pin(response.bytes_stream());
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return Err(RegistryError::BadRequest {
                    reason: "upstream manifest is too large".to_string(),
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        let digest = Digest::of(&bytes);
        if Digest::parse(reference).is_ok_and(|expected| expected != digest)
            || declared
                .is_some_and(|declared| Digest::parse(&declared).ok().as_ref() != Some(&digest))
        {
            return Err(RegistryError::BadRequest {
                reason: "upstream manifest digest mismatch".to_string(),
            });
        }
        Manifest::parse(&bytes, None)
            .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })?;
        if upstream.caches() {
            storage.write_upstream_document(&namespace, key, &bytes).await?;
        }
        manifest_response(bytes, false).map(Some)
    }

    pub(super) async fn proxy_blob(
        &self,
        key: &CanonicalPackageName,
        source: &RegistrySource,
        digest: &Digest,
    ) -> Response {
        let (upstream, namespace) = match upstream_for(&self.state, &self.identity, source, key) {
            Ok(found) => found,
            Err(err) => return registry_error(err),
        };
        let namespace = format!("{namespace}-oci-blobs");
        let filename = digest.blob_filename();
        let storage = &self.state.inner.storage;
        let result = async {
            if upstream.caches()
                && let Some((file, len)) =
                    storage.open_upstream_blob(&namespace, key, &filename).await?
            {
                return Ok(tarball_response(
                    if self.method == Method::HEAD {
                        Body::empty()
                    } else {
                        streaming::stream_file(file)
                    },
                    Some(len),
                ));
            }
            if self.method == Method::HEAD {
                let fetched = upstream
                    .head_oci(key.as_str(), &format!("blobs/{digest}"), "application/octet-stream")
                    .await?;
                return Ok(match fetched {
                    FetchOutcome::NotFound => {
                        error(ErrorCode::BlobUnknown, "upstream blob does not exist")
                    }
                    FetchOutcome::Ok(response) => tarball_response(
                        Body::empty(),
                        response
                            .headers()
                            .get(header::CONTENT_LENGTH)
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.parse().ok()),
                    ),
                });
            }
            let fetched = upstream
                .fetch_oci(key.as_str(), &format!("blobs/{digest}"), "application/octet-stream")
                .await?;
            let response = match fetched {
                FetchOutcome::NotFound => {
                    return Ok(error(ErrorCode::BlobUnknown, "upstream blob does not exist"));
                }
                FetchOutcome::Ok(response) => response,
            };
            let write = storage.open_upstream_blob_tmp(&namespace, key, &filename).await?;
            let integrity = sha256_integrity(digest.hex()).expect("validated SHA-256 digest");
            let limit = self.state.inner.config.oci.max_blob_bytes;
            if !upstream.caches() {
                let (file, len, path) =
                    streaming::download_verified_to_temp(response, write, &integrity, limit)
                        .await
                        .map_err(|err| tarball_stream_error(err, key, &filename))?;
                return Ok(tarball_response(
                    streaming::stream_file_and_remove(file, path),
                    Some(len),
                ));
            }
            let body = streaming::stream_verified_to_cache(response, write, &integrity, limit)
                .map_err(|err| tarball_stream_error(err, key, &filename))?;
            Ok::<_, RegistryError>(tarball_response(body, None))
        }
        .await;
        let mut response = result.unwrap_or_else(IntoResponse::into_response);
        if response.status().is_success() {
            super::insert_header(&mut response, DOCKER_CONTENT_DIGEST, &digest.to_string());
        }
        self.caller_scoped(Some(key.as_str()), api_version(response))
    }
}

fn manifest_response(bytes: Vec<u8>, head: bool) -> Result<Response, RegistryError> {
    let manifest = Manifest::parse(&bytes, None)
        .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, manifest.media_type())
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(DOCKER_CONTENT_DIGEST, Digest::of(&bytes).to_string())
        .body(if head { Body::empty() } else { Body::from(bytes) })
        .unwrap_or_else(|_| server_error()))
}
