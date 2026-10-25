//! The `<name>@<version>+r<revision>` route: a registry revision's tar.
//!
//! A hosted version serves the tar of its published `.tgz` at revision `0`.
//! An upstream revision is bound to its digest in the upstream packument,
//! fetched at `<name>@<version>+r<revision>`, gzip-encoded or as a plain tar,
//! and is cached or served only once the whole tar matched the digest.
//! Either is encoded for the request as a tar registry's tarball is (see
//! [`super::tar_responses`]).

use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, FetchOutcome, HeaderMap, Identity,
    Integrity, MAX_TARBALL_BYTES, RegistryError, RegistrySource, Response, Storage, TarSource,
    TgzTrust, Upstream, Value, authorize, authorized_upstream, ensure_osv_allowed,
    extract_upstream_revision_integrity, header, hosted_read_namespace, not_found,
    private_no_cache, read_source_packument, resolve_registry_source, revision_source_registry,
    streaming, tar_decode_limit, tar_resource_integrity, tar_response, tarball_integrity_error,
    tarball_response, tarball_stream_error_for_package, tgz_integrity_key,
    upstream_cache_namespace,
};
use axum::{body::Body, response::IntoResponse};
use pnpm_crypto_hash::has_complete_sha512_digest;
use pnpm_lockfile::TarballRevision;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Split a `<version>+r<revision>` suffix from `<version>`.
///
/// Returns `None` when `version` does not end with `+r<digits>`, `Some(Ok(_))`
/// when it names a canonical revision, and `Some(Err(()))` when `+r<digits>`
/// has a leading zero or overflows [`TarballRevision`].
pub(super) fn parse_revision_version(version: &str) -> Option<Result<(&str, TarballRevision), ()>> {
    let (base_version, build) = version.split_once('+')?;
    let digits = build.strip_prefix('r')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if base_version.is_empty() || (digits != "0" && digits.starts_with('0')) {
        return Some(Err(()));
    }
    let parsed = digits
        .parse::<u64>()
        .ok()
        .and_then(|num| TarballRevision::try_from(num).ok());
    Some(
        parsed
            .map(|revision| (base_version, revision))
            .ok_or(()),
    )
}

pub(super) async fn serve_revision_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    target: &str,
    (raw_name, version, revision): (&str, &str, TarballRevision),
) -> Response {
    let Ok(name) = CanonicalPackageName::parse(raw_name, Ecosystem::Npm) else {
        return not_found();
    };
    if !pnpr_package_name::is_safe_path_segment(version) || version.contains('+') {
        return not_found();
    }
    match resolve_registry_source(state, target, name.as_str()) {
        RegistrySource::Upstream(source) => {
            serve_upstream_revision_tarball(
                state,
                identity,
                headers,
                &source,
                (&name, version, revision),
            )
            .await
        }
        RegistrySource::Hosted(source) => {
            serve_hosted_revision_tarball(
                state,
                identity,
                headers,
                &source,
                (&name, version, revision),
            )
            .await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    }
}

async fn serve_upstream_revision_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    source: &str,
    (name, version, revision): (&CanonicalPackageName, &str, TarballRevision),
) -> Response {
    let (upstream, integrity) =
        match load_upstream_revision_target(state, identity, source, name, version, revision).await
        {
            Ok(Some(target)) => target,
            Ok(None) => return not_found(),
            Err(response) => return response,
        };
    let cache_filename =
        format!("revision-{}.bin", super::ecosystem::sha256_hex(integrity.to_string().as_bytes()));
    let request = UpstreamRevision {
        state,
        headers,
        source,
        upstream,
        name,
        version,
        revision,
        cache_filename: &cache_filename,
    };
    request.serve(&integrity).await
}

async fn load_upstream_revision_target<'state>(
    state: &'state AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    version: &str,
    revision: TarballRevision,
) -> Result<Option<(&'state Upstream, Integrity)>, Response> {
    let resolved = RegistrySource::Upstream(source.to_string());
    if let Err(err) = authorize(state, identity, &resolved, name.as_str(), Action::Access) {
        return Err(err.into_response());
    }
    if let Err(err) = ensure_osv_allowed(state, name, version) {
        return Err(err.into_response());
    }
    let upstream = match authorized_upstream(state, identity, source) {
        Ok(upstream) => upstream,
        Err(err) => return Err(err.into_response()),
    };
    let Some(source_registry) = revision_source_registry(state, source) else {
        return Ok(None);
    };
    let bytes = match read_source_packument(state, identity, &resolved, name).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(err) => return Err(err.into_response()),
    };
    match extract_upstream_revision_integrity(&bytes, name, version, revision, source_registry) {
        Ok(integrity) => Ok(integrity.map(|integrity| (upstream, integrity))),
        Err(err) => Err(RegistryError::from(err).into_response()),
    }
}

