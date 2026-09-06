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

use super::{
    Action, AppState, AuthedCaller, RegistrySource, TargetRegistry, authorize,
    documents::{read_hosted_document, store_hosted_artifact},
    ecosystem::{addressed_registry, caller_scoped, hosted_sources, mount_bases},
    hosted_read_namespace, private_no_cache,
    publishing::{PublishTarget, resolve_publish_target_for},
    resolve_ecosystem_source,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, OriginalUri, Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use futures_util::StreamExt as _;
use pnpr_error::RegistryError;
use pnpr_oci::{
    API_SEGMENT, Digest, ErrorBody, ErrorCode, ImageDocument, Manifest, ManifestEntry, TagEntry,
    is_valid_tag, media_type,
};
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_search::percent_decode;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate, Storage, upload::BlobUpload};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncReadExt as _;

const ECOSYSTEM: Ecosystem = Ecosystem::Oci;

/// The largest manifest accepted. The distribution spec puts the ceiling at
/// 4 MiB; a manifest is a list of digests, so real ones are far smaller.
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

/// The largest single blob accepted. Image layers are the only artifact pnpr
/// serves that routinely runs to gigabytes.
///
/// As a `DefaultBodyLimit` this bounds one request. A resumable upload is many
/// requests, so [`append_body`] holds the whole upload to the same
/// ceiling; without that, chunks that are each under the limit add up past it.
const MAX_BLOB_BYTES: usize = 10 * 1024 * 1024 * 1024;

/// The most distinct blobs one manifest may reference. An image index lists
/// a handful of manifests and an image its layers, so this is far above any
/// real one and only bounds what a crafted manifest can ask the store to do.
const MAX_MANIFEST_REFERENCES: usize = 4096;

/// How much of a blob is hashed per read when verifying a finished upload.
const HASH_CHUNK: usize = 64 * 1024;

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
            .route(&format!("{base}/{API_SEGMENT}/"), get(get_version_check))
            .route(&format!("{base}/{API_SEGMENT}"), get(get_version_check))
            .route(&format!("{base}/{API_SEGMENT}/{{*path}}"), any(dispatch));
    }
    // Layers are the one artifact pnpr serves that outgrows the publish body
    // limit the rest of the router carries. `route_layer` so the limit
    // applies to these routes without giving the router a fallback of its
    // own, which would make it unmergeable.
    router.route_layer(DefaultBodyLimit::max(MAX_BLOB_BYTES))
}

