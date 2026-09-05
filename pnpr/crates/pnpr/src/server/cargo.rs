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
    Action, AppState, AuthedCaller, RegistrySource, TargetRegistry, authorize, authorized_upstream,
    cached_upstream_tarball,
    ecosystem::{
        UpstreamDocument, addressed_registry, caller_scoped, is_fetchable_artifact_url,
        load_upstream_document, registry_endpoint, registry_requires_auth, serve_upstream_artifact,
        sha256_hex, sha256_integrity,
    },
    hosted_read_namespace, json_response, not_found,
    publishing::{PublishTarget, resolve_publish_target_for},
    resolve_ecosystem_source, resolve_write_target_for, tarball_response, upstream_cache_namespace,
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
    CrateDocument, IndexConfig, crate_filename, download_url, errors_json, ok_json, parse_index,
    parse_publish_body, publish_ok_json, sparse_index_path, validate_crate_archive,
    validate_crate_name,
};
use pnpr_error::RegistryError;
use pnpr_package_name::{PackageName, is_safe_path_segment};
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_storage::{PACKUMENT_WRITE_RETRIES, PackumentUpdate};
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
    let response = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, key.as_str()) {
        RegistrySource::Hosted(source) => {
            match read_hosted_document(&state, &identity, &source, &key).await {
                Ok(Some(document)) => index_response(document.render_index()),
                Ok(None) => not_found(),
                Err(err) => error_response(err),
            }
        }
        source @ RegistrySource::Upstream(_) => {
            match load_upstream_index(&state, &identity, &source, &key, &path).await {
                Ok(Some(bytes)) => index_response(String::from_utf8_lossy(&bytes).into_owned()),
                Ok(None) => not_found(),
                Err(err) => error_response(err),
            }
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(key.as_str()), response)
}

async fn read_hosted_document(
    state: &AppState,
    identity: &Identity,
    source: &str,
    key: &PackageName,
) -> Result<Option<CrateDocument>, RegistryError> {
    let org = hosted_read_namespace(state, identity, source, key.as_str())?;
    state
        .inner
        .storage
        .for_hosted(&org)
        .read_hosted_packument(key)
        .await?
        .map(|bytes| CrateDocument::parse(&bytes).map_err(RegistryError::Json))
        .transpose()
}

/// The upstream behind `source`, authorized for `identity` to read `key`,
/// with its cache namespace.
fn upstream_for<'state>(
    state: &'state AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &PackageName,
) -> Result<(&'state pnpr_upstream::Upstream, String), RegistryError> {
    let RegistrySource::Upstream(name) = source else {
        return Err(RegistryError::NotFound);
    };
    authorize(state, identity, source, key.as_str(), Action::Access)?;
    let upstream = authorized_upstream(state, identity, name)?;
    Ok((upstream, upstream_cache_namespace(state, name)))
}

