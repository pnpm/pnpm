//! The Python package index surface.
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
//! In a multi-ecosystem registry, every endpoint answers under `/pypi/` (the
//! default target) and `/pypi/~<name>/` (a named registry). A Python-only
//! registry omits `/pypi`. Project names are compared normalized (PEP 503); a
//! page requested under a non-normalized spelling redirects to the normalized
//! URL, as pypi.org does.

pub(super) use publication::{PypiPublication, authorize_upload, verify_upload};

mod publication;
use publication::post_upload;

use super::{
    Action, AppState, AuthedCaller, HostedGate, RegistrySource, TargetRegistry, authorize,
    documents::{read_hosted_document, stage_hosted_artifact, store_hosted_artifact},
    ecosystem::{
        UpstreamDocument, addressed_registry, caller_scoped, hosted_sources,
        is_fetchable_artifact_url, load_upstream_document, mount_bases, registry_endpoint,
        serve_hosted_blob, serve_upstream_artifact, sha256_hex, sha256_integrity, upstream_for,
    },
    hosted_gate, not_found, private_no_cache,
    publishing::{PublishTarget, StagedPublish, resolve_publish_target_for},
    resolve_ecosystem_source,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, is_safe_path_segment};
use pnpr_policy::Identity;
use pnpr_pypi::{
    DistributionKind, FILES_PATH, HTML_CONTENT_TYPE, JSON_CONTENT_TYPE, ProjectDocument,
    ProjectFile, SIMPLE_PATH, UPLOAD_PATH, Upload, Yanked, multipart, normalize_version,
    parse_distribution_filename, parse_upload, render_project_list_html, render_project_list_json,
    wants_json, wants_versioned_html,
};
use pnpr_registry::Ecosystem;
use pnpr_storage::publish::now_iso;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const ECOSYSTEM: Ecosystem = Ecosystem::Pypi;
/// The largest Simple API page accepted from an upstream.
const PAGE_LIMIT: usize = 64 * 1024 * 1024;

pub(super) fn routes(prefixed: bool) -> Router<AppState> {
    let mut router = Router::new();
    for base in mount_bases(ECOSYSTEM, prefixed) {
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

/// One listed name, and the hosted source it was listed from.
struct VisibleName<'a> {
    source: &'a str,
    name: &'a str,
}

async fn get_project_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
    };
    let names = match visible_project_names(&state, &identity, &target).await {
        Ok(names) => names,
        Err(err) => return err.into_response(),
    };
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
    private_no_cache(response)
}

/// `GET simple/` — every hosted project the caller may read through the
/// addressed registry. Upstream sources are not enumerated: an upstream
/// index's full project list is not something installers ask for, and
/// pypi.org's runs to hundreds of thousands of names.
/// Every hosted project of the target registry this caller may see, from the
/// sources the registry routes to.
async fn visible_project_names(
    state: &AppState,
    identity: &Identity,
    target: &str,
) -> Result<BTreeSet<String>, RegistryError> {
    let mut names = BTreeSet::new();
    for source in hosted_sources(state, target, ECOSYSTEM) {
        let Some(hosted) = state.inner.config.hosted.get(&source) else { continue };
        let listed = state.inner.storage.for_hosted(&hosted.org).hosted_package_names().await?;
        for name in listed {
            if visible_here(state, identity, target, &VisibleName { source: &source, name: &name })
            {
                names.insert(name);
            }
        }
    }
    Ok(names)
}

/// Whether the target registry routes this name to this source and the caller
/// may read it there.
fn visible_here(
    state: &AppState,
    identity: &Identity,
    target: &str,
    listed: &VisibleName<'_>,
) -> bool {
    let routed_here = matches!(
        resolve_ecosystem_source(state, target, ECOSYSTEM, listed.name),
        RegistrySource::Hosted(selected) if selected == listed.source,
    );
    routed_here
        && matches!(
            hosted_gate(state, identity, listed.source, listed.name),
            HostedGate::Allowed(_),
        )
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
    let Ok(key) = CanonicalPackageName::parse(raw_project, ECOSYSTEM) else { return not_found() };
    let project = key.as_str();
    let endpoint = registry_endpoint(&state, ECOSYSTEM, registry.as_deref());
    if project != *raw_project {
        return Response::builder()
            .status(StatusCode::MOVED_PERMANENTLY)
            .header(header::LOCATION, format!("{endpoint}/{SIMPLE_PATH}/{project}/"))
            .body(Body::empty())
            .expect("static-shape response always builds");
    }
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
    };
    let document = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, project) {
        RegistrySource::Hosted(source) => {
            read_hosted_document::<ProjectDocument>(&state, &identity, &source, &key).await
        }
        source @ RegistrySource::Upstream(_) => {
            load_upstream_page(&state, &identity, &source, &key, project)
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
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(project), response)
}

/// An upstream project's page through the cache, as the parsed document plus
/// the URL its file URLs resolve against.
async fn load_upstream_page(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &CanonicalPackageName,
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
    let Ok(key) = CanonicalPackageName::parse(raw_project, ECOSYSTEM) else { return not_found() };
    let project = key.as_str();
    if !is_safe_path_segment(filename) {
        return not_found();
    }
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
    };
    let response = match resolve_ecosystem_source(&state, &target, ECOSYSTEM, project) {
        RegistrySource::Hosted(source) => {
            match read_hosted_document::<ProjectDocument>(&state, &identity, &source, &key).await {
                Ok(Some(document)) if document.file(filename).is_some() => {
                    serve_hosted_blob(&state, &identity, &source, &key, filename)
                        .await
                        .unwrap_or_else(IntoResponse::into_response)
                }
                Ok(_) => not_found(),
                Err(err) => err.into_response(),
            }
        }
        source @ RegistrySource::Upstream(_) => {
            file_via_upstream(&state, &identity, &source, &key, project, filename).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    };
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(project), response)
}

/// Proxy a file download: bind the request to the page's entry for the
/// filename (its origin URL and SHA-256) and stream it through the
/// verifying cache.
async fn file_via_upstream(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    key: &CanonicalPackageName,
    project: &str,
    filename: &str,
) -> Response {
    let (upstream, namespace) = match upstream_for(state, identity, source, key) {
        Ok(upstream) => upstream,
        Err(err) => return err.into_response(),
    };

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