/// `GET /v2/` — the version check every client makes first, and what
/// `docker login` reads to decide whether its credentials were accepted.
async fn get_version_check(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
) -> Response {
    if addressed_registry(&state, registry.as_deref()).is_none() {
        return error(ErrorCode::NameUnknown, "no registry is addressed here");
    }
    // A client fixes its authentication scheme from this one response and
    // never offers credentials again if it comes back 200, so an anonymous
    // caller is challenged even where reads are open: without the challenge
    // a push could not authenticate. A caller with no credentials carries on
    // regardless, and public repositories still answer its later requests.
    if identity == Identity::Anonymous {
        return error(ErrorCode::Unauthorized, "authentication required");
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
    let Some(tail) = params.get("path") else {
        return error(ErrorCode::Unsupported, "unrecognized distribution endpoint");
    };
    let Some(endpoint) = parse_endpoint(tail) else {
        return error(ErrorCode::Unsupported, "unrecognized distribution endpoint");
    };
    let (parts, body) = incoming.into_parts();
    let request = Request {
        state,
        identity,
        registry,
        base: api_base(uri.path(), tail),
        method: parts.method,
        digest: query_param(uri.query(), "digest"),
        headers: parts.headers,
    };
    match endpoint {
        Endpoint::Catalog => request.catalog().await,
        Endpoint::Tags { name } => request.tags(&name).await,
        Endpoint::Manifest { name, reference } => request.manifest(&name, &reference, body).await,
        Endpoint::Blob { name, digest } => request.blob(&name, &digest).await,
        Endpoint::StartUpload { name } => request.start_upload(&name, body).await,
        Endpoint::Upload { name, id } => request.upload(&name, &id, body).await,
    }
}

/// Which endpoint a captured path tail addresses.
enum Endpoint {
    Catalog,
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
    headers: HeaderMap,
}

impl Request {
    /// `GET /v2/_catalog` — the repository names this caller may read.
    async fn catalog(&self) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let Some(target) = addressed_registry(&self.state, self.registry.as_deref()) else {
            return error(ErrorCode::NameUnknown, "no registry is addressed here");
        };
        let mut repositories = Vec::new();
        for source in hosted_sources(&self.state, &target, ECOSYSTEM) {
            let Some(hosted) = self.state.inner.config.hosted.get(&source) else { continue };
            let storage = self.state.inner.storage.for_hosted(&hosted.org);
            let Ok(names) = storage.hosted_package_names().await else { continue };
            // A listing may only name what this caller could have fetched.
            repositories.extend(names.into_iter().filter(|name| {
                authorize(
                    &self.state,
                    &self.identity,
                    &RegistrySource::Hosted(source.clone()),
                    name,
                    Action::Access,
                )
                .is_ok()
            }));
        }
        repositories.sort();
        repositories.dedup();
        // The listing is built from what this caller may read, so it is
        // caller-specific whichever registry it came through.
        private_no_cache(json(StatusCode::OK, &Catalog { repositories }))
    }

    /// `GET /v2/<name>/tags/list`.
    async fn tags(&self, name: &str) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let response =
            match read_hosted_document::<ImageDocument>(&self.state, &self.identity, &source, &key)
                .await
            {
                Ok(Some(document)) => json(
                    StatusCode::OK,
                    &TagList { name: key.as_str(), tags: document.tag_names() },
                ),
                Ok(None) => unknown_repository(name).respond(),
                Err(err) => registry_error(err),
            };
        self.caller_scoped(Some(key.as_str()), response)
    }

    /// Keep a response that can vary by caller out of shared caches, the way
    /// every other surface does. Without it an intermediary could replay an
    /// authenticated pull of a private repository to the next caller.
    fn caller_scoped(&self, package: Option<&str>, response: Response) -> Response {
        caller_scoped(&self.state, ECOSYSTEM, self.registry.as_deref(), package, response)
    }

    /// `HEAD`/`GET`/`PUT`/`DELETE /v2/<name>/manifests/<reference>`.
    async fn manifest(&self, name: &str, reference: &str, body: Body) -> Response {
        match self.method {
            Method::GET | Method::HEAD => self.read_manifest(name, reference).await,
            Method::PUT => self.write_manifest(name, reference, body).await,
            Method::DELETE => self.delete_manifest(name, reference).await,
            _ => method_not_allowed(),
        }
    }

    async fn read_manifest(&self, name: &str, reference: &str) -> Response {
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let document =
            match read_hosted_document::<ImageDocument>(&self.state, &self.identity, &source, &key)
                .await
            {
                Ok(Some(document)) => document,
                Ok(None) => return unknown_repository(name).respond(),
                Err(err) => return registry_error(err),
            };
        let Some(entry) = document.resolve(reference).cloned() else {
            return error(ErrorCode::ManifestUnknown, "no such manifest or tag");
        };
        let org = match hosted_read_namespace(&self.state, &self.identity, &source, key.as_str()) {
            Ok(org) => org,
            Err(err) => return registry_error(err),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        // A manifest is small enough to answer from memory, and the response
        // carries its digest and media type either way.
        let bytes = match read_manifest_bytes(&storage, &key, &entry.digest.blob_filename()).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return error(ErrorCode::ManifestUnknown, "no such manifest"),
            Err(err) => return registry_error(err),
        };
        // `Content-Length` is the manifest's own length on a HEAD too, which is
        // what a client reads to decide whether it already holds the bytes.
        let length = bytes.len();
        let body = if self.method == Method::HEAD { Body::empty() } else { Body::from(bytes) };
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, entry.media_type)
            .header(header::CONTENT_LENGTH, length)
            .header(DOCKER_CONTENT_DIGEST, entry.digest.to_string())
            .body(body)
            .unwrap_or_else(|_| server_error());
        self.caller_scoped(Some(key.as_str()), response)
    }

    async fn write_manifest(&self, name: &str, reference: &str, body: Body) -> Response {
        let (key, org) = match self.publish_target(name) {
            Ok(target) => target,
            Err(refusal) => return refusal.respond(),
        };
        let content_type =
            self.headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        let bytes = match collect_body(body, MAX_MANIFEST_BYTES).await {
            Ok(bytes) => bytes,
            Err(refusal) => return refusal.respond(),
        };
        let digest = Digest::of(&bytes);
        // A manifest pushed under a digest must be the bytes that digest
        // names, or a later pull by it would serve something else.
        if Digest::parse(reference).is_ok_and(|addressed| addressed != digest) {
            return error(
                ErrorCode::DigestInvalid,
                "manifest does not match the digest it was pushed under",
            );
        }
        let manifest = match Manifest::parse(&bytes, content_type) {
            Ok(manifest) => manifest,
            Err(err) => return error(ErrorCode::ManifestInvalid, err.to_string()),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        // One digest is looked up once however often the manifest names it,
        // and a manifest that names more than any image could is refused:
        // otherwise a single 4 MiB body of repeated descriptors becomes tens
        // of thousands of blob lookups.
        let mut looked_up = HashSet::new();
        for descriptor in manifest.references() {
            if !looked_up.insert(descriptor.digest.clone()) {
                continue;
            }
            if looked_up.len() > MAX_MANIFEST_REFERENCES {
                return error(
                    ErrorCode::ManifestInvalid,
                    format!(
                        "a manifest may not reference more than {MAX_MANIFEST_REFERENCES} blobs",
                    ),
                );
            }
            // The size is part of what a client verifies, so a descriptor that
            // disagrees with the stored bytes publishes an image nothing can
            // pull. Refuse it here rather than at every puller.
            match storage.open_hosted_blob(&key, &descriptor.digest.blob_filename()).await {
                Ok(Some((_, Some(size)))) if size != descriptor.size => {
                    return error(
                        ErrorCode::ManifestInvalid,
                        format!(
                            "{} is {size} bytes, but the manifest declares {}",
                            descriptor.digest, descriptor.size,
                        ),
                    );
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    return error(
                        ErrorCode::ManifestBlobUnknown,
                        format!("{} is not in this repository", descriptor.digest),
                    );
                }
                Err(err) => return registry_error(err),
            }
        }

        let mut addition = ImageDocument::new(key.as_str());
        addition.insert_manifest(ManifestEntry {
            digest: digest.clone(),
            media_type: manifest.media_type().to_string(),
            size: bytes.len() as u64,
        });
        if Digest::parse(reference).is_err() {
            if !is_valid_tag(reference) {
                return error(ErrorCode::ManifestInvalid, "not a valid tag or digest");
            }
            addition.set_tag(TagEntry {
                tag: reference.to_string(),
                digest: digest.clone(),
                updated: now_millis(),
            });
        }
        // An index's children are manifests, not loose blobs, and the check
        // above only proves the bytes are present. Refusing here rather than
        // before the write is what makes it race-free: the closure reads the
        // stored document under the package lock, so a child pushed
        // concurrently is seen and one deleted concurrently is not missed.
        //
        // Re-pushing a manifest is how a client retries and moving a tag is
        // ordinary, so nothing else is refused; the merge decides what wins.
        let children: Vec<Digest> = if media_type::is_index(manifest.media_type()) {
            manifest.references().map(|descriptor| descriptor.digest.clone()).collect()
        } else {
            Vec::new()
        };
        let refuse = move |stored: &ImageDocument| {
            for child in &children {
                if stored.manifest(child).is_none() {
                    return Err(RegistryError::BadRequest {
                        reason: format!("{child} is not a manifest in this repository"),
                    });
                }
            }
            Ok(())
        };
        match store_hosted_artifact::<ImageDocument>(
            &self.state,
            &org,
            &key,
            &digest.blob_filename(),
            &bytes,
            refuse,
            addition,
        )
        .await
        {
            Ok(()) => {
                created(&format!("{}/{}/manifests/{digest}", self.base, key.as_str()), &digest)
            }
            Err(err) => registry_error(err),
        }
    }

    async fn delete_manifest(&self, name: &str, reference: &str) -> Response {
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        if let Err(err) = authorize(
            &self.state,
            &self.identity,
            &RegistrySource::Hosted(source.clone()),
            key.as_str(),
            Action::Unpublish,
        ) {
            return registry_error(err);
        }
        let Some(hosted) = self.state.inner.config.hosted.get(&source) else {
            return unknown_repository(name).respond();
        };
        let storage = self.state.inner.storage.for_hosted(&hosted.org);
        let outcome = storage
            .update_hosted_document_with_retry(&key, DOCUMENT_WRITE_RETRIES, |existing| {
                let Some(bytes) = existing else { return Ok(None) };
                let mut document = ImageDocument::parse(bytes).map_err(RegistryError::Json)?;
                let removed = match Digest::parse(reference) {
                    Ok(digest) => document.remove_manifest(&digest),
                    Err(_) => document.remove_tag(reference),
                };
                Ok(removed.then(|| document.to_bytes()))
            })
            .await;
        match outcome {
            Ok(DocumentUpdate::Written) => no_content(StatusCode::ACCEPTED),
            Ok(_) => error(ErrorCode::ManifestUnknown, "no such manifest or tag"),
            Err(err) => registry_error(err),
        }
    }

    /// `HEAD`/`GET /v2/<name>/blobs/<digest>`.
    ///
    /// Deleting a blob is optional in the spec and is not offered: nothing
    /// tracks which manifests reference a blob yet, so removing one would
    /// leave the repository advertising an image that can no longer be
    /// pulled. Reclaiming what no manifest names is the collector's job,
    /// tracked in
    /// [pnpm/pnpm#14630](https://github.com/pnpm/pnpm/issues/14630).
    async fn blob(&self, name: &str, digest: &str) -> Response {
        let Ok(digest) = Digest::parse(digest) else {
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        match self.method {
            Method::GET | Method::HEAD => self.read_blob(name, &digest).await,
            _ => method_not_allowed(),
        }
    }

    async fn read_blob(&self, name: &str, digest: &Digest) -> Response {
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let org = match hosted_read_namespace(&self.state, &self.identity, &source, key.as_str()) {
            Ok(org) => org,
            Err(err) => return registry_error(err),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        let (body, size) = match storage.open_hosted_blob(&key, &digest.blob_filename()).await {
            Ok(Some(blob)) => blob,
            Ok(None) => return error(ErrorCode::BlobUnknown, "no such blob"),
            Err(err) => return registry_error(err),
        };
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(DOCKER_CONTENT_DIGEST, digest.to_string());
        if let Some(size) = size {
            response = response.header(header::CONTENT_LENGTH, size);
        }
        let body = if self.method == Method::HEAD { Body::empty() } else { body };
        let response = response.body(body).unwrap_or_else(|_| server_error());
        self.caller_scoped(Some(key.as_str()), response)
    }

    /// `POST /v2/<name>/blobs/uploads/` — start an upload, or complete one in
    /// a single request when the client sends `?digest=`.
    async fn start_upload(&self, name: &str, body: Body) -> Response {
        if self.method != Method::POST {
            return method_not_allowed();
        }
        let (key, org) = match self.publish_target(name) {
            Ok(target) => target,
            Err(refusal) => return refusal.respond(),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        let upload = match storage.begin_blob_upload().await {
            Ok(upload) => upload,
            Err(err) => return registry_error(err),
        };
        if let Err(refusal) = append_body(&storage, &upload, body).await {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return refusal.respond();
        }
        // A cross-repository mount is optional: handing back a fresh upload is
        // the spec's own fallback, and the client re-sends the bytes.
        match self.digest.as_deref() {
            Some(digest) => self.finish_upload(&storage, upload, &key, digest).await,
            None => self.upload_progress(&key, &upload).await,
        }
    }

    /// `PATCH`/`PUT`/`GET`/`DELETE /v2/<name>/blobs/uploads/<id>`.
    async fn upload(&self, name: &str, id: &str, body: Body) -> Response {
        let (key, org) = match self.publish_target(name) {
            Ok(target) => target,
            Err(refusal) => return refusal.respond(),
        };
        // One upload is one sequence of bytes, so its requests are serialized:
        // two chunks appending at once, or a chunk landing between the hash
        // and the promotion, would store bytes that are not the digest they
        // are stored under.
        let _guard = self.state.inner.package_locks.lock(&upload_lock_key(id)).await;
        let storage = self.state.inner.storage.for_hosted(&org);
        let upload = match storage.open_blob_upload(id).await {
            Ok(Some(upload)) => upload,
            Ok(None) => return error(ErrorCode::BlobUploadUnknown, "no such upload"),
            Err(err) => return registry_error(err),
        };
        match self.method {
            Method::PATCH => {
                if let Err(response) = self.check_chunk_start(&key, &upload).await {
                    return response;
                }
                match append_body(&storage, &upload, body).await {
                    Ok(()) => self.upload_progress(&key, &upload).await,
                    Err(refusal) => refusal.respond(),
                }
            }
            Method::PUT => {
                let Some(digest) = self.digest.as_deref() else {
                    return error(
                        ErrorCode::DigestInvalid,
                        "a completed upload must name its digest",
                    );
                };
                if let Err(refusal) = append_body(&storage, &upload, body).await {
                    return refusal.respond();
                }
                self.finish_upload(&storage, upload, &key, digest).await
            }
            Method::GET => self.upload_progress(&key, &upload).await,
            Method::DELETE => match storage.abort_blob_upload(upload.id()).await {
                Ok(_) => no_content(StatusCode::NO_CONTENT),
                Err(err) => registry_error(err),
            },
            _ => method_not_allowed(),
        }
    }

    /// Refuse a chunk that does not continue where the upload left off, so a
    /// retry cannot silently interleave bytes.
    async fn check_chunk_start(
        &self,
        key: &CanonicalPackageName,
        upload: &BlobUpload,
    ) -> Result<(), Response> {
        let Some(range) = self.headers.get(header::CONTENT_RANGE) else { return Ok(()) };
        let offset = upload.offset().await.map_err(registry_error)?;
        match range.to_str().ok().and_then(parse_range_start) {
            Some(start) if start == offset => Ok(()),
            // The refusal carries where the upload actually stands, so the
            // client can resume rather than start over.
            Some(_) => Err(range_not_satisfiable(&self.base, key.as_str(), upload.id(), offset)),
            None => Err(error(ErrorCode::BlobUploadInvalid, "malformed Content-Range")),
        }
    }

    async fn upload_progress(&self, key: &CanonicalPackageName, upload: &BlobUpload) -> Response {
        match upload.offset().await {
            Ok(offset) => accepted(&self.base, key.as_str(), upload.id(), offset),
            Err(err) => registry_error(err),
        }
    }

    /// Verify a finished upload against the digest the client promised and
    /// promote it into the repository.
    async fn finish_upload(
        &self,
        storage: &Storage,
        upload: BlobUpload,
        key: &CanonicalPackageName,
        digest: &str,
    ) -> Response {
        let Ok(digest) = Digest::parse(digest) else {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        match hash_upload(&upload).await {
            Ok(actual) if actual == digest => {}
            Ok(_) => {
                let _ = storage.abort_blob_upload(upload.id()).await;
                return error(ErrorCode::DigestInvalid, "uploaded bytes do not match the digest");
            }
            Err(err) => return registry_error(err),
        }
        let slot = match storage.stage_uploaded_blob(upload, key, &digest.blob_filename()).await {
            Ok(slot) => slot,
            Err(err) => return registry_error(err),
        };
        match storage.finalize_blob_slot(slot).await {
            // A blob is its bytes, so another writer winning the slot means
            // the content is already there, which is what the client wanted.
            Ok(_) => created(&format!("{}/{}/blobs/{digest}", self.base, key.as_str()), &digest),
            Err(err) => registry_error(err),
        }
    }

    /// The hosted registry a read of `name` resolves to, with the canonical
    /// repository name.
    fn hosted_source(&self, name: &str) -> Result<(CanonicalPackageName, String), Refusal> {
        let key = CanonicalPackageName::parse(name, ECOSYSTEM)
            .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "not a valid repository name"))?;
        let target = addressed_registry(&self.state, self.registry.as_deref())
            .ok_or_else(|| Refusal::new(ErrorCode::NameUnknown, "no registry is addressed here"))?;
        match resolve_ecosystem_source(&self.state, &target, ECOSYSTEM, key.as_str()) {
            RegistrySource::Hosted(source) => Ok((key, source)),
            // Upstream image registries are not proxied yet, so a name routing
            // to one is not served rather than served wrongly.
            RegistrySource::Upstream(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => {
                Err(unknown_repository(name))
            }
        }
    }

    /// The hosted registry a write of `name` lands on, once the caller is
    /// allowed to publish it.
    fn publish_target(&self, name: &str) -> Result<(CanonicalPackageName, String), Refusal> {
        let key = CanonicalPackageName::parse(name, ECOSYSTEM)
            .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "not a valid repository name"))?;
        match resolve_publish_target_for(
            &self.state,
            &self.identity,
            self.registry.as_deref(),
            ECOSYSTEM,
            key.as_str(),
        ) {
            PublishTarget::Hosted { source, org } => {
                authorize(
                    &self.state,
                    &self.identity,
                    &RegistrySource::Hosted(source),
                    key.as_str(),
                    Action::Publish,
                )?;
                Ok((key, org))
            }
            PublishTarget::Denied(err) => Err(err.into()),
            PublishTarget::Reject(reason) => Err(Refusal::new(ErrorCode::Denied, reason)),
            PublishTarget::NotFound => Err(unknown_repository(name)),
        }
    }
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
/// `RegistryError` already decided one: deriving it back from the spec code
/// turned every error without a code of its own into `405`.
struct Refusal {
    status: StatusCode,
    code: ErrorCode,
    message: String,
}

impl Refusal {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { status: status_for(code), code, message: message.into() }
    }

    fn respond(self) -> Response {
        respond(self.status, self.code, self.message)
    }
}

/// A pnpr error in the distribution spec's own vocabulary, keeping the status
/// the error chose.
impl From<RegistryError> for Refusal {
    fn from(err: RegistryError) -> Self {
        let message = err.public_message();
        let status = err.into_response().status();
        let code = match status {
            StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
            StatusCode::FORBIDDEN => ErrorCode::Denied,
            StatusCode::NOT_FOUND => ErrorCode::NameUnknown,
            StatusCode::TOO_MANY_REQUESTS => ErrorCode::TooManyRequests,
            // No spec code means "the registry refused this"; the status is
            // what a client acts on.
            _ => ErrorCode::Unsupported,
        };
        Self { status, code, message }
    }
}

fn registry_error(err: RegistryError) -> Response {
    Refusal::from(err).respond()
}

fn error(code: ErrorCode, message: impl Into<String>) -> Response {
    Refusal::new(code, message).respond()
}

fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorCode::Denied => StatusCode::FORBIDDEN,
        ErrorCode::NameUnknown
        | ErrorCode::ManifestUnknown
        | ErrorCode::BlobUnknown
        | ErrorCode::BlobUploadUnknown => StatusCode::NOT_FOUND,
        ErrorCode::Unsupported => StatusCode::METHOD_NOT_ALLOWED,
        ErrorCode::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::BlobUploadInvalid
        | ErrorCode::DigestInvalid
        | ErrorCode::ManifestBlobUnknown
        | ErrorCode::ManifestInvalid
        | ErrorCode::NameInvalid
        | ErrorCode::SizeInvalid => StatusCode::BAD_REQUEST,
    }
}

