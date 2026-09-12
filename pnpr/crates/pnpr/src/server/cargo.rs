//! The Cargo registry surface.
//!
//! Two URL families make up a Cargo registry. The **sparse index** —
//! `index/config.json` and one `index/<prefix>/<crate>` file per crate — is
//! what `cargo` resolves from; a hosted crate's file is rendered from its
//! stored [`CrateDocument`], an upstream's is proxied through the cache
//! unchanged (index files carry no URLs; the `config.json` pnpr serves points
//! downloads back at itself). The **crates API** serves downloads
//! (`api/v1/crates/<crate>/<version>/download`, verified against the index
//! checksum when proxied), accepts `cargo publish` (`PUT api/v1/crates/new`)
//! and yank / unyank on hosted registries, and answers `cargo search` over
//! the crates a registry hosts.
//!
//! In a multi-ecosystem registry, both families answer under `/cargo/` (the
//! default target) and `/cargo/~<name>/` (a named registry). A Cargo-only
//! registry omits `/cargo`. Crate names are case-insensitive:
//! the index path `cargo` requests is lowercase, so hosted documents and cache
//! entries are keyed by the lowercase name while archives keep the name as
//! published.

pub(super) use publication::{CratePublication, authorize_crate_publish, verify_crate_archive};

mod publication;
use publication::{delete_yank, put_publish, put_unyank};

use super::{
    Action, AppState, AuthedCaller, DiscoverySource, RegistrySource, SearchPage, TargetRegistry,
    authorize, discovery_sources,
    documents::{read_hosted_document, stage_hosted_artifact, store_hosted_artifact},
    ecosystem::{
        UpstreamDocument, addressed_registry, caller_scoped, is_fetchable_artifact_url,
        load_upstream_document, mount_bases, registry_endpoint, registry_requires_auth,
        serve_hosted_blob, serve_upstream_artifact, sha256_hex, sha256_integrity, upstream_for,
    },
    hosted_search_names, json_response, not_found, private_no_cache,
    publishing::{PublishTarget, StagedPublish, resolve_publish_target_for},
    resolve_ecosystem_source, resolve_write_target_for,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, RawQuery, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, put},
};
use pnpr_cargo::{
    CrateDocument, IndexConfig, IndexEntry, PublishMetadata, SearchCrate, SearchMeta,
    SearchResponse, bounded_description, crate_filename, download_url, errors_json, ok_json,
    parse_index, parse_publish_body, publish_ok_json, sparse_index_path, validate_crate_archive,
};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, is_safe_path_segment};
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

