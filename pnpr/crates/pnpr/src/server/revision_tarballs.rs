//! The `-/tarballs/sha512/<digest>` route: a registry revision's tar, by the
//! digest of the tar.
//!
//! A hosted version serves the tar of its published `.tgz`. An upstream
//! revision is fetched by the same digest, gzip-encoded or as a plain tar,
//! and is cached or served only once the whole tar matched the digest.
//! Either is encoded for the request as a tar registry's tarball is (see
//! [`super::tar_responses`]).

use super::{
    AppState, CanonicalPackageName, DistBlock, FetchOutcome, HeaderMap, Identity, Integrity,
    MAX_TARBALL_BYTES, Registry, RegistryError, Response, RevisionScan, RevisionSource, Storage,
    TarSource, TgzTrust, Value, authorized_revision_upstream, header, hosted_revision_refs,
    hosted_revision_sources, integrity_addressed_tarball_integrity,
    integrity_addressed_tarball_path, not_found, private_no_cache, revision_registry_is_private,
    serve_private_revision_refs, serve_revision_refs, streaming, tar_decode_limit,
    tar_resource_integrity, tar_response, tarball_integrity_error, tarball_response,
    tarball_stream_error_for_package, tgz_integrity_key, upstream_cache_namespace,
};
use axum::{body::Body, response::IntoResponse};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

pub(super) async fn serve_revision_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: &str,
    digest: &str,
) -> Response {
    let Some(integrity) = integrity_addressed_tarball_integrity(digest) else {
        return not_found();
    };
    if matches!(
        state.inner.config.routing.registries.get(registry),
        Some(Registry::Upstream { .. }),
    ) {
        let request = UpstreamRevision { state, identity, headers, registry, digest };
        let response = request.serve(&integrity).await;
        return if revision_registry_is_private(state, registry) {
            private_no_cache(response)
        } else {
            response
        };
    }
    serve_hosted_revision_tarball(state, identity, headers, registry, digest).await
}

struct UpstreamRevision<'a> {
    state: &'a AppState,
    identity: &'a Identity,
    headers: &'a HeaderMap,
    registry: &'a str,
    digest: &'a str,
}

impl UpstreamRevision<'_> {
    async fn serve(&self, integrity: &Integrity) -> Response {
        let upstream = match authorized_revision_upstream(self.state, self.identity, self.registry)
        {
            Ok(upstream) => upstream,
            Err(err) => return err.into_response(),
        };
        let namespace = upstream_cache_namespace(self.state, self.registry);
        if upstream.caches()
            && let Some(response) = self.cached(&namespace).await
        {
            return response;
        }
        let response = match upstream.fetch_revision_tarball_response(self.digest).await {
            Ok(FetchOutcome::Ok(response)) => response,
            Ok(FetchOutcome::NotFound) => return not_found(),
            Err(err) => return err.into_response(),
        };
        let fetched = match self.download_checked(&namespace, response, integrity).await {
            Ok(fetched) => fetched,
            Err(err) => return err.into_response(),
        };
        if upstream.caches() {
            if let Err(err) = fetched.write.finalize().await {
                return RegistryError::Io(err).into_response();
            }
            return self.cached(&namespace).await.unwrap_or_else(not_found);
        }
        match fetched.write.into_temp_file().await {
            Ok((file, len, tmp_path)) => {
                let body = streaming::stream_file_and_remove(file, tmp_path);
                let resource = gzip_tar(fetched.gzip);
                revision_tarball_response(self.state, self.headers, body, Some(len), resource).await
            }
            Err(err) => RegistryError::Io(err).into_response(),
        }
    }

    /// Download the upstream's response into a temporary blob and check the
    /// tar it holds against `integrity`.
    async fn download_checked(
        &self,
        namespace: &str,
        response: pnpm_network::ThrottledResponse,
        integrity: &Integrity,
    ) -> Result<FetchedRevision, RegistryError> {
        let gzip =
            is_gzip_encoded(response.headers()).map_err(|reason| self.integrity_error(reason))?;
        let storage = &self.state.inner.storage;
        let mut write = storage.open_upstream_revision_blob_tmp(namespace, self.digest).await?;
        if let Err(err) =
            streaming::download_unverified(response, &mut write, MAX_TARBALL_BYTES).await
        {
            write.abandon().await;
            return Err(tarball_stream_error_for_package(err, "registry revision", self.digest));
        }
        let expected = integrity.clone();
        let checked = write.check(move |file| {
            tar_resource_integrity(std::io::BufReader::new(file), gzip)
                .is_some_and(|tar| tar.matches(&expected).is_some())
        })
        .await;
        match checked {
            Ok(true) => Ok(FetchedRevision { write, gzip }),
            Ok(false) => {
                write.abandon().await;
                Err(self.integrity_error("the tar does not match its digest".to_string()))
            }
            Err(err) => {
                write.abandon().await;
                Err(RegistryError::Io(err))
            }
        }
    }

    /// The cached revision, which was checked against its digest before it
    /// was cached.
    async fn cached(&self, namespace: &str) -> Option<Response> {
        let storage = &self.state.inner.storage;
        let (mut file, len) = match storage.open_upstream_revision_blob(namespace, self.digest)
            .await
        {
            Ok(Some(cached)) => cached,
            Ok(None) => return None,
            Err(err) => {
                tracing::warn!(?err, registry = %self.registry, digest = %self.digest, "revision tarball cache open failed");
                return None;
            }
        };
        let gzip = match starts_with_gzip_magic(&mut file).await {
            Ok(gzip) => gzip,
            Err(err) => return Some(RegistryError::Io(err).into_response()),
        };
        let body = streaming::stream_file(file);
        Some(
            revision_tarball_response(self.state, self.headers, body, Some(len), gzip_tar(gzip))
                .await,
        )
    }

    fn integrity_error(&self, reason: String) -> RegistryError {
        tarball_integrity_error("registry revision", self.digest, reason)
    }
}

