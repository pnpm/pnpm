//! The OCI distribution surface.
//!
//! Unlike every other ecosystem, this one cannot be moved under a path
//! prefix. A client derives the API root from the image reference's host, so
//! `pnpr.example.com/acme/app:1.0` always requests
//! `/v2/acme/app/manifests/1.0`. `/v2/` therefore mounts at the host root
//! whatever else is served here, and the repository name alone selects the
//! registry through the same declared-provenance rules every other surface
//! uses. `/oci/v2/` and `/oci/~<name>/v2/` are served too, for the clients
//! that do accept a path in their registry configuration.
//!
//! A push is many requests: every blob, then the manifest that references
//! them. The manifest write is the commit point, and the only part that goes
//! through the publish journal. A blob nothing references is invisible rather
//! than half-published, which is what makes the split safe, and why
//! collecting unreferenced blobs is a job of its own.

pub(super) mod tokens;

pub(super) use response::{Refusal, error};

pub(super) use publication::{OciPublication, authorize_publication};

mod response;
use response::{
    accepted, api_version, created, hosted_manifest_response, insert_header, json,
    method_not_allowed, no_content, range_not_satisfiable, ranged_blob_response, registry_error,
    server_error, unknown_repository,
};

mod upload_body;
use upload_body::{
    append_body, collect_body, hash_upload, now_millis, parse_content_range, read_manifest_bytes,
    upload_lock_key,
};

mod referrer_page;
use referrer_page::{
    ReferrerDescriptor, ReferrerFilter, ReferrerPage, ReferrerStep, Referrers, indexed_referrer,
    read_image_document, referrer_entry_step,
};

mod discovery;

mod manifest_request;

mod upload_session;

mod referrers;

mod deletion;
mod proxy;
mod publication;

use super::{
    Action, AppState, AuthedCaller, RegistrySource, TargetRegistry, authorize,
    documents::read_hosted_document,
    ecosystem::{addressed_registry, caller_scoped, hosted_sources, mount_bases},
    hosted_read_namespace, private_no_cache, resolve_ecosystem_source,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, OriginalUri, Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use pnpr_error::RegistryError;
use pnpr_oci::{
    API_SEGMENT, Digest, ErrorBody, ErrorCode, ImageDocument, Manifest, ManifestEntry,
    ReferrerMetadata, media_type,
};
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_search::percent_decode;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate, Storage, upload::BlobUpload};
use serde::Serialize;
use sha2::Sha256;
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

const ECOSYSTEM: Ecosystem = Ecosystem::Oci;

/// The most distinct blobs one manifest may reference. An image index lists
/// a handful of manifests and an image its layers, so this is far above any
/// real one and only bounds what a crafted manifest can ask the store to do.
const MAX_MANIFEST_REFERENCES: usize = 4096;

/// How much of a blob is hashed per read when verifying a finished upload.
const HASH_CHUNK: usize = 64 * 1024;

const MAX_REFERRER_READS: usize = 32;
const MAX_REFERRER_READ_BYTES: u64 = 8 * 1024 * 1024;

const DOCKER_CONTENT_DIGEST: &str = "docker-content-digest";
const DOCKER_UPLOAD_UUID: &str = "docker-upload-uuid";
const API_VERSION_HEADER: &str = "docker-distribution-api-version";

/// What a 401 offers. A client that reads this sends its stored credentials
/// on the retry; one that does not have any carries on unauthenticated.
const CHALLENGE: &str = r#"Basic realm="pnpr""#;

pub(super) fn routes(prefixed: bool) -> Router<AppState> {
    // The root is this surface's own addition: a client derives the API root
    // from the image reference's host, so `/v2/` answers there whether or not
    // the ecosystem is prefixed.
    let mut bases = vec![String::new()];
    bases.extend(mount_bases(ECOSYSTEM, prefixed));
    bases.dedup();
    let mut router = Router::new();
    for base in bases {
        router = router
            .route(&format!("{base}/{API_SEGMENT}/token"), get(tokens::issue))
            .route(&format!("{base}/{API_SEGMENT}/"), get(get_version_check))
            .route(&format!("{base}/{API_SEGMENT}"), get(get_version_check))
            .route(&format!("{base}/{API_SEGMENT}/{{*path}}"), any(dispatch));
    }
    // Streaming handlers enforce the configured cumulative upload limit.
    router.route_layer(DefaultBodyLimit::disable())
}