pub(super) fn routes(prefixed: bool) -> Router<AppState> {
    let mut router = Router::new();
    for base in mount_bases(ECOSYSTEM, prefixed) {
        router = router
            .route(&format!("{base}/index/config.json"), get(get_index_config))
            .route(&format!("{base}/index/{{a}}/{{b}}"), get(get_index_file))
            .route(&format!("{base}/index/{{a}}/{{b}}/{{c}}"), get(get_index_file))
            .route(&format!("{base}/api/v1/crates"), get(get_search))
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

/// `cargo search`'s default page size, which is also what crates.io returns
/// when a request names none.
const DEFAULT_SEARCH_PAGE: usize = 10;

async fn get_search(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    RawQuery(query): RawQuery,
) -> Response {
    let query_string = query.unwrap_or_default();
    let respond = |crates: Vec<SearchCrate>, total: usize| {
        json_response(
            StatusCode::OK,
            &serde_json::to_value(SearchResponse { crates, meta: SearchMeta { total } })
                .expect("search response serializes"),
        )
    };
    let Some(text) = pnpr_search::parse_query(&query_string).map(pnpr_search::SearchText::Package)
    else {
        return respond(Vec::new(), 0);
    };
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
    };
    let size = pnpr_search::parse_usize_param(&query_string, "per_page")
        .map_or(DEFAULT_SEARCH_PAGE, |size| size.clamp(1, pnpr_search::MAX_PAGE_SIZE));
    // crates.io numbers pages from one; anything lower starts at the first.
    let from = pnpr_search::parse_usize_param(&query_string, "page")
        .map_or(0, |page| page.saturating_sub(1).saturating_mul(size));

    let mut page = SearchPage::new(from, size);
    if let Err(err) = collect_hosted_crates(&state, &identity, &target, &text, &mut page).await {
        return error_response(err);
    }
    let total = page.total();
    // Results are filtered per caller (registry access plus per-package
    // ACL), so they must never land in a shared HTTP cache.
    private_no_cache(respond(page.objects, total))
}

/// Add every hosted registry's matching crates to the page.
async fn collect_hosted_crates(
    state: &AppState,
    identity: &Identity,
    target: &str,
    text: &pnpr_search::SearchText,
    page: &mut SearchPage<SearchCrate>,
) -> Result<(), RegistryError> {
    for source in discovery_sources(state, target, ECOSYSTEM) {
        let DiscoverySource::Hosted(source) = source else {
            continue;
        };
        let hosted = hosted_search_names(state, identity, target, &source, ECOSYSTEM, text).await;
        let (storage, names) = match hosted {
            Ok(Some(hosted)) => hosted,
            Ok(None) => continue,
            Err(err) => return Err(err),
        };
        add_crates_to_page(page, &storage, names).await;
    }
    Ok(())
}

/// `GET api/v1/crates?q=<query>&per_page=<n>&page=<n>` — `cargo search`.
///
/// Hosted sources only. An upstream contributes nothing, the way an npm
/// upstream does until its `search` is turned on, and searching one needs
/// its `config.json` `api` base rather than the index base pnpr proxies.
/// Add one hosted source's names to the page.
///
/// A hosted namespace shared with another ecosystem holds names that are not
/// crate names. Dropping them before the position is claimed keeps them out of
/// the page and out of the total, and costs no read.
async fn add_crates_to_page(
    page: &mut SearchPage<SearchCrate>,
    storage: &pnpr_storage::Storage,
    names: Vec<String>,
) {
    for name in names {
        let Ok(key) = CanonicalPackageName::parse(&name, ECOSYSTEM) else {
            continue;
        };
        if page.push_name(&name) {
            page.objects.push(search_crate(storage, &key).await);
        }
    }
}

/// One search row, read from the crate's stored document. A document that
/// cannot be read or parsed still answers with the name the caller matched,
/// so the page never disagrees with the total it reports.
async fn search_crate(storage: &pnpr_storage::Storage, key: &CanonicalPackageName) -> SearchCrate {
    let document = async {
        let bytes = storage.read_hosted_document(key).await.ok()??;
        CrateDocument::parse(&bytes).ok()
    }
    .await;
    document.as_ref().map_or_else(
        || SearchCrate {
            name: key.as_str().to_string(),
            description: None,
            max_version: String::new(),
        },
        CrateDocument::to_search_crate,
    )
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

/// `GET index/config.json`.
async fn get_index_config(
    State(state): State<AppState>,
    TargetRegistry(registry): TargetRegistry,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
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
    let Ok(key) = CanonicalPackageName::parse(name, ECOSYSTEM) else { return not_found() };
    let path = sparse_index_path(name);
    if path != segments.join("/") {
        return not_found();
    }
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
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
    key: &CanonicalPackageName,
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
    let Ok(key) = CanonicalPackageName::parse(name, ECOSYSTEM) else { return not_found() };
    if !is_safe_path_segment(version) {
        return not_found();
    }
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
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
    key: &CanonicalPackageName,
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
    key: &CanonicalPackageName,
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
    let entries = match parse_upstream_index(&index, name) {
        Ok(entries) => entries,
        Err(err) => return error_response(err),
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
    let config = match upstream_index_config(state, upstream, &namespace).await {
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

/// The upstream sparse index's `config.json`, through the cache.
async fn upstream_index_config(
    state: &AppState,
    upstream: &pnpr_upstream::Upstream,
    namespace: &str,
) -> Result<IndexConfig, RegistryError> {
    let config_key = CanonicalPackageName::parse(INDEX_CONFIG_KEY, Ecosystem::Npm)
        .expect("static key is a safe segment");
    let request = UpstreamDocument {
        name: &config_key,
        relative_path: INDEX_CONFIG_KEY,
        accept: None,
        limit: INDEX_CONFIG_LIMIT,
    };
    let bytes = load_upstream_document(state, upstream, namespace, request, |document| {
        IndexConfig::parse(&document.bytes).map(|_| document.bytes).map_err(|err| {
            RegistryError::UpstreamResponse { url: document.url, reason: err.to_string() }
        })
    })
    .await?
    .ok_or_else(|| RegistryError::UpstreamResponse {
        url: INDEX_CONFIG_KEY.to_string(),
        reason: "the upstream index has no config.json".to_string(),
    })?;
    IndexConfig::parse(&bytes).map_err(RegistryError::Json)
}

fn parse_upstream_index(index: &str, name: &str) -> Result<Vec<IndexEntry>, RegistryError> {
    parse_index(index).map_err(|err| RegistryError::UpstreamResponse {
        url: sparse_index_path(name),
        reason: err.to_string(),
    })
}
