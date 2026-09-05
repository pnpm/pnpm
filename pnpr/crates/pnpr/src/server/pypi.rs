//! The Python package index surface at `/pypi/`.
//!
//! The **Simple Repository API** — `simple/` (the project list) and
//! `simple/<project>/` (a project's files) — is served as PEP 691 JSON or
//! PEP 503 HTML by `Accept`. A hosted project page is rendered from its
//! stored [`ProjectDocument`]; an upstream's is read as JSON, cached beside
//! the URL it came from, and re-rendered with every file URL pointing back at
//! `files/<project>/<filename>`, so downloads flow through pnpr and are
//! verified against the page's SHA-256 on the way into the cache. The
//! **legacy upload API** (`POST legacy/`) accepts what `twine upload` sends
//! into a hosted registry.
//!
//! Every endpoint answers under `/pypi/` (the default target) and
//! `/pypi/~<name>/` (a named registry). Project names are compared normalized
//! (PEP 503); a page requested under a non-normalized spelling redirects to
//! the normalized URL, as pypi.org does.

use super::{
    Action, AppState, AuthedCaller, HostedGate, RegistrySource, TargetRegistry, authorize,
    authorized_upstream, cached_upstream_tarball,
    ecosystem::{
        UpstreamDocument, addressed_registry, caller_scoped, hosted_sources,
        is_fetchable_artifact_url, load_upstream_document, registry_endpoint,
        serve_upstream_artifact, sha256_hex, sha256_integrity,
    },
    hosted_gate, hosted_read_namespace, not_found,
    publishing::{PublishTarget, resolve_publish_target_for},
    resolve_ecosystem_source, tarball_response, upstream_cache_namespace,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use pnpr_error::RegistryError;
use pnpr_package_name::{PackageName, is_safe_path_segment};
use pnpr_policy::Identity;
use pnpr_pypi::{
    DistributionKind, FILES_PATH, HTML_CONTENT_TYPE, JSON_CONTENT_TYPE, ProjectDocument,
    ProjectFile, SIMPLE_PATH, UPLOAD_PATH, Yanked, multipart, normalize_name, normalize_version,
    parse_distribution_filename, parse_upload, render_project_list_html, render_project_list_json,
    wants_json, wants_versioned_html,
};
use pnpr_registry::Ecosystem;
use pnpr_storage::{PACKUMENT_WRITE_RETRIES, publish::now_iso};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const ECOSYSTEM: Ecosystem = Ecosystem::Pypi;
/// The largest Simple API page accepted from an upstream.
const PAGE_LIMIT: usize = 64 * 1024 * 1024;

/// The Python routes, each registered for the default target (`/pypi/...`)
/// and for a named registry (`/pypi/~<name>/...`), with and without the
/// trailing slash the Simple API spells its URLs with.
pub(super) fn routes() -> Router<AppState> {
    let mut router = Router::new();
    for base in ["/pypi", "/pypi/{prefix}"] {
        router = router
            .route(&format!("{base}/{SIMPLE_PATH}"), get(get_project_list))
            .route(&format!("{base}/{SIMPLE_PATH}/"), get(get_project_list))
            .route(&format!("{base}/{SIMPLE_PATH}/{{project}}"), get(get_project_page))
            .route(&format!("{base}/{SIMPLE_PATH}/{{project}}/"), get(get_project_page))
            .route(&format!("{base}/{FILES_PATH}/{{project}}/{{filename}}"), get(get_file))
            .route(&format!("{base}/{UPLOAD_PATH}"), post(post_upload))
            .route(&format!("{base}/{UPLOAD_PATH}/"), post(post_upload));
    }
    router
}

/// An upstream project page as cached: the JSON body beside the URL it was
/// served from, which its relative file URLs resolve against.
#[derive(Serialize, Deserialize)]
struct CachedPage {
    url: String,
    body: Box<RawValue>,
}

fn bad_request(reason: impl std::fmt::Display) -> RegistryError {
    RegistryError::BadRequest { reason: reason.to_string() }
}

fn html_response(headers: &HeaderMap, html: String) -> Response {
    let accept = headers.get(header::ACCEPT).and_then(|value| value.to_str().ok());
    let content_type =
        if wants_versioned_html(accept) { HTML_CONTENT_TYPE } else { "text/html; charset=utf-8" };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(html))
        .expect("static-shape response always builds")
}

