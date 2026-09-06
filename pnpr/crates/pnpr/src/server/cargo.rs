//! The Cargo registry surface at `/cargo/`.
//!
//! Two URL families make up a Cargo registry. The **sparse index** —
//! `index/config.json` and one `index/<prefix>/<crate>` file per crate — is
//! what `cargo` resolves from; a hosted crate's file is rendered from its
//! stored [`CrateDocument`], an upstream's is proxied through the cache
//! unchanged (index files carry no URLs; the `config.json` pnpr serves points
//! downloads back at itself). The **crates API** serves downloads
//! (`api/v1/crates/<crate>/<version>/download`, verified against the index
//! checksum when proxied), accepts `cargo publish` (`PUT api/v1/crates/new`)
//! and yank / unyank on hosted registries.
//!
//! Both families answer under `/cargo/` (the default target) and
//! `/cargo/~<name>/` (a named registry). Crate names are case-insensitive:
//! the index path `cargo` requests is lowercase, so hosted documents and cache
//! entries are keyed by the lowercase name while archives keep the name as
//! published.

use super::{
    Action, AppState, AuthedCaller, RegistrySource, TargetRegistry, authorize,
    documents::{read_hosted_document, stage_hosted_artifact, store_hosted_artifact},
    ecosystem::{
        UpstreamDocument, addressed_registry, caller_scoped, is_fetchable_artifact_url,
        load_upstream_document, registry_endpoint, registry_requires_auth, serve_hosted_blob,
        serve_upstream_artifact, sha256_hex, sha256_integrity, upstream_for,
    },
    json_response, not_found,
    publishing::{PublishTarget, StagedPublish, resolve_publish_target_for},
    resolve_ecosystem_source, resolve_write_target_for,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, put},
};
use pnpr_cargo::{
    CrateDocument, IndexConfig, IndexEntry, PublishMetadata, crate_filename, download_url,
    errors_json, ok_json, parse_index, parse_publish_body, publish_ok_json, sparse_index_path,
    validate_crate_archive, validate_crate_name,
};
use pnpr_error::RegistryError;
use pnpr_package_name::{PackageName, is_safe_path_segment};
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate};
use std::{collections::HashMap, fmt::Display};

const ECOSYSTEM: Ecosystem = Ecosystem::Cargo;
/// The largest sparse-index file accepted from an upstream.
const INDEX_FILE_LIMIT: usize = 64 * 1024 * 1024;
const INDEX_CONFIG_LIMIT: usize = 64 * 1024;
/// The cache key of an upstream's `config.json`. No crate name contains a
/// `.`, so it can never collide with a crate's own entry.
const INDEX_CONFIG_KEY: &str = "config.json";

/// The Cargo routes, each registered for the default target (`/cargo/...`)
/// and for a named registry (`/cargo/~<name>/...`). A static `index` or `api`
/// segment wins over `{prefix}` at the same position, and a registry name
/// always carries its `~`, so the two forms never overlap.
pub(super) fn routes() -> Router<AppState> {
    let mut router = Router::new();
    for base in ["/cargo", "/cargo/{prefix}"] {
        router = router
            .route(&format!("{base}/index/config.json"), get(get_index_config))
            .route(&format!("{base}/index/{{a}}/{{b}}"), get(get_index_file))
            .route(&format!("{base}/index/{{a}}/{{b}}/{{c}}"), get(get_index_file))
            .route(&format!("{base}/api/v1/crates/new"), put(put_publish))
            .route(
                &format!("{base}/api/v1/crates/{{name}}/{{version}}/download"),
                get(get_download),
            )
            .route(&format!("{base}/api/v1/crates/{{name}}/{{version}}/yank"), delete(delete_yank))
            .route(&format!("{base}/api/v1/crates/{{name}}/{{version}}/unyank"), put(put_unyank));
    }
    router
}

/// A registry error in the crates API's JSON shape, so `cargo` prints the
/// detail instead of a bare status.
fn error_response(err: RegistryError) -> Response {
    let detail = err.public_message();
    let status = err.into_response().status();
    json_response(status, &errors_json(&detail))
}

fn bad_request(reason: impl Display) -> Response {
    error_response(RegistryError::BadRequest { reason: reason.to_string() })
}

/// The lowercase cache/storage key of a crate.
fn crate_key(name: &str) -> Result<PackageName, RegistryError> {
    validate_crate_name(name)
        .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })?;
    PackageName::parse(&name.to_ascii_lowercase())
}