/// Every failing response carries the spec's error body, and a 401 carries
/// the challenge as well: without it a Docker client treats the refusal as
/// final instead of retrying with its stored credentials.
fn respond(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Response {
    let mut response = json(status, &ErrorBody::new(code, message));
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(CHALLENGE));
    }
    api_version(response)
}

fn method_not_allowed() -> Response {
    error(ErrorCode::Unsupported, "unsupported method for this endpoint")
}

fn unknown_repository(name: &str) -> Refusal {
    Refusal::new(ErrorCode::NameUnknown, format!("no repository named {name:?} is served here"))
}

fn api_version(mut response: Response) -> Response {
    response.headers_mut().insert(API_VERSION_HEADER, HeaderValue::from_static("registry/2.0"));
    response
}

fn json<Payload: Serialize>(status: StatusCode, payload: &Payload) -> Response {
    let bytes = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| server_error())
}

fn server_error() -> Response {
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

fn no_content(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .unwrap_or_else(|_| server_error())
}

/// `202 Accepted` with where the upload continues and how much of it landed.
fn accepted(base: &str, name: &str, id: &str, offset: u64) -> Response {
    // `Range` here is the inclusive span already stored, and an empty upload
    // has none, which the spec spells `0-0`.
    let range = if offset == 0 { "0-0".to_string() } else { format!("0-{}", offset - 1) };
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .header(header::LOCATION, format!("{base}/{name}/blobs/uploads/{id}"))
        .header(header::RANGE, range)
        .header(DOCKER_UPLOAD_UUID, id)
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .unwrap_or_else(|_| server_error())
}

fn range_not_satisfiable(base: &str, name: &str, id: &str, offset: u64) -> Response {
    let mut response = accepted(base, name, id, offset);
    *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
    response
}

fn created(location: &str, digest: &Digest) -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::LOCATION, location)
        .header(DOCKER_CONTENT_DIGEST, digest.to_string())
        .header(header::CONTENT_LENGTH, 0)
        .body(Body::empty())
        .unwrap_or_else(|_| server_error())
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