async fn load_upstream_index(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &PackageName,
    path: &str,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let (upstream, namespace) = upstream_for(state, identity, source, key)?;
    let request =
        UpstreamDocument { name: key, relative_path: path, accept: None, limit: INDEX_FILE_LIMIT };
    load_upstream_document(state, upstream, &namespace, request, |document| Ok(document.bytes))
        .await
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
    let filename = crate_filename(name, version);
    let response = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, key.as_str()) {
        RegistrySource::Hosted(source) => {
            match hosted_read_namespace(&state, &identity, &source, key.as_str()) {
                Ok(org) => {
                    let storage = state.inner.storage.for_hosted(&org);
                    match storage.open_hosted_tarball(&key, &filename).await {
                        Ok(Some((body, len))) => tarball_response(body, len),
                        Ok(None) => not_found(),
                        Err(err) => error_response(err),
                    }
                }
                Err(err) => error_response(err),
            }
        }
        source @ RegistrySource::Upstream(_) => {
            download_via_upstream(&state, &identity, &source, &key, name, version).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(key.as_str()), response)
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
    let filename = crate_filename(name, version);
    if upstream.caches()
        && let Some(response) = cached_upstream_tarball(state, &namespace, key, &filename).await
    {
        return response;
    }
    let index =
        match load_upstream_index(state, identity, source, key, &sparse_index_path(name)).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return not_found(),
            Err(err) => return error_response(err),
        };
    let entries = match parse_index(&String::from_utf8_lossy(&index)) {
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
    if let Err(err) = metadata.validate() {
        return bad_request(err);
    }
    // The archive is the tail of the body; re-slice it so the check below
    // can own it without copying.
    let archive = body.slice(body.len() - archive.len()..);
    let key = match crate_key(&metadata.name) {
        Ok(key) => key,
        Err(err) => return error_response(err),
    };
    let target =
        resolve_publish_target_for(&state, &identity, registry.as_deref(), ECOSYSTEM, key.as_str());
    let (source, org) = match target {
        PublishTarget::Hosted { source, org } => (source, org),
        PublishTarget::Reject(reason) => return bad_request(reason),
        PublishTarget::Denied(err) => return error_response(err),
        PublishTarget::NotFound => return not_found(),
    };
    if let Err(err) =
        authorize(&state, &identity, &RegistrySource::Hosted(source), key.as_str(), Action::Publish)
    {
        return error_response(err);
    }
    let (name, version) = (metadata.name.clone(), metadata.vers.clone());
    let checked = tokio::task::spawn_blocking(move || {
        validate_crate_archive(&archive, &name, &version)
            .map(|()| (sha256_hex(&archive), archive))
            .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })
    })
    .await;
    let (cksum, archive) = match checked {
        Ok(Ok(checked)) => checked,
        Ok(Err(err)) => return error_response(err),
        Err(err) => return error_response(RegistryError::JoinError(err)),
    };
    let filename = crate_filename(&metadata.name, &metadata.vers);
    let version = metadata.vers.clone();
    let entry = metadata.into_index_entry(cksum);

    // Serialize against other writers of this crate on this instance so two
    // publishes cannot both pass the duplicate check.
    let _guard = state.inner.package_locks.lock(key.as_str()).await;
    let storage = state.inner.storage.for_hosted(&org);
    let already_published = || RegistryError::BadRequest {
        reason: format!("crate version `{version}` is already uploaded"),
    };
    match storage.read_hosted_packument(&key).await {
        Ok(Some(bytes)) => match CrateDocument::parse(&bytes) {
            Ok(existing) if existing.version(&version).is_some() => {
                return error_response(already_published());
            }
            Ok(_) => {}
            Err(err) => return error_response(RegistryError::Json(err)),
        },
        Ok(None) => {}
        Err(err) => return error_response(err),
    }
    let written = async {
        let slot = storage.reserve_hosted_tarball(&key, &filename).await?;
        tokio::fs::write(&slot.tmp_path, &archive).await?;
        storage.finalize_tarball_slot(slot).await?;
        storage
            .update_hosted_packument_with_retry(&key, PACKUMENT_WRITE_RETRIES, |existing| {
                let mut document = match existing {
                    Some(bytes) => CrateDocument::parse(bytes)?,
                    None => CrateDocument::new(&entry.name),
                };
                if document.version(&entry.vers).is_some() {
                    return Err(already_published());
                }
                document.versions.push(entry.clone());
                Ok(Some(document.to_bytes()))
            })
            .await
    };
    match written.await {
        Ok(_) => json_response(StatusCode::OK, &publish_ok_json()),
        Err(err) => error_response(err),
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
        .update_hosted_packument_with_retry(&key, PACKUMENT_WRITE_RETRIES, |existing| {
            let Some(bytes) = existing else { return Ok(None) };
            let mut document = CrateDocument::parse(bytes)?;
            let Some(entry) = document.version_mut(version) else { return Ok(None) };
            entry.yanked = yanked;
            Ok(Some(document.to_bytes()))
        })
        .await;
    match outcome {
        Ok(PackumentUpdate::Written) => json_response(StatusCode::OK, &ok_json()),
        Ok(PackumentUpdate::NotFound) => not_found(),
        Err(err) => error_response(err),
    }
}