struct UpstreamRevision<'a> {
    state: &'a AppState,
    headers: &'a HeaderMap,
    source: &'a str,
    upstream: &'a Upstream,
    name: &'a CanonicalPackageName,
    version: &'a str,
    revision: TarballRevision,
    cache_filename: &'a str,
}

impl UpstreamRevision<'_> {
    async fn serve(&self, integrity: &Integrity) -> Response {
        let namespace = upstream_cache_namespace(self.state, self.source);
        if self.upstream.caches()
            && let Some(response) = self.cached(&namespace).await
        {
            return response;
        }
        let fetched =
            self.upstream.fetch_revision_tarball_response(self.name, self.version, self.revision)
                .await;
        let response = match fetched {
            Ok(FetchOutcome::Ok(response)) => response,
            Ok(FetchOutcome::NotFound) => return not_found(),
            Err(err) => return err.into_response(),
        };
        let checked = match self.download_checked(&namespace, response, integrity).await {
            Ok(checked) => checked,
            Err(err) => return err.into_response(),
        };
        if self.upstream.caches() {
            if let Err(err) = checked.write.finalize().await {
                return RegistryError::Io(err).into_response();
            }
            return self.cached(&namespace).await.unwrap_or_else(not_found);
        }
        match checked.write.into_temp_file().await {
            Ok((file, len, tmp_path)) => {
                let body = streaming::stream_file_and_remove(file, tmp_path);
                let resource = gzip_tar(checked.gzip);
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
        let mut write =
            storage.open_upstream_blob_tmp(namespace, self.name, self.cache_filename).await?;
        if let Err(err) =
            streaming::download_unverified(response, &mut write, MAX_TARBALL_BYTES).await
        {
            write.abandon().await;
            let label = format!("{}+r{}", self.version, self.revision);
            return Err(tarball_stream_error_for_package(err, self.name.as_str(), &label));
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
        let (mut file, len) =
            match storage.open_upstream_blob(namespace, self.name, self.cache_filename).await {
                Ok(Some(cached)) => cached,
                Ok(None) => return None,
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        registry = %self.source,
                        package = %self.name.as_str(),
                        "revision tarball cache open failed",
                    );
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
        let label = format!("{}+r{}", self.version, self.revision);
        tarball_integrity_error(self.name.as_str(), &label, reason)
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

async fn serve_hosted_revision_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    source: &str,
    (package, version, revision): (&CanonicalPackageName, &str, TarballRevision),
) -> Response {
    let org = match hosted_read_namespace(state, identity, source, package.as_str()) {
        Ok(org) => org,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = ensure_osv_allowed(state, package, version) {
        return err.into_response();
    }
    if revision.get() != 0 {
        return not_found();
    }
    let storage = state.inner.storage.for_hosted(&org);
    let original = match hosted_original(&storage, package, version).await {
        Ok(Some(original)) => original,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    open_hosted_revision_tarball(state, headers, &storage, package, version, original).await
}

/// The published `.tgz` of a hosted version.
struct HostedOriginal {
    /// The version's `.tgz` integrity, which keys the tar-integrity table and
    /// the level-19 variant.
    tgz_integrity: String,
    unpacked_size: Option<u64>,
}

/// The hosted version's published `.tgz`, while its tar has a recorded
/// `sha512` digest. A version without a recorded tar has none to serve.
async fn hosted_original(
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
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
    let has_tar = derived
        .and_then(|tar| tar.tar_integrity.parse::<Integrity>().ok())
        .is_some_and(|tar| has_complete_sha512_digest(&tar));
    let unpacked_size = derived
        .and_then(|tar| tar.unpacked_size)
        .or_else(|| dist.get("unpackedSize").and_then(Value::as_u64));
    Ok(has_tar.then_some(HostedOriginal { tgz_integrity, unpacked_size }))
}

async fn open_hosted_revision_tarball(
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