/// The first byte a `Content-Range: <start>-<end>` names.
///
/// Both halves are parsed, not just the first: reading only the text before
/// the hyphen would accept `0-garbage` whenever its leading number happened
/// to match the offset, letting a client advance an upload with a range that
/// means nothing.
fn parse_range_start(range: &str) -> Option<u64> {
    let (start, end) = range.trim().split_once('-')?;
    let start = start.trim().parse().ok()?;
    end.trim().parse::<u64>().ok()?;
    Some(start)
}

async fn collect_body(body: Body, limit: usize) -> Result<Bytes, Refusal> {
    axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| Refusal::new(ErrorCode::SizeInvalid, "request body is too large or truncated"))
}

/// Stream a request body onto the end of an upload, holding the whole upload
/// to [`MAX_BLOB_BYTES`]. The per-request body limit cannot do that on its
/// own: a resumable upload is many requests, each one under the limit.
///
/// An upload that runs over is dropped rather than kept truncated at the
/// ceiling, because what was sent is not a blob anyone asked for.
async fn append_body(storage: &Storage, upload: &BlobUpload, body: Body) -> Result<(), Refusal> {
    let mut written = upload.offset().await?;
    let mut writer = upload.append().await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|_| Refusal::new(ErrorCode::BlobUploadInvalid, "upload stream ended early"))?;
        let Some(next) = advance_within_ceiling(written, chunk.len()) else {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return Err(Refusal::new(
                ErrorCode::SizeInvalid,
                format!("a blob may not exceed {MAX_BLOB_BYTES} bytes"),
            ));
        };
        written = next;
        writer.write_all(&chunk).await?;
    }
    writer.finish().await?;
    Ok(())
}