/// `GET /v2/` — the version check every client makes first, and what
/// `docker login` reads to decide whether its credentials were accepted.
async fn get_version_check(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    if addressed_registry(&state, registry.as_deref(), Ecosystem::Oci).is_none() {
        return error(ErrorCode::NameUnknown, "no registry is addressed here");
    }
    // A client fixes its authentication scheme from this one response and
    // never offers credentials again if it comes back 200, so an anonymous
    // caller is challenged even where reads are open: without the challenge
    // a push could not authenticate. A caller with no credentials carries on
    // regardless, and public repositories still answer its later requests.
    if identity == Identity::Anonymous
        && !headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(super::authentication::token_credentials)
            .is_some_and(|token| token.starts_with(tokens::TOKEN_PREFIX))
    {
        return tokens::challenge(
            &state,
            uri.path().trim_end_matches('/'),
            None,
            error(ErrorCode::Unauthorized, "authentication required"),
        );
    }
    api_version(StatusCode::OK.into_response())
}

/// Every other endpoint. A repository name is many path segments, so the
/// route captures the whole tail and the trailing verb says which endpoint
/// was addressed.
async fn dispatch(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    OriginalUri(uri): OriginalUri,
    Path(params): Path<HashMap<String, String>>,
    incoming: axum::extract::Request,
) -> Response {
    let Some(endpoint) = params.get("path").and_then(|tail| parse_endpoint(tail)) else {
        return error(ErrorCode::NameUnknown, "no distribution endpoint at this path");
    };
    let tail = &params["path"];
    let (parts, body) = incoming.into_parts();
    let request = Request {
        state,
        identity,
        registry,
        base: api_base(uri.path(), tail),
        method: parts.method,
        digest: query_param(uri.query(), "digest"),
        query: uri.query().unwrap_or_default().to_string(),
        headers: parts.headers,
    };
    let scope_name = match &endpoint {
        Endpoint::Catalog => None,
        Endpoint::Referrers { name, .. }
        | Endpoint::Tags { name }
        | Endpoint::Manifest { name, .. }
        | Endpoint::Blob { name, .. }
        | Endpoint::StartUpload { name }
        | Endpoint::Upload { name, .. } => Some(name.clone()),
    };
    let response = match endpoint {
        Endpoint::Referrers { name, digest } => request.referrers(&name, &digest).await,
        Endpoint::Catalog => request.catalog().await,
        Endpoint::Tags { name } => request.tags(&name).await,
        Endpoint::Manifest { name, reference } => request.manifest(&name, &reference, body).await,
        Endpoint::Blob { name, digest } => request.blob(&name, &digest).await,
        Endpoint::StartUpload { name } => request.start_upload(&name, body).await,
        Endpoint::Upload { name, id } => request.upload(&name, &id, body).await,
    };
    request.challenge(scope_name.as_deref(), response)
}

/// Which endpoint a captured path tail addresses.
enum Endpoint {
    Catalog,
    Referrers { name: String, digest: String },
    Tags { name: String },
    Manifest { name: String, reference: String },
    Blob { name: String, digest: String },
    StartUpload { name: String },
    Upload { name: String, id: String },
}

/// Split a tail into its repository name and the endpoint it addresses. The
/// name runs up to the trailing verb, which is why a repository may not be
/// named so that its last components spell one.
fn parse_endpoint(tail: &str) -> Option<Endpoint> {
    let trimmed = tail.trim_matches('/');
    if trimmed == "_catalog" {
        return Some(Endpoint::Catalog);
    }
    let segments: Vec<&str> = trimmed.split('/').collect();
    let name = |upto: usize| (upto > 0).then(|| segments[..upto].join("/"));
    let last = segments.len();
    match segments.as_slice() {
        [.., "blobs", "uploads"] => Some(Endpoint::StartUpload { name: name(last - 2)? }),
        [.., "blobs", "uploads", id] => {
            Some(Endpoint::Upload { name: name(last - 3)?, id: (*id).to_string() })
        }
        [.., "tags", "list"] => Some(Endpoint::Tags { name: name(last - 2)? }),
        [.., "manifests", reference] => {
            Some(Endpoint::Manifest { name: name(last - 2)?, reference: (*reference).to_string() })
        }
        [.., "referrers", digest] => {
            Some(Endpoint::Referrers { name: name(last - 2)?, digest: (*digest).to_string() })
        }
        [.., "blobs", digest] => {
            Some(Endpoint::Blob { name: name(last - 2)?, digest: (*digest).to_string() })
        }
        _ => None,
    }
}