/// `GET index/config.json`.
async fn get_index_config(
    State(state): State<AppState>,
    TargetRegistry(registry): TargetRegistry,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    let config = IndexConfig::for_registry(
        &registry_endpoint(&state, ECOSYSTEM, registry.as_deref()),
        registry_requires_auth(&state, &target, ECOSYSTEM),
    );
    let response = json_response(
        StatusCode::OK,
        &serde_json::to_value(config).expect("index config serializes"),
    );
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), None, response)
}

fn index_response(text: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(text))
        .expect("static-shape response always builds")
}

/// `GET index/<prefix>/<crate>`. The requested path must be exactly the
/// crate's sparse-index path, so a crate is reachable at one URL only.
async fn get_index_file(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let segments: Vec<&str> =
        ["a", "b", "c"].iter().filter_map(|key| params.get(*key).map(String::as_str)).collect();
    let Some(name) = segments.last().copied() else { return not_found() };
    if validate_crate_name(name).is_err() {
        return not_found();
    }
    let path = sparse_index_path(name);
    if path != segments.join("/") {
        return not_found();
    }
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    let key = match crate_key(name) {
        Ok(key) => key,
        Err(err) => return error_response(err),
    };
    let index = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, key.as_str()) {
        RegistrySource::Hosted(source) => {
            read_hosted_document::<CrateDocument>(&state, &identity, &source, &key)
                .await
                .map(|document| document.map(|document| document.render_index()))
        }
        source @ RegistrySource::Upstream(_) => {
            load_upstream_index(&state, &identity, &source, &key, &path).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    };
    let response = match index {
        Ok(Some(text)) => index_response(text),
        Ok(None) => not_found(),
        Err(err) => error_response(err),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(key.as_str()), response)
}

async fn load_upstream_index(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &PackageName,
    path: &str,
) -> Result<Option<String>, RegistryError> {
    let (upstream, namespace) = upstream_for(state, identity, source, key)?;
    let request =
        UpstreamDocument { name: key, relative_path: path, accept: None, limit: INDEX_FILE_LIMIT };
    let bytes = load_upstream_document(state, upstream, &namespace, request, |document| {
        decode_index_text(document.bytes, path).map(String::into_bytes)
    })
    .await?;
    bytes.map(|bytes| decode_index_text(bytes, path)).transpose()
}

fn decode_index_text(bytes: Vec<u8>, path: &str) -> Result<String, RegistryError> {
    String::from_utf8(bytes).map_err(|err| RegistryError::UpstreamResponse {
        url: path.to_string(),
        reason: format!("sparse index is not valid UTF-8: {err}"),
    })
}

/// `GET api/v1/crates/<crate>/<version>/download`.
async fn get_download(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let (Some(name), Some(version)) = (params.get("name"), params.get("version")) else {
        return not_found();
    };
    if validate_crate_name(name).is_err() || !is_safe_path_segment(version) {
        return not_found();
    }
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    let key = match crate_key(name) {
        Ok(key) => key,
        Err(err) => return error_response(err),
    };
    let response = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, key.as_str()) {
        RegistrySource::Hosted(source) => {
            download_hosted_crate(&state, &identity, &source, &key, version)
                .await
                .unwrap_or_else(error_response)
        }
        source @ RegistrySource::Upstream(_) => {
            download_via_upstream(&state, &identity, &source, &key, name, version).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(key.as_str()), response)
}

async fn download_hosted_crate(
    state: &AppState,
    identity: &Identity,
    source: &str,
    key: &PackageName,
    version: &str,
) -> Result<Response, RegistryError> {
    let document = read_hosted_document::<CrateDocument>(state, identity, source, key)
        .await?
        .ok_or(RegistryError::NotFound)?;
    let entry = document.version(version).ok_or(RegistryError::NotFound)?;
    let filename = crate_filename(&entry.name, &entry.vers);
    serve_hosted_blob(state, identity, source, key, &filename).await
}