/// The upload's length once `chunk` is accepted, or `None` when that would
/// take it past [`MAX_BLOB_BYTES`]. Saturating, so a length near `u64::MAX`
/// refuses rather than wrapping into an accept.
fn advance_within_ceiling(written: u64, chunk: usize) -> Option<u64> {
    let next = written.saturating_add(chunk as u64);
    (next <= MAX_BLOB_BYTES as u64).then_some(next)
}

/// Milliseconds since the Unix epoch: the ordering one tag write carries.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX))
}

/// An upload's key in the shared lock table, kept out of the package keyspace
/// so an upload and a publish of the same name never contend by accident.
fn upload_lock_key(id: &str) -> String {
    format!("oci-upload:{id}")
}

/// Hash a finished upload without holding it in memory.
async fn hash_upload(upload: &BlobUpload) -> Result<Digest, RegistryError> {
    let mut file = tokio::fs::File::open(upload.path()).await.map_err(RegistryError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_CHUNK];
    loop {
        let read = file.read(&mut buffer).await.map_err(RegistryError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Digest::parse(&format!("sha256:{:x}", hasher.finalize()))
        .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })
}

async fn read_manifest_bytes(
    storage: &Storage,
    key: &CanonicalPackageName,
    filename: &str,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let Some((body, _)) = storage.open_hosted_blob(key, filename).await? else {
        return Ok(None);
    };
    let bytes = axum::body::to_bytes(body, MAX_MANIFEST_BYTES)
        .await
        .map_err(|_| RegistryError::BadRequest { reason: "manifest is too large".to_string() })?;
    Ok(Some(bytes.to_vec()))
}

#[cfg(test)]
mod tests;