/// An upstream revision whose tar matched its digest, not yet cached or served.
struct FetchedRevision {
    write: pnpr_storage::BlobWrite,
    /// Whether the blob holds the tar's gzip encoding rather than the tar.
    gzip: bool,
}

/// Whether an upstream revision response is gzip-encoded. Any coding other
/// than gzip or none is refused.
fn is_gzip_encoded(headers: &HeaderMap) -> Result<bool, String> {
    let Some(coding) = headers.get(header::CONTENT_ENCODING) else { return Ok(false) };
    let coding_bytes = coding.as_bytes();
    if coding_bytes.eq_ignore_ascii_case(b"identity") {
        return Ok(false);
    }
    if coding_bytes.eq_ignore_ascii_case(b"gzip") || coding_bytes.eq_ignore_ascii_case(b"x-gzip") {
        return Ok(true);
    }
    Err(format!("the upstream sent an unsupported content coding: {coding:?}"))
}

/// How a stored upstream revision holds its tar.
fn gzip_tar(gzip: bool) -> RevisionResource {
    if gzip {
        RevisionResource::Gzip(TarSource {
            name: None,
            tgz_integrity: None,
            keeps_content: false,
            trust: TgzTrust::Verified,
            tar_integrity_known: true,
            decode_limit: tar_decode_limit(None),
        })
    } else {
        RevisionResource::Tar
    }
}

async fn starts_with_gzip_magic(file: &mut tokio::fs::File) -> std::io::Result<bool> {
    let mut magic = [0u8; 2];
    let gzip = match file.read_exact(&mut magic).await {
        Ok(_) => magic == GZIP_MAGIC,
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => false,
        Err(err) => return Err(err),
    };
    file.seek(std::io::SeekFrom::Start(0)).await?;
    Ok(gzip)
}

/// A revision's stored body.
pub(super) enum RevisionResource {
    /// The gzip encoding of the tar: a published `.tgz`, or an upstream's
    /// gzip-encoded response.
    Gzip(TarSource),
    /// The tar itself. The response compression layer encodes it for the
    /// request.
    Tar,
}

/// Serve a revision's tar from `body`, encoded as the request accepts.
pub(super) async fn revision_tarball_response(
    state: &AppState,
    headers: &HeaderMap,
    body: Body,
    len: Option<u64>,
    resource: RevisionResource,
) -> Response {
    let response = match resource {
        RevisionResource::Gzip(source) => {
            tar_response(state, headers, tarball_response(body, len), source).await
        }
        RevisionResource::Tar => {
            let mut response = tarball_response(body, len);
            let response_headers = response.headers_mut();
            response_headers.insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/x-tar"),
            );
            response_headers.append(
                header::VARY,
                header::HeaderValue::from_static("accept-encoding"),
            );
            response
        }
    };
    private_no_cache(response)
}