/// Proxy a crate download: bind the request to the upstream index entry's
/// checksum, expand the upstream `config.json`'s `dl` template, and stream
/// the archive through the verifying cache.
async fn download_via_upstream(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &PackageName,
    name: &str,
    version: &str,
) -> Response {
    let (upstream, namespace) = match upstream_for(state, identity, source, key) {
        Ok(upstream) => upstream,
        Err(err) => return error_response(err),
    };
    let index =
        match load_upstream_index(state, identity, source, key, &sparse_index_path(name)).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return not_found(),
            Err(err) => return error_response(err),
        };
    let entries = match parse_index(&index) {
        Ok(entries) => entries,
        Err(err) => {
            return error_response(RegistryError::UpstreamResponse {
                url: sparse_index_path(name),
                reason: err.to_string(),
            });
        }
    };
    let Some(entry) =
        entries.iter().find(|entry| entry.vers == version && entry.name.eq_ignore_ascii_case(name))
    else {
        return not_found();
    };
    let Some(integrity) = sha256_integrity(&entry.cksum) else {
        return error_response(RegistryError::UpstreamResponse {
            url: sparse_index_path(name),
            reason: format!("index entry {name}@{version} has no SHA-256 checksum"),
        });
    };
    let config_key = PackageName::parse(INDEX_CONFIG_KEY).expect("static key is a safe segment");
    let request = UpstreamDocument {
        name: &config_key,
        relative_path: INDEX_CONFIG_KEY,
        accept: None,
        limit: INDEX_CONFIG_LIMIT,
    };
    let config = load_upstream_document(state, upstream, &namespace, request, |document| {
        IndexConfig::parse(&document.bytes).map(|_| document.bytes).map_err(|err| {
            RegistryError::UpstreamResponse { url: document.url, reason: err.to_string() }
        })
    })
    .await
    .and_then(|bytes| {
        let bytes = bytes.ok_or_else(|| RegistryError::UpstreamResponse {
            url: INDEX_CONFIG_KEY.to_string(),
            reason: "the upstream index has no config.json".to_string(),
        })?;
        IndexConfig::parse(&bytes).map_err(RegistryError::Json)
    });
    let config = match config {
        Ok(config) => config,
        Err(err) => return error_response(err),
    };
    let url = download_url(&config.dl, &entry.name, &entry.vers, &entry.cksum);
    if !url::Url::parse(&url).is_ok_and(|url| is_fetchable_artifact_url(&url)) {
        return error_response(RegistryError::UpstreamResponse {
            url: INDEX_CONFIG_KEY.to_string(),
            reason: "the upstream `dl` template does not produce an HTTP(S) URL".to_string(),
        });
    }
    let filename = crate_filename(&entry.name, &entry.vers);
    serve_upstream_artifact(state, upstream, &namespace, key, &filename, &url, &integrity).await
}

/// `PUT api/v1/crates/new` — `cargo publish`.
async fn put_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    body: Bytes,
) -> Response {
    let (metadata, archive) = match parse_publish_body(&body) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(err),
    };
    // The archive is the tail of the body; re-slice it so the checks below
    // can own it without copying.
    let archive = body.slice(body.len() - archive.len()..);
    let publication =
        match validate_crate_publish(&state, &identity, registry.as_deref(), metadata, archive)
            .await
        {
            Ok(publication) => publication,
            Err(err) => return error_response(err),
        };
    match publication.publish(&state).await {
        Ok(()) => json_response(StatusCode::OK, &publish_ok_json()),
        Err(err) => error_response(err),
    }
}

/// A `cargo publish` that may proceed: the caller is allowed to publish the
/// crate, the archive holds what its metadata says, and the index entry that
/// will record it is built. Publishing it is [`Self::publish`] on its own, or
/// [`Self::stage`] as one package of a cross-ecosystem batch.
pub(super) struct CratePublication {
    key: PackageName,
    org: String,
    filename: String,
    entry: IndexEntry,
    archive: Bytes,
}

/// Every check a `cargo publish` must pass before anything is written.
pub(super) async fn validate_crate_publish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    metadata: PublishMetadata,
    archive: Bytes,
) -> Result<CratePublication, RegistryError> {
    let target = authorize_crate_publish(state, identity, registry, &metadata)?;
    verify_crate_archive(target, metadata, archive).await
}

/// Where a crate publish writes, once the metadata is well-formed and the
/// caller is allowed to publish it there. Everything this decides is cheap,
/// so a caller holding an undecoded payload can settle the question before
/// spending anything on the archive.
pub(super) struct CrateTarget {
    key: PackageName,
    org: String,
}