/// One in-flight distribution request, with the parts every endpoint needs.
/// The body stays out so a handler that streams it can still read the rest.
struct Request {
    state: AppState,
    identity: Identity,
    registry: Option<String>,
    base: String,
    method: Method,
    /// The `?digest=` a client completes an upload with.
    digest: Option<String>,
    query: String,
    headers: HeaderMap,
}

impl Request {
    /// Keep a response that can vary by caller out of shared caches, the way
    /// every other surface does. Without it an intermediary could replay an
    /// authenticated pull of a private repository to the next caller.
    fn caller_scoped(&self, package: Option<&str>, response: Response) -> Response {
        caller_scoped(&self.state, ECOSYSTEM, self.registry.as_deref(), package, response)
    }

    async fn blob(&self, name: &str, digest: &str) -> Response {
        let Ok(digest) = Digest::parse(digest) else {
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        match self.method {
            Method::GET | Method::HEAD => self.read_blob(name, &digest).await,
            Method::DELETE => self.delete_blob(name, &digest).await,
            _ => method_not_allowed(),
        }
    }

    async fn read_blob(&self, name: &str, digest: &Digest) -> Response {
        if let Some((key, source)) = self.upstream_source(name) {
            return self.proxy_blob(&key, &source, digest).await;
        }
        let repo = match self.hosted_repo(name) {
            Ok(repo) => repo,
            Err(refusal) => return refusal.respond(),
        };
        let etag = format!(r#""{digest}""#);
        if let Some(range) = self.requested_download_range(&etag) {
            let ranged = repo
                .storage
                .open_hosted_blob_range(&repo.key, &digest.blob_filename(), &range)
                .await;
            let response = match ranged {
                Ok(Some(blob)) => ranged_blob_response(blob, digest, &etag),
                Ok(None) => return error(ErrorCode::BlobUnknown, "no such blob"),
                Err(err) => return registry_error(err),
            };
            return self.caller_scoped(Some(repo.key.as_str()), response);
        }
        let (body, size) =
            match repo.storage.open_hosted_blob(&repo.key, &digest.blob_filename()).await {
                Ok(Some(blob)) => blob,
                Ok(None) => return error(ErrorCode::BlobUnknown, "no such blob"),
                Err(err) => return registry_error(err),
            };
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ETAG, etag)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(DOCKER_CONTENT_DIGEST, digest.to_string());
        if let Some(size) = size {
            response = response.header(header::CONTENT_LENGTH, size);
        }
        let body = if self.method == Method::HEAD { Body::empty() } else { body };
        let response = response.body(body).unwrap_or_else(|_| server_error());
        self.caller_scoped(Some(repo.key.as_str()), response)
    }

    /// The byte range a `GET` asks for, when it asks for exactly one and its
    /// `If-Range` still matches.
    fn requested_download_range(&self, etag: &str) -> Option<pnpr_storage::GetRange> {
        if self.method != Method::GET
            || self.headers.get(header::IF_RANGE).is_some_and(|value| value != etag)
            || self.headers.get_all(header::RANGE).iter().count() != 1
        {
            return None;
        }
        self.headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_download_range)
    }

