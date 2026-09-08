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

pub(super) use publication::{OciPublication, authorize_publication};

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
use futures_util::StreamExt as _;
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
use sha2::{Digest as _, Sha256};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncReadExt as _;

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
    fn page_size(&self) -> Result<Option<usize>, Refusal> {
        query_param(Some(&self.query), "n")
            .map(|value| {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(Refusal::new(
                        ErrorCode::NameInvalid,
                        "n must be a non-negative integer",
                    ));
                }
                value
                    .parse::<usize>()
                    .map_err(|_| Refusal::new(ErrorCode::NameInvalid, "n is too large"))
            })
            .transpose()
    }

    fn paginate<Item: AsRef<str>>(
        &self,
        items: &mut Vec<Item>,
        endpoint: &str,
    ) -> Result<Option<String>, Refusal> {
        let count = self.page_size()?;
        if let Some(last) = query_param(Some(&self.query), "last") {
            items.retain(|item| item.as_ref() > last.as_str());
        }
        let Some(count) = count else { return Ok(None) };
        let more = items.len() > count;
        items.truncate(count);
        Ok(if more && let Some(last) = items.last() {
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("n", &count.to_string())
                .append_pair("last", last.as_ref())
                .finish();
            Some(format!(r#"<{}/{endpoint}?{query}>; rel="next""#, self.base))
        } else {
            None
        })
    }

    async fn mount_blob(
        &self,
        destination: &Storage,
        key: &CanonicalPackageName,
        mount: &str,
        from: &str,
    ) -> Result<Option<Response>, RegistryError> {
        let Ok(digest) = Digest::parse(mount) else { return Ok(None) };
        let Ok((source_key, source)) = self.hosted_source(from) else { return Ok(None) };
        if let Some(raw) = self
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(super::authentication::token_credentials)
            && let Some(claims) = tokens::decode(&self.state, &raw)?
            && !claims.allows(source_key.as_str(), "pull")
        {
            return Ok(None);
        }
        let source_org = match hosted_read_namespace(
            &self.state,
            &self.identity,
            &source,
            source_key.as_str(),
        ) {
            Ok(org) => org,
            Err(
                RegistryError::Unauthenticated { .. }
                | RegistryError::Forbidden { .. }
                | RegistryError::NotFound,
            ) => return Ok(None),
            Err(err) => return Err(err),
        };
        let source_storage = self.state.inner.storage.for_hosted(&source_org);
        let Some((body, _)) =
            source_storage.open_hosted_blob(&source_key, &digest.blob_filename()).await?
        else {
            return Ok(None);
        };
        let upload = destination.begin_blob_upload(key).await?;
        if let Err(refusal) =
            append_body(destination, &upload, body, self.state.inner.config.oci.max_blob_bytes)
                .await
        {
            destination.abort_blob_upload(upload.id()).await?;
            return Ok(Some(refusal.respond()));
        }
        Ok(Some(self.finish_upload(destination, upload, key, mount).await))
    }

    async fn referrers(&self, name: &str, digest: &str) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let Ok(digest) = Digest::parse(digest) else {
            return error(ErrorCode::DigestInvalid, "not a supported digest");
        };
        let Ok(last) =
            query_param(Some(&self.query), "last").map(|value| Digest::parse(&value)).transpose()
        else {
            return error(ErrorCode::DigestInvalid, "last must be a supported digest");
        };
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let org = match hosted_read_namespace(&self.state, &self.identity, &source, key.as_str()) {
            Ok(org) => org,
            Err(err) => return registry_error(err),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        let document =
            match read_hosted_document::<ImageDocument>(&self.state, &self.identity, &source, &key)
                .await
            {
                Ok(document) => document.unwrap_or_else(|| ImageDocument::new(key.as_str())),
                Err(err) => return registry_error(err),
            };
        let filter = ReferrerFilter::new(digest, &self.query);
        let page = match self.scan_referrers(&storage, &key, &document, &filter, last).await {
            Ok(page) => page,
            Err(response) => return response,
        };
        if let Err(response) = self.record_referrer_index(&storage, &key, &page.additions).await {
            return response;
        }
        let response = self.referrers_page_response(&key, &filter, &page);
        self.caller_scoped(Some(key.as_str()), response)
    }

    /// Walk the manifest index from `last` onwards, reading only the manifests
    /// the index cannot answer for, until the page fills up.
    async fn scan_referrers<'a>(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        document: &'a ImageDocument,
        filter: &ReferrerFilter,
        last: Option<Digest>,
    ) -> Result<ReferrerPage<'a>, Response> {
        let start = document.manifests().partition_point(|entry| {
            last.as_ref().is_some_and(|last| entry.digest.hex() <= last.hex())
        });
        let mut entries = document.manifests()[start..].iter().peekable();
        let mut page = ReferrerPage::new(self.state.inner.config.oci.max_manifest_bytes);
        // The index is migrated in place the first time a manifest is read for
        // metadata it should already carry. The re-read document is the one
        // every later entry is judged against, under a lock so two scans do
        // not migrate the same repository at once.
        let mut migration_guard = None;
        let mut migrated: Option<ImageDocument> = None;
        while let Some(&entry) = entries.peek() {
            let indexed = indexed_referrer(migrated.as_ref(), entry);
            // The lock is only held while the migration has something to
            // write; an entry the index already answers for releases it.
            if migration_guard.is_some() && indexed.flatten().is_some() && page.additions.is_empty()
            {
                drop(migration_guard.take());
            }
            match referrer_entry_step(entry, indexed, filter, &page, migration_guard.is_some()) {
                ReferrerStep::Skip => {}
                ReferrerStep::Stop => break,
                ReferrerStep::Migrate => {
                    migration_guard =
                        Some(self.state.inner.referrer_migration_locks.lock(key.as_str()).await);
                    migrated = Some(read_image_document(storage, key).await?);
                    continue;
                }
                ReferrerStep::Read { unindexed } => {
                    let manifest =
                        self.read_referrer_manifest(storage, key, entry, &mut page).await?;
                    if !page.push_referrer(entry, manifest, filter, unindexed)? {
                        break;
                    }
                }
            }
            page.cursor = Some(&entry.digest);
            entries.next();
        }
        page.more = entries.peek().is_some();
        Ok(page)
    }

    /// Read and parse one indexed manifest, charging it against the page's
    /// read budget.
    async fn read_referrer_manifest(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        entry: &ManifestEntry,
        page: &mut ReferrerPage<'_>,
    ) -> Result<Manifest, Response> {
        let read = read_manifest_bytes(
            storage,
            key,
            &entry.digest.blob_filename(),
            self.state.inner.config.oci.max_manifest_bytes,
        )
        .await;
        let bytes = match read {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                return Err(error(ErrorCode::ManifestUnknown, "a referenced manifest is missing"));
            }
            Err(err) => return Err(registry_error(err)),
        };
        page.charge_read(bytes.len() as u64);
        Manifest::parse(&bytes, Some(&entry.media_type))
            .map_err(|err| registry_error(RegistryError::Internal { reason: err.to_string() }))
    }

    /// Write back the referrer metadata this scan had to read for, so the next
    /// one answers from the index alone.
    async fn record_referrer_index(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        additions: &[ManifestEntry],
    ) -> Result<(), Response> {
        if additions.is_empty() {
            return Ok(());
        }
        let _guard = self.state.inner.package_locks.lock(key.as_str()).await;
        storage
            .update_hosted_document_with_retry(key, DOCUMENT_WRITE_RETRIES, |existing| {
                let Some(bytes) = existing else { return Ok(None) };
                let mut current = ImageDocument::parse(bytes)?;
                let mut changed = false;
                for entry in additions {
                    if current.manifest(&entry.digest).is_some_and(|held| held.referrer.is_none()) {
                        current.insert_manifest(entry.clone());
                        changed = true;
                    }
                }
                Ok(changed.then(|| current.to_bytes()))
            })
            .await
            .map_err(registry_error)?;
        Ok(())
    }

    fn referrers_page_response(
        &self,
        key: &CanonicalPackageName,
        filter: &ReferrerFilter,
        page: &ReferrerPage<'_>,
    ) -> Response {
        let manifests = page
            .referrers
            .iter()
            .map(|(entry, manifest)| ReferrerDescriptor::new(entry, manifest))
            .collect();
        let mut response = json(
            StatusCode::OK,
            &Referrers { schema_version: 2, media_type: media_type::OCI_IMAGE_INDEX, manifests },
        );
        insert_header(&mut response, "content-type", media_type::OCI_IMAGE_INDEX);
        if filter.artifact_type.is_some() {
            insert_header(&mut response, "oci-filters-applied", "artifactType");
        }
        if let Some(link) = page.next_link(&self.base, key, filter) {
            insert_header(&mut response, "link", &link);
        }
        response
    }

    /// `GET /v2/_catalog` — the repository names this caller may read.
    async fn catalog(&self) -> Response {
        if self.method != Method::GET {
            return method_not_allowed();
        }
        let Some(target) =
            addressed_registry(&self.state, self.registry.as_deref(), Ecosystem::Oci)
        else {
            return error(ErrorCode::NameUnknown, "no registry is addressed here");
        };
        match self.page_size() {
            Ok(Some(0)) => {
                return private_no_cache(json(
                    StatusCode::OK,
                    &Catalog { repositories: Vec::new() },
                ));
            }
            Ok(_) => {}
            Err(refusal) => return refusal.respond(),
        }
        let last = query_param(Some(&self.query), "last");
        let mut repositories = Vec::new();
        for source in hosted_sources(&self.state, &target, ECOSYSTEM) {
            let Some(hosted) = self.state.inner.config.hosted.get(&source) else { continue };
            let storage = self.state.inner.storage.for_hosted(&hosted.org);
            let names = match storage.hosted_package_names().await {
                Ok(names) => names,
                Err(err) => return registry_error(err),
            };
            // A listing may only name what this caller could have fetched.
            repositories.extend(names.into_iter().filter(|name| {
                last.as_ref().is_none_or(|last| name > last)
                    && CanonicalPackageName::parse(name, ECOSYSTEM).is_ok()
                    && matches!(resolve_ecosystem_source(&self.state, &target, ECOSYSTEM, name), RegistrySource::Hosted(ref resolved) if resolved == &source)
                    && authorize(
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
        let link = match self.paginate(&mut repositories, "_catalog") {
            Ok(link) => link,
            Err(refusal) => return refusal.respond(),
        };
        let mut response = json(StatusCode::OK, &Catalog { repositories });
        if let Some(link) = link {
            insert_header(&mut response, "link", &link);
        }
        private_no_cache(response)
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
                Ok(Some(document)) => {
                    let mut tags = document.tag_names();
                    let link =
                        match self.paginate(&mut tags, &format!("{}/tags/list", key.as_str())) {
                            Ok(link) => link,
                            Err(refusal) => return refusal.respond(),
                        };
                    let mut response = json(StatusCode::OK, &TagList { name: key.as_str(), tags });
                    if let Some(link) = link {
                        insert_header(&mut response, "link", &link);
                    }
                    response
                }
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
        if let Some((key, source)) = self.upstream_source(name) {
            return self.proxy_manifest(&key, &source, reference).await;
        }
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
        let bytes = match read_manifest_bytes(
            &storage,
            &key,
            &entry.digest.blob_filename(),
            self.state.inner.config.oci.max_manifest_bytes,
        )
        .await
        {
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
        let bytes = match collect_body(body, self.state.inner.config.oci.max_manifest_bytes).await {
            Ok(bytes) => bytes,
            Err(refusal) => return refusal.respond(),
        };
        let publication = match OciPublication::new(
            (key, org),
            reference.to_string(),
            bytes,
            content_type,
            self.state.inner.config.oci.max_manifest_bytes,
        ) {
            Ok(publication) => publication,
            Err(refusal) => return refusal.respond(),
        };
        let digest = publication.digest.clone();
        let key = publication.key.clone();
        let subject = publication.subject();
        let _guard = self.state.inner.package_locks.lock(key.as_str()).await;
        let staged = match publication.stage(&self.state).await {
            Ok(staged) => staged,
            Err(refusal) => return refusal.respond(),
        };
        match super::publishing::commit_publishes(&self.state, vec![staged])
            .await
            .and_then(super::publishing::report_unrecorded)
        {
            Ok(()) => {
                let mut response =
                    created(&format!("{}/{}/manifests/{digest}", self.base, key.as_str()), &digest);
                if let Some(subject) = subject {
                    insert_header(&mut response, "oci-subject", &subject.to_string());
                }
                response
            }
            Err(err) => registry_error(err),
        }
    }

    async fn delete_manifest(&self, name: &str, reference: &str) -> Response {
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let org = match hosted_read_namespace(&self.state, &self.identity, &source, key.as_str()) {
            Ok(org) => org,
            Err(err) => return registry_error(err),
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
        let storage = self.state.inner.storage.for_hosted(&org);
        let _guard = self.state.inner.package_locks.lock(key.as_str()).await;
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

    /// The byte range a `GET` asks for, when it asks for exactly one and its
    /// `If-Range` still matches.
    fn requested_download_range(&self, etag: &str) -> Option<pnpr_storage::GetRange> {
        if self.method != Method::GET
            || !self.headers.get(header::IF_RANGE).is_none_or(|value| value == etag)
            || self.headers.get_all(header::RANGE).iter().count() != 1
        {
            return None;
        }
        self.headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_download_range)
    }

    async fn read_blob(&self, name: &str, digest: &Digest) -> Response {
        if let Some((key, source)) = self.upstream_source(name) {
            return self.proxy_blob(&key, &source, digest).await;
        }
        let (key, source) = match self.hosted_source(name) {
            Ok(found) => found,
            Err(refusal) => return refusal.respond(),
        };
        let org = match hosted_read_namespace(&self.state, &self.identity, &source, key.as_str()) {
            Ok(org) => org,
            Err(err) => return registry_error(err),
        };
        let storage = self.state.inner.storage.for_hosted(&org);
        let etag = format!(r#""{digest}""#);
        if let Some(range) = self.requested_download_range(&etag) {
            let ranged =
                storage.open_hosted_blob_range(&key, &digest.blob_filename(), &range).await;
            let response = match ranged {
                Ok(Some(blob)) => ranged_blob_response(blob, digest, &etag),
                Ok(None) => return error(ErrorCode::BlobUnknown, "no such blob"),
                Err(err) => return registry_error(err),
            };
            return self.caller_scoped(Some(key.as_str()), response);
        }
        let (body, size) = match storage.open_hosted_blob(&key, &digest.blob_filename()).await {
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
        if let (Some(mount), Some(from)) =
            (query_param(Some(&self.query), "mount"), query_param(Some(&self.query), "from"))
        {
            match self.mount_blob(&storage, &key, &mount, &from).await {
                Ok(Some(response)) => return response,
                Ok(None) => {}
                Err(err) => return registry_error(err),
            }
        }
        let upload = match storage.begin_blob_upload(&key).await {
            Ok(upload) => upload,
            Err(err) => return registry_error(err),
        };
        if let Err(refusal) =
            append_body(&storage, &upload, body, self.state.inner.config.oci.max_blob_bytes).await
        {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return refusal.respond();
        }
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
        let upload = match storage.open_blob_upload(&key, id).await {
            Ok(Some(upload)) => upload,
            Ok(None) => return error(ErrorCode::BlobUploadUnknown, "no such upload"),
            Err(err) => return registry_error(err),
        };
        match self.method {
            Method::PATCH => self.append_chunk(&storage, &key, &upload, body).await,
            Method::PUT => self.complete_upload(&storage, key, upload, body).await,
            Method::GET => self.upload_progress(&key, &upload).await,
            Method::DELETE => match storage.abort_blob_upload(upload.id()).await {
                Ok(_) => no_content(StatusCode::NO_CONTENT),
                Err(err) => registry_error(err),
            },
            _ => method_not_allowed(),
        }
    }

    /// Append one chunk of an upload and report the session's new offset.
    async fn append_chunk(
        &self,
        storage: &pnpr_storage::Storage,
        key: &CanonicalPackageName,
        upload: &BlobUpload,
        body: Body,
    ) -> Response {
        if let Err(response) = self.check_chunk_start(key, upload).await {
            return response;
        }
        let appended =
            append_body(storage, upload, body, self.state.inner.config.oci.max_blob_bytes).await;
        match appended {
            Ok(()) => self.upload_progress(key, upload).await,
            Err(refusal) => refusal.respond(),
        }
    }

    /// Append the last bytes of an upload and promote it to the blob its
    /// digest names.
    async fn complete_upload(
        &self,
        storage: &pnpr_storage::Storage,
        key: CanonicalPackageName,
        upload: BlobUpload,
        body: Body,
    ) -> Response {
        let Some(digest) = self.digest.as_deref() else {
            return error(ErrorCode::DigestInvalid, "a completed upload must name its digest");
        };
        let appended =
            append_body(storage, &upload, body, self.state.inner.config.oci.max_blob_bytes).await;
        if let Err(refusal) = appended {
            return refusal.respond();
        }
        self.finish_upload(storage, upload, &key, digest).await
    }

    /// Refuse a chunk that does not continue where the upload left off, so a
    /// retry cannot silently interleave bytes.
    async fn check_chunk_start(
        &self,
        key: &CanonicalPackageName,
        upload: &BlobUpload,
    ) -> Result<(), Response> {
        let Some(range) = self.headers.get(header::CONTENT_RANGE) else { return Ok(()) };
        let Some((start, end)) = range.to_str().ok().and_then(parse_content_range) else {
            return Err(error(ErrorCode::BlobUploadInvalid, "malformed Content-Range"));
        };
        // A body of a different length than the range declares would leave
        // the upload somewhere neither side named. Checked against the
        // declared length before anything is written, rather than against
        // where the upload ended up afterwards.
        //
        // The span is computed with a ceiling rather than plain arithmetic:
        // `0-18446744073709551615` is a range a client can send, and one more
        // than it does not fit the number that holds it.
        let Some(span) = end.checked_sub(start).and_then(|span| span.checked_add(1)) else {
            return Err(error(ErrorCode::BlobUploadInvalid, "Content-Range is not a real span"));
        };
        let limit = self.state.inner.config.oci.max_blob_bytes;
        if span > limit {
            return Err(error(
                ErrorCode::SizeInvalid,
                format!("a blob may not exceed {limit} bytes"),
            ));
        }
        if let Some(declared) = self.content_length()
            && declared != span
        {
            return Err(error(
                ErrorCode::BlobUploadInvalid,
                "Content-Length disagrees with Content-Range",
            ));
        }
        let offset = upload.offset().await.map_err(registry_error)?;
        if start == offset {
            return Ok(());
        }
        // The refusal carries where the upload actually stands, so the client
        // can resume rather than start over.
        Err(range_not_satisfiable(&self.base, key.as_str(), upload.id(), offset))
    }

    fn content_length(&self) -> Option<u64> {
        self.headers.get(header::CONTENT_LENGTH)?.to_str().ok()?.trim().parse().ok()
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
        match storage.finalize_uploaded_blob(upload, key, &digest.blob_filename()).await {
            Ok(pnpr_storage::BlobFinalize::Conflict) => {
                error(ErrorCode::DigestInvalid, "stored blob conflicts with the uploaded content")
            }
            Ok(_) => created(&format!("{}/{}/blobs/{digest}", self.base, key.as_str()), &digest),
            Err(err) => registry_error(err),
        }
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Referrers<'listing> {
    schema_version: u64,
    media_type: &'listing str,
    manifests: Vec<ReferrerDescriptor<'listing>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferrerDescriptor<'listing> {
    media_type: &'listing str,
    digest: &'listing Digest,
    size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_type: Option<&'listing str>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    annotations: &'listing std::collections::BTreeMap<String, String>,
}

impl<'listing> ReferrerDescriptor<'listing> {
    fn new(entry: &'listing ManifestEntry, manifest: &'listing Manifest) -> Self {
        Self {
            media_type: &entry.media_type,
            digest: &entry.digest,
            size: entry.size,
            artifact_type: manifest.artifact_type(),
            annotations: manifest.annotations(),
        }
    }
}

fn insert_header(response: &mut Response, name: &'static str, value: &str) {
    let value = HeaderValue::from_str(value).expect("generated OCI response header is valid");
    response.headers_mut().insert(name, value);
}

/// Render a ranged blob read as its response.
fn ranged_blob_response(blob: pnpr_storage::RangedBlob, digest: &Digest, etag: &str) -> Response {
    match blob {
        pnpr_storage::RangedBlob::Read { body, range, size } => Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, range.end - range.start)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{size}", range.start, range.end - 1),
            )
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ETAG, etag)
            .header(DOCKER_CONTENT_DIGEST, digest.to_string())
            .body(body)
            .unwrap_or_else(|_| server_error()),
        pnpr_storage::RangedBlob::Unsatisfiable { size } => {
            let mut response = respond(
                StatusCode::RANGE_NOT_SATISFIABLE,
                ErrorCode::SizeInvalid,
                "range is outside the blob",
            );
            insert_header(&mut response, "content-range", &format!("bytes */{size}"));
            response
        }
    }
}

/// What a referrers query selects.
struct ReferrerFilter {
    subject: Digest,
    artifact_type: Option<String>,
    artifact_type_digest: Option<Digest>,
}

impl ReferrerFilter {
    fn new(subject: Digest, query: &str) -> Self {
        let artifact_type = query_param(Some(query), "artifactType");
        let artifact_type_digest = artifact_type.as_ref().map(|value| Digest::of(value.as_bytes()));
        Self { subject, artifact_type, artifact_type_digest }
    }

    /// Whether the manifest itself has to be read: either the index carries no
    /// metadata for it, or the metadata says it is a match and the descriptor
    /// needs the manifest to be built.
    fn needs_manifest(&self, indexed: Option<&ReferrerMetadata>) -> bool {
        indexed.is_none_or(|metadata| {
            metadata.subject.as_ref() == Some(&self.subject)
                && self
                    .artifact_type_digest
                    .as_ref()
                    .is_none_or(|filter| metadata.artifact_type_digest.as_ref() == Some(filter))
        })
    }

    fn matches(&self, manifest: &Manifest) -> bool {
        manifest.referrer_metadata().subject.as_ref() == Some(&self.subject)
            && self
                .artifact_type
                .as_deref()
                .is_none_or(|filter| manifest.artifact_type() == Some(filter))
    }
}

/// One page of a referrers scan, with the budgets that end it.
struct ReferrerPage<'a> {
    referrers: Vec<(&'a ManifestEntry, Manifest)>,
    /// Index entries this scan learned and should write back.
    additions: Vec<ManifestEntry>,
    /// The last manifest this page covered, for the `Link` cursor.
    cursor: Option<&'a Digest>,
    /// Whether manifests are left after this page.
    more: bool,
    max_manifest_bytes: usize,
    inspected: usize,
    read_bytes: u64,
    response_bytes: usize,
}

impl<'a> ReferrerPage<'a> {
    fn new(max_manifest_bytes: usize) -> Self {
        Self {
            referrers: Vec::new(),
            additions: Vec::new(),
            cursor: None,
            more: false,
            max_manifest_bytes,
            inspected: 0,
            read_bytes: 0,
            response_bytes: 128,
        }
    }

    /// Whether one more manifest of `size` bytes fits the read budget.
    fn can_read(&self, size: u64) -> bool {
        self.inspected < MAX_REFERRER_READS
            && (self.inspected == 0 || self.read_bytes + size <= MAX_REFERRER_READ_BYTES)
    }

    fn charge_read(&mut self, bytes: u64) {
        self.inspected += 1;
        self.read_bytes += bytes;
    }

    /// Take one read manifest into the page. Reports `false` when the response
    /// budget is spent and the page has to end here.
    fn push_referrer(
        &mut self,
        entry: &'a ManifestEntry,
        manifest: Manifest,
        filter: &ReferrerFilter,
        unindexed: bool,
    ) -> Result<bool, Response> {
        if unindexed {
            let mut addition = entry.clone();
            addition.referrer = Some(manifest.referrer_metadata());
            self.additions.push(addition);
        }
        if !filter.matches(&manifest) {
            return Ok(true);
        }
        let descriptor_bytes = match serde_json::to_vec(&ReferrerDescriptor::new(entry, &manifest))
        {
            Ok(bytes) => bytes.len() + 1,
            Err(err) => return Err(registry_error(err.into())),
        };
        if !self.referrers.is_empty()
            && self.response_bytes + descriptor_bytes > self.max_manifest_bytes
        {
            return Ok(false);
        }
        self.response_bytes += descriptor_bytes;
        self.referrers.push((entry, manifest));
        Ok(true)
    }

    /// The `Link` header pointing at the next page, when there is one.
    fn next_link(
        &self,
        base: &str,
        key: &CanonicalPackageName,
        filter: &ReferrerFilter,
    ) -> Option<String> {
        let cursor = self.more.then_some(self.cursor)??;
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("last", &cursor.to_string());
        if let Some(artifact_type) = filter.artifact_type.as_deref() {
            query.append_pair("artifactType", artifact_type);
        }
        Some(format!(
            r#"<{base}/{}/referrers/{}?{}>; rel="next""#,
            key.as_str(),
            filter.subject,
            query.finish(),
        ))
    }
}

/// What the scan does with one indexed manifest.
enum ReferrerStep {
    /// Nothing here answers the query; move on.
    Skip,
    /// Read the manifest. `unindexed` says the index carried no metadata for
    /// it, so what is read has to be written back.
    Read { unindexed: bool },
    /// Re-read the document under the migration lock and judge this entry
    /// against it.
    Migrate,
    /// The page's read or response budget is spent.
    Stop,
}

fn referrer_entry_step(
    entry: &ManifestEntry,
    indexed: Option<Option<&ReferrerMetadata>>,
    filter: &ReferrerFilter,
    page: &ReferrerPage<'_>,
    migrating: bool,
) -> ReferrerStep {
    let Some(indexed) = indexed else {
        return ReferrerStep::Skip;
    };
    if !filter.needs_manifest(indexed) {
        return ReferrerStep::Skip;
    }
    if !page.can_read(entry.size) {
        return ReferrerStep::Stop;
    }
    if indexed.is_none() && !migrating {
        return ReferrerStep::Migrate;
    }
    ReferrerStep::Read { unindexed: indexed.is_none() }
}

/// The referrer metadata the index holds for one manifest, or `None` when a
/// migrated document no longer lists it at all.
fn indexed_referrer<'a>(
    migrated: Option<&'a ImageDocument>,
    entry: &'a ManifestEntry,
) -> Option<Option<&'a ReferrerMetadata>> {
    let Some(migrated) = migrated else {
        return Some(entry.referrer.as_ref());
    };
    Some(migrated.manifest(&entry.digest)?.referrer.as_ref())
}

/// Read the repository's image document as it stands now.
async fn read_image_document(
    storage: &pnpr_storage::Storage,
    key: &CanonicalPackageName,
) -> Result<ImageDocument, Response> {
    match storage.read_hosted_document(key).await {
        Ok(Some(bytes)) => ImageDocument::parse(&bytes).map_err(|err| registry_error(err.into())),
        Ok(None) => Ok(ImageDocument::new(key.as_str())),
        Err(err) => Err(registry_error(err)),
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
pub(super) struct Refusal {
    status: StatusCode,
    code: ErrorCode,
    message: String,
    original: Option<Box<RegistryError>>,
}

impl Refusal {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { status: status_for(code), code, message: message.into(), original: None }
    }

    pub(super) fn respond(self) -> Response {
        let status = self.original.map_or(self.status, |err| err.into_response().status());
        respond(status, self.code, self.message)
    }
}

/// A pnpr error in the distribution spec's own vocabulary, keeping the status
/// the error chose.
impl From<RegistryError> for Refusal {
    fn from(err: RegistryError) -> Self {
        let upload_conflict = matches!(err, RegistryError::BlobUploadConflict { .. });
        let message = err.public_message();
        let status = err.status_code();
        let code = match status {
            StatusCode::CONFLICT if upload_conflict => ErrorCode::BlobUploadInvalid,
            StatusCode::UNAUTHORIZED => ErrorCode::Unauthorized,
            StatusCode::FORBIDDEN => ErrorCode::Denied,
            StatusCode::NOT_FOUND => ErrorCode::NameUnknown,
            StatusCode::TOO_MANY_REQUESTS => ErrorCode::TooManyRequests,
            // No spec code means "the registry refused this"; the status is
            // what a client acts on.
            _ => ErrorCode::Unsupported,
        };
        Self { status, code, message, original: Some(Box::new(err)) }
    }
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

fn registry_error(err: RegistryError) -> Response {
    Refusal::from(err).respond()
}

pub(super) fn error(code: ErrorCode, message: impl Into<String>) -> Response {
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

/// The inclusive bounds a `Content-Range: <start>-<end>` names.
///
/// Both are parsed and kept: reading only the text before the hyphen would
/// accept `0-garbage` whenever its leading number happened to match the
/// offset, and discarding the end would accept `5-2`. Either lets a client
/// advance an upload under a range that means nothing.
fn parse_content_range(range: &str) -> Option<(u64, u64)> {
    let (start, end) = range.trim().split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = end.trim().parse().ok()?;
    (start <= end).then_some((start, end))
}

async fn collect_body(body: Body, limit: usize) -> Result<Bytes, Refusal> {
    axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| Refusal::new(ErrorCode::SizeInvalid, "request body is too large or truncated"))
}

/// Stream a request body onto the end of an upload, holding the whole upload
/// to the configured byte limit. The per-request body limit cannot do that on its
/// own: a resumable upload is many requests, each one under the limit.
///
/// An upload that runs over is dropped rather than kept truncated at the
/// ceiling, because what was sent is not a blob anyone asked for.
///
/// A stream that ends early is kept instead. The bytes written are an ordered
/// prefix of the blob, which is the state a resumable upload exists to hold:
/// discarding it would cost a client the whole of a multi-gigabyte layer for
/// one dropped connection. The refusal a later chunk gets carries where the
/// upload actually stands, so the client resumes from the prefix, and the
/// digest check at `PUT` is what decides whether the assembled bytes are the
/// blob that was promised.
async fn append_body(
    storage: &Storage,
    upload: &BlobUpload,
    body: Body,
    limit: u64,
) -> Result<(), Refusal> {
    let mut written = upload.offset().await?;
    if written > limit {
        storage.abort_blob_upload(upload.id()).await?;
        return Err(Refusal::new(
            ErrorCode::SizeInvalid,
            format!("a blob may not exceed {limit} bytes"),
        ));
    }
    let mut writer = upload.append().await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            writer.finish().await?;
            return Err(Refusal::new(ErrorCode::BlobUploadInvalid, "upload stream ended early"));
        };
        let Some(next) = advance_within_ceiling(written, chunk.len(), limit) else {
            let _ = storage.abort_blob_upload(upload.id()).await;
            return Err(Refusal::new(
                ErrorCode::SizeInvalid,
                format!("a blob may not exceed {limit} bytes"),
            ));
        };
        written = next;
        writer.write_all(&chunk).await?;
    }
    writer.finish().await?;
    Ok(())
}

/// The upload's length once `chunk` is accepted, or `None` when that would
/// take it past the configured byte limit. Overflow refuses the chunk.
fn advance_within_ceiling(written: u64, chunk: usize, limit: u64) -> Option<u64> {
    let next = written.checked_add(chunk as u64)?;
    (next <= limit).then_some(next)
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
    upload.materialize().await?;
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
    limit: usize,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let Some((body, _)) = storage.open_hosted_blob(key, filename).await? else {
        return Ok(None);
    };
    let bytes = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| RegistryError::BadRequest { reason: "manifest is too large".to_string() })?;
    Ok(Some(bytes.to_vec()))
}

#[cfg(test)]
mod tests;