fn json_page_response(json: &serde_json::Value) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, JSON_CONTENT_TYPE)
        .body(Body::from(serde_json::to_vec(json).expect("static-shape JSON serializes")))
        .expect("static-shape response always builds")
}

fn accepts_json(headers: &HeaderMap) -> bool {
    wants_json(headers.get(header::ACCEPT).and_then(|value| value.to_str().ok()))
}

/// `GET simple/` — every hosted project the caller may read through the
/// addressed registry. Upstream sources are not enumerated: an upstream
/// index's full project list is not something installers ask for, and
/// pypi.org's runs to hundreds of thousands of names.
async fn get_project_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    let mut names = BTreeSet::new();
    for source in hosted_sources(&state, &target, ECOSYSTEM) {
        let Some(hosted) = state.inner.config.hosted.get(&source) else { continue };
        let listed = match state.inner.storage.for_hosted(&hosted.org).hosted_package_names().await
        {
            Ok(listed) => listed,
            Err(err) => return err.into_response(),
        };
        for name in listed {
            let routed_here = matches!(
                resolve_ecosystem_source(&state, &target, ECOSYSTEM, &name),
                RegistrySource::Hosted(selected) if selected == source,
            );
            if routed_here
                && matches!(hosted_gate(&state, &identity, &source, &name), HostedGate::Allowed(_))
            {
                names.insert(name);
            }
        }
    }
    let response = if accepts_json(&headers) {
        json_page_response(&render_project_list_json(names.iter().map(String::as_str)))
    } else {
        let simple_base =
            format!("{}/{SIMPLE_PATH}", registry_endpoint(&state, ECOSYSTEM, registry.as_deref()));
        html_response(
            &headers,
            render_project_list_html(&simple_base, names.iter().map(String::as_str)),
        )
    };
    // The list is filtered per caller, so it is never shareable.
    super::private_no_cache(response)
}

/// `GET simple/<project>/`.
async fn get_project_page(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let Some(raw_project) = params.get("project") else { return not_found() };
    let Ok(project) = normalize_name(raw_project) else { return not_found() };
    let endpoint = registry_endpoint(&state, ECOSYSTEM, registry.as_deref());
    if project != *raw_project {
        return Response::builder()
            .status(StatusCode::MOVED_PERMANENTLY)
            .header(header::LOCATION, format!("{endpoint}/{SIMPLE_PATH}/{project}/"))
            .body(Body::empty())
            .expect("static-shape response always builds");
    }
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    let key = match PackageName::parse(&project) {
        Ok(key) => key,
        Err(err) => return err.into_response(),
    };
    let document = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, &project) {
        RegistrySource::Hosted(source) => {
            read_hosted_document(&state, &identity, &source, &key).await
        }
        source @ RegistrySource::Upstream(_) => {
            load_upstream_page(&state, &identity, &source, &key, &project)
                .await
                .map(|page| page.map(|(document, _)| document))
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(None),
    };
    let response = match document {
        Ok(Some(document)) => {
            let file_base = format!("{endpoint}/{FILES_PATH}/{project}");
            if accepts_json(&headers) {
                json_page_response(&document.render_json(&file_base))
            } else {
                html_response(&headers, document.render_html(&file_base))
            }
        }
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(&project), response)
}