    /// The hosted registry a read of `name` resolves to, with the canonical
    /// repository name.
    fn hosted_source(&self, name: &str) -> Result<(CanonicalPackageName, String), Refusal> {
        let key = CanonicalPackageName::parse(name, ECOSYSTEM)
            .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "not a valid repository name"))?;
        let target = addressed_registry(&self.state, self.registry.as_deref(), Ecosystem::Oci)
            .ok_or_else(|| Refusal::new(ErrorCode::NameUnknown, "no registry is addressed here"))?;
        match resolve_ecosystem_source(&self.state, &target, ECOSYSTEM, key.as_str()) {
            RegistrySource::Hosted(source) => Ok((key, source)),
            RegistrySource::Upstream(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => {
                Err(unknown_repository(name))
            }
        }
    }

    /// The hosted registry a write of `name` lands on, once the caller is
    /// allowed to publish it.
    fn publish_target(&self, name: &str) -> Result<(CanonicalPackageName, String), Refusal> {
        authorize_publication(&self.state, &self.identity, self.registry.as_deref(), name)
    }

    /// The hosted repository `name` addresses, with the storage this caller
    /// may read it from.
    fn hosted_repo(&self, name: &str) -> Result<HostedRepo, Refusal> {
        let (key, source) = self.hosted_source(name)?;
        let org = hosted_read_namespace(&self.state, &self.identity, &source, key.as_str())
            .map_err(Refusal::from)?;
        let storage = self.state.inner.storage.for_hosted(&org);
        Ok(HostedRepo { key, source, storage })
    }

    /// Whether a bearer token on the request denies pulling `source_key`.
    fn token_forbids_pull(&self, source_key: &str) -> Result<bool, RegistryError> {
        let Some(raw) = self
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(super::authentication::token_credentials)
        else {
            return Ok(false);
        };
        Ok(tokens::decode(&self.state, &raw)?
            .is_some_and(|claims| !claims.allows(source_key, "pull")))
    }
}

fn parse_download_range(value: &str) -> Option<pnpr_storage::GetRange> {
    let (unit, bounds) = value.trim().split_once('=')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (start, end) = bounds.split_once('-')?;
    let number = |value: &str| {
        (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| value.parse::<u64>().ok())
            .flatten()
    };
    if start.is_empty() {
        return number(end).map(pnpr_storage::GetRange::Suffix);
    }
    let start = number(start)?;
    if end.is_empty() {
        return Some(pnpr_storage::GetRange::Offset(start));
    }
    let end = number(end)?;
    if start > end {
        return None;
    }
    Some(match end.checked_add(1) {
        Some(end) => pnpr_storage::GetRange::Bounded(start..end),
        None => pnpr_storage::GetRange::Offset(start),
    })
}

#[derive(Serialize)]
struct Catalog {
    repositories: Vec<String>,
}

#[derive(Serialize)]
struct TagList<'listing> {
    name: &'listing str,
    tags: Vec<&'listing str>,
}

/// A refusal on its way to becoming a response, small enough to ride in a
/// `Result`'s error slot as the response itself is not.
///
/// The status is carried rather than re-derived from the code, because a
/// `RegistryError` has already chosen one, and deriving it back from the spec
/// code would answer `405` for every error that has no code of its own.
/// A hosted repository and the storage it is read from.
struct HostedRepo {
    key: CanonicalPackageName,
    source: String,
    storage: Storage,
}

impl From<Refusal> for RegistryError {
    fn from(refusal: Refusal) -> Self {
        if let Some(original) = refusal.original {
            return *original;
        }
        match refusal.status {
            StatusCode::UNAUTHORIZED => Self::Unauthenticated { resource: refusal.message },
            StatusCode::FORBIDDEN => Self::Forbidden {
                user: String::new(),
                action: "publish",
                resource: refusal.message,
            },
            StatusCode::NOT_FOUND => Self::NotFound,
            _ => Self::BadRequest { reason: refusal.message },
        }
    }
}

/// The API root a request came in through, so an upload continues where it
/// started even when the client addressed a prefixed base.
fn api_base(uri_path: &str, tail: &str) -> String {
    uri_path
        .trim_end_matches('/')
        .strip_suffix(tail.trim_end_matches('/'))
        .map_or_else(|| format!("/{API_SEGMENT}"), |base| base.trim_end_matches('/').to_string())
}

/// One named parameter of a raw query string.
fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

#[cfg(test)]
mod tests;