pub(super) async fn serve_hosted_revision_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: &str,
    digest: &str,
) -> Response {
    let sources = hosted_revision_sources(state, registry);
    if sources.is_empty() {
        return not_found();
    }

    let mut scan = RevisionScan::default();
    for source in sources {
        let Some(hosted) = state.inner.config.routing.hosted.get(&source) else {
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
            RevisionSource { registry, source: &source, storage: &storage, headers, digest },
            refs,
            &mut scan,
        )
        .await;
        if let Some(response) = served {
            return response;
        }
    }

    serve_private_revision_refs(state, headers, scan).await
}

/// The published `.tgz` of a hosted version whose tar `digest` names.
pub(super) struct HostedOriginal {
    /// The version's `.tgz` integrity, which keys the tar-integrity table and
    /// the level-19 variant.
    tgz_integrity: String,
    unpacked_size: Option<u64>,
}

/// The hosted version's published `.tgz`, while its tar is the one `digest`
/// names. A version without a recorded tar has none to serve.
pub(super) async fn hosted_original(
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
    digest: &str,
) -> Result<Option<HostedOriginal>, RegistryError> {
    let Some(bytes) = storage.read_hosted_document(package).await? else {
        return Ok(None);
    };
    let packument = serde_json::from_slice::<Value>(&bytes)?;
    let Some(dist) = packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .and_then(|manifest| manifest.get("dist"))
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let Some(tgz_integrity) = tgz_integrity_key(dist) else { return Ok(None) };
    let table = storage.read_tar_integrities(package).await?;
    let derived = table.get(&tgz_integrity);
    let current = derived
        .and_then(|tar| tar.tar_integrity.parse::<Integrity>().ok())
        .and_then(|tar| integrity_addressed_tarball_path(&tar))
        .is_some_and(|path| path == format!("-/tarballs/sha512/{digest}"));
    let unpacked_size = derived
        .and_then(|tar| tar.unpacked_size)
        .or_else(|| dist.get("unpackedSize").and_then(Value::as_u64));
    Ok(current.then_some(HostedOriginal { tgz_integrity, unpacked_size }))
}

pub(super) async fn open_hosted_revision_tarball(
    state: &AppState,
    headers: &HeaderMap,
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
    original: HostedOriginal,
) -> Response {
    let filename = package.tarball_name_for_version(version);
    let (body, len) = match storage.open_hosted_blob(package, &filename).await {
        Ok(Some(blob)) => blob,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let source = TarSource {
        name: Some(package.clone()),
        tgz_integrity: Some(original.tgz_integrity),
        keeps_content: true,
        trust: TgzTrust::Verified,
        tar_integrity_known: true,
        decode_limit: tar_decode_limit(original.unpacked_size),
    };
    revision_tarball_response(state, headers, body, len, RevisionResource::Gzip(source)).await
}

/// Prefer SRI and fall back to legacy SHA-1, while refusing unverified bytes.
pub(super) fn declared_tarball_integrity(
    dist: &DistBlock,
    name: &CanonicalPackageName,
    filename: &str,
    version: &str,
) -> Result<Integrity, RegistryError> {
    let integrity = if let Some(declared) = dist.integrity.as_deref() {
        streaming::parse_integrity(declared)
            .map_err(|err| {
                tarball_integrity_error(
                    name.as_str(),
                    filename,
                    format!("malformed dist.integrity: {err}"),
                )
            })?
    } else {
        let shasum = dist.shasum
            .as_deref()
            .ok_or_else(|| {
                tarball_integrity_error(
                    name.as_str(),
                    filename,
                    format!("packument has no dist.integrity or dist.shasum for {version:?}"),
                )
            })?;
        Integrity::from_hex(shasum, ssri::Algorithm::Sha1)
            .map_err(|err| {
                tarball_integrity_error(
                    name.as_str(),
                    filename,
                    format!("malformed dist.shasum: {err}"),
                )
            })?
    };
    Ok(integrity)
}