async fn read_hosted_document(
    state: &AppState,
    identity: &Identity,
    source: &str,
    key: &PackageName,
) -> Result<Option<ProjectDocument>, RegistryError> {
    let org = hosted_read_namespace(state, identity, source, key.as_str())?;
    state
        .inner
        .storage
        .for_hosted(&org)
        .read_hosted_packument(key)
        .await?
        .map(|bytes| ProjectDocument::parse(&bytes).map_err(RegistryError::Json))
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

/// An upstream project's page through the cache, as the parsed document plus
/// the URL its file URLs resolve against.
async fn load_upstream_page(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &PackageName,
    project: &str,
) -> Result<Option<(ProjectDocument, url::Url)>, RegistryError> {
    let (upstream, namespace) = upstream_for(state, identity, source, key)?;
    let relative_path = format!("{project}/");
    let request = UpstreamDocument {
        name: key,
        relative_path: &relative_path,
        accept: Some(JSON_CONTENT_TYPE),
        limit: PAGE_LIMIT,
    };
    let Some(bytes) = load_upstream_document(state, upstream, &namespace, request, |document| {
        let body = serde_json::from_slice::<Box<RawValue>>(&document.bytes).map_err(|_| {
            RegistryError::UpstreamResponse {
                url: document.url.clone(),
                reason: "the upstream index must support the Simple JSON API (PEP 691)".to_string(),
            }
        })?;
        Ok(serde_json::to_vec(&CachedPage { url: document.url, body })?)
    })
    .await?
    else {
        return Ok(None);
    };
    let page: CachedPage = serde_json::from_slice(&bytes)?;
    let document = ProjectDocument::parse(page.body.get().as_bytes())?;
    let base = url::Url::parse(&page.url).map_err(|err| RegistryError::UpstreamResponse {
        url: page.url.clone(),
        reason: format!("cached page URL is invalid: {err}"),
    })?;
    Ok(Some((document, base)))
}

/// `GET files/<project>/<filename>`.
async fn get_file(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let (Some(raw_project), Some(filename)) = (params.get("project"), params.get("filename"))
    else {
        return not_found();
    };
    let Ok(project) = normalize_name(raw_project) else { return not_found() };
    if !is_safe_path_segment(filename) {
        return not_found();
    }
    let Some(target) = addressed_registry(&state, registry.as_deref()) else {
        return not_found();
    };
    let key = match PackageName::parse(&project) {
        Ok(key) => key,
        Err(err) => return err.into_response(),
    };
    let response = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, &project) {
        RegistrySource::Hosted(source) => {
            match hosted_read_namespace(&state, &identity, &source, &project) {
                Ok(org) => {
                    let storage = state.inner.storage.for_hosted(&org);
                    match storage.open_hosted_tarball(&key, filename).await {
                        Ok(Some((body, len))) => tarball_response(body, len),
                        Ok(None) => not_found(),
                        Err(err) => err.into_response(),
                    }
                }
                Err(err) => err.into_response(),
            }
        }
        source @ RegistrySource::Upstream(_) => {
            file_via_upstream(&state, &identity, &source, &key, &project, filename).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(&project), response)
}

/// Proxy a file download: bind the request to the page's entry for the
/// filename (its origin URL and SHA-256) and stream it through the
/// verifying cache.
async fn file_via_upstream(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &PackageName,
    project: &str,
    filename: &str,
) -> Response {
    let (upstream, namespace) = match upstream_for(state, identity, source, key) {
        Ok(upstream) => upstream,
        Err(err) => return err.into_response(),
    };
    if upstream.caches()
        && let Some(response) = cached_upstream_tarball(state, &namespace, key, filename).await
    {
        return response;
    }
    let (document, base) = match load_upstream_page(state, identity, source, key, project).await {
        Ok(Some(page)) => page,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let Some(entry) = document.file(filename) else { return not_found() };
    let bad_entry = |reason: String| {
        RegistryError::UpstreamResponse { url: base.to_string(), reason }.into_response()
    };
    let Some(origin) = entry.url.as_deref() else {
        return bad_entry(format!("file {filename} has no URL"));
    };
    let url = match base.join(origin) {
        Ok(url) if is_fetchable_artifact_url(&url) => url,
        _ => return bad_entry(format!("file {filename} has no fetchable HTTP(S) URL")),
    };
    let Some(integrity) = entry.sha256().and_then(sha256_integrity) else {
        return bad_entry(format!("file {filename} has no SHA-256 hash"));
    };
    serve_upstream_artifact(state, upstream, &namespace, key, filename, url.as_str(), &integrity)
        .await
}

/// `POST legacy/` — the legacy upload API `twine` speaks.
async fn post_upload(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response = match upload_file(&state, &identity, registry.as_deref(), &headers, &body).await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    };
    super::private_no_cache(response)
}

async fn upload_file(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), RegistryError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| bad_request("request body must be multipart/form-data"))?;
    let parts = multipart::parse_form(content_type, body).map_err(bad_request)?;
    let upload = parse_upload(parts).map_err(bad_request)?;
    let project = normalize_name(&upload.name).map_err(bad_request)?;
    let version = normalize_version(&upload.version).map_err(bad_request)?;
    let distribution = parse_distribution_filename(&upload.filename).map_err(bad_request)?;
    if distribution.name != project {
        return Err(bad_request(format!(
            "filename {:?} does not belong to project {project:?}",
            upload.filename,
        )));
    }
    if normalize_version(&distribution.version).map_err(bad_request)? != version {
        return Err(bad_request(format!(
            "filename {:?} does not carry version {version:?}",
            upload.filename,
        )));
    }
    match (upload.filetype.as_str(), distribution.kind) {
        ("bdist_wheel", DistributionKind::Wheel) | ("sdist", DistributionKind::Sdist) => {}
        (filetype, _) => {
            return Err(bad_request(format!(
                "filetype {filetype:?} does not match the filename {:?}",
                upload.filename,
            )));
        }
    }
    let key = PackageName::parse(&project)?;
    let (source, org) =
        match resolve_publish_target_for(state, identity, registry, ECOSYSTEM, &project) {
            PublishTarget::Hosted { source, org } => (source, org),
            PublishTarget::Reject(reason) => return Err(RegistryError::BadRequest { reason }),
            PublishTarget::Denied(err) => return Err(err),
            PublishTarget::NotFound => return Err(RegistryError::NotFound),
        };
    authorize(state, identity, &RegistrySource::Hosted(source), &project, Action::Publish)?;
    let sha256 = sha256_hex(&upload.content);
    if upload.sha256_digest.as_deref().is_some_and(|declared| declared != sha256) {
        return Err(bad_request("sha256_digest does not match the uploaded file"));
    }
    let entry = ProjectFile {
        filename: upload.filename.clone(),
        url: None,
        hashes: BTreeMap::from([("sha256".to_string(), sha256)]),
        requires_python: upload.requires_python,
        yanked: Yanked::Flag(false),
        size: Some(upload.content.len() as u64),
        upload_time: Some(now_iso()),
    };
    let already_exists = || RegistryError::BadRequest {
        reason: format!("File already exists: {:?}", upload.filename),
    };

    // Serialize against other writers of this project on this instance so two
    // uploads cannot both pass the duplicate check.
    let _guard = state.inner.package_locks.lock(key.as_str()).await;
    let storage = state.inner.storage.for_hosted(&org);
    if let Some(bytes) = storage.read_hosted_packument(&key).await?
        && ProjectDocument::parse(&bytes)?.file(&upload.filename).is_some()
    {
        return Err(already_exists());
    }
    let slot = storage.reserve_hosted_tarball(&key, &upload.filename).await?;
    tokio::fs::write(&slot.tmp_path, &upload.content).await?;
    storage.finalize_tarball_slot(slot).await?;
    storage
        .update_hosted_packument_with_retry(&key, PACKUMENT_WRITE_RETRIES, |existing| {
            let mut document = match existing {
                Some(bytes) => ProjectDocument::parse(bytes)?,
                None => ProjectDocument::new(&project),
            };
            if document.file(&entry.filename).is_some() {
                return Err(already_exists());
            }
            document.files.push(entry.clone());
            Ok(Some(document.to_bytes()))
        })
        .await?;
    Ok(())
}