pub(super) fn authorize_crate_publish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    metadata: &PublishMetadata,
) -> Result<CrateTarget, RegistryError> {
    metadata.validate().map_err(|err| RegistryError::BadRequest { reason: err.to_string() })?;
    let key = crate_key(&metadata.name)?;
    let (source, org) =
        match resolve_publish_target_for(state, identity, registry, ECOSYSTEM, key.as_str()) {
            PublishTarget::Hosted { source, org } => (source, org),
            PublishTarget::Reject(reason) => return Err(RegistryError::BadRequest { reason }),
            PublishTarget::Denied(err) => return Err(err),
            PublishTarget::NotFound => return Err(RegistryError::NotFound),
        };
    authorize(state, identity, &RegistrySource::Hosted(source), key.as_str(), Action::Publish)?;
    Ok(CrateTarget { key, org })
}

/// Check the archive against the metadata it was published with, and build
/// the index entry that will record it.
pub(super) async fn verify_crate_archive(
    target: CrateTarget,
    metadata: PublishMetadata,
    archive: Bytes,
) -> Result<CratePublication, RegistryError> {
    let CrateTarget { key, org } = target;
    let (name, version) = (metadata.name.clone(), metadata.vers.clone());
    let checked = tokio::task::spawn_blocking(move || {
        validate_crate_archive(&archive, &name, &version)
            .map(|()| (sha256_hex(&archive), archive))
            .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })
    })
    .await
    .map_err(RegistryError::JoinError)??;
    let (cksum, archive) = checked;
    let filename = crate_filename(&metadata.name, &metadata.vers);
    Ok(CratePublication { key, org, filename, entry: metadata.into_index_entry(cksum), archive })
}

impl CratePublication {
    pub(super) fn key(&self) -> &PackageName {
        &self.key
    }

    /// Publish this crate on its own, in a transaction of one.
    async fn publish(self, state: &AppState) -> Result<(), RegistryError> {
        store_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.filename,
            &self.archive,
            refuse_published_version(&self.entry.vers),
            CrateDocument { name: self.entry.name.clone(), versions: vec![self.entry.clone()] },
        )
        .await
    }

    /// Stage this crate as one package of a larger transaction. The caller
    /// holds the package lock and commits.
    pub(super) async fn stage(self, state: &AppState) -> Result<StagedPublish, RegistryError> {
        stage_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.filename,
            &self.archive,
            &refuse_published_version(&self.entry.vers),
            CrateDocument { name: self.entry.name.clone(), versions: vec![self.entry.clone()] },
        )
        .await
    }
}

/// A published crate version is immutable, so a document that already carries
/// `vers` is one this publish must not land on.
fn refuse_published_version(vers: &str) -> impl Fn(&CrateDocument) -> Result<(), RegistryError> {
    let vers = vers.to_string();
    move |document: &CrateDocument| match document.version(&vers) {
        Some(_) => Err(RegistryError::BadRequest {
            reason: format!("crate version `{vers}` is already uploaded"),
        }),
        None => Ok(()),
    }
}

/// `DELETE api/v1/crates/<crate>/<version>/yank`.
async fn delete_yank(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    set_yanked(&state, &identity, registry.as_deref(), &params, true).await
}

/// `PUT api/v1/crates/<crate>/<version>/unyank`.
async fn put_unyank(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    set_yanked(&state, &identity, registry.as_deref(), &params, false).await
}

/// Yanking is an owner action on crates.io, so it takes the same `publish`
/// permission a new version does.
async fn set_yanked(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    params: &HashMap<String, String>,
    yanked: bool,
) -> Response {
    let (Some(name), Some(version)) = (params.get("name"), params.get("version")) else {
        return not_found();
    };
    let key = match crate_key(name) {
        Ok(key) => key,
        Err(err) => return error_response(err),
    };
    let target = match resolve_write_target_for(state, identity, registry, ECOSYSTEM, &key) {
        Ok(target) => target,
        Err(err) => return error_response(err),
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source),
        key.as_str(),
        Action::Publish,
    ) {
        return error_response(err);
    }
    let _guard = state.inner.package_locks.lock(key.as_str()).await;
    let outcome = state
        .inner
        .storage
        .for_hosted(&target.org)
        .update_hosted_document_with_retry(&key, DOCUMENT_WRITE_RETRIES, |existing| {
            let Some(bytes) = existing else { return Ok(None) };
            let mut document = CrateDocument::parse(bytes)?;
            let Some(entry) = document.version_mut(version) else { return Ok(None) };
            entry.yanked = yanked;
            Ok(Some(document.to_bytes()))
        })
        .await;
    match outcome {
        Ok(DocumentUpdate::Written) => json_response(StatusCode::OK, &ok_json()),
        Ok(DocumentUpdate::NotFound) => not_found(),
        Err(err) => error_response(err),
    }
}
