//! The registry admin API: `/-/pnpr/v0/admin/registries` and
//! `/-/pnpr/v0/admin/registry-revisions`. Reads need one of the `auth.admins`
//! or `auth.registryAdmins`, and writes one of the `auth.registryAdmins`.

use super::{
    super::{
        AppState, AuthedCaller, RegistryError, Response, StatusCode, iso_from_unix_millis,
        json_response, parse_json_body, private_no_cache, team_mutations::respond,
    },
    edit::{EditOutcome, Effect, GraphEdit, Scope, edit_graph},
    stored::{graph_version, parse_document, stored_document},
};
use crate::audit::Actor;
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, header},
    routing::{get, post},
};
use pnpr_config::RegistryGraphDocument;
use pnpr_policy::Identity;
use pnpr_storage::metadata::RegistryGraphRevision;
use serde::Deserialize;
use serde_json::{Value, json};

/// The largest registry document or entry the API accepts.
const MAX_GRAPH_BODY_BYTES: usize = 4 * 1024 * 1024;

const DEFAULT_REVISIONS: u32 = 50;
const MAX_REVISIONS: u32 = 500;

pub(in super::super) fn registry_admin_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/-/pnpr/v0/admin/registries",
            get(get_document)
                .put(put_document)
                .route_layer(DefaultBodyLimit::max(MAX_GRAPH_BODY_BYTES)),
        )
        .route(
            "/-/pnpr/v0/admin/registries/{name}",
            get(get_registry)
                .put(put_registry)
                .delete(delete_registry)
                .route_layer(DefaultBodyLimit::max(MAX_GRAPH_BODY_BYTES)),
        )
        .route("/-/pnpr/v0/admin/registry-revisions", get(list_revisions))
        .route("/-/pnpr/v0/admin/registry-revisions/{version}", get(get_revision))
        .route("/-/pnpr/v0/admin/registry-revisions/{version}/restore", post(restore_revision))
}

/// Whether a request path belongs to this API, which stays reachable while the
/// replica cannot serve its graph, so an admin can repair the graph.
pub(in super::super) fn is_registry_admin_path(path: &str) -> bool {
    path == "/-/pnpr/v0/admin/registries"
        || path.starts_with("/-/pnpr/v0/admin/registries/")
        || path == "/-/pnpr/v0/admin/registry-revisions"
        || path.starts_with("/-/pnpr/v0/admin/registry-revisions/")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DryRun {
    #[serde(default)]
    dry_run: bool,
}

#[derive(Deserialize)]
struct RevisionPage {
    before: Option<u64>,
    limit: Option<u32>,
}

/// `GET /-/pnpr/v0/admin/registries`: the stored document, its version in
/// `ETag`.
async fn get_document(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let result = async {
        require_reader(&state, &identity)?;
        let (document, version) = read_document(&state).await?;
        Ok(versioned(json_response(StatusCode::OK, &serde_json::to_value(&document)?), &version))
    };
    private_no_cache(respond(result.await))
}

/// `PUT /-/pnpr/v0/admin/registries`: replace the document.
async fn put_document(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Query(query): Query<DryRun>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        require_writer(&state, &identity)?;
        let document: RegistryGraphDocument = parse_json_body(&body)?;
        let expected = if_match(&headers)?;
        let request = replacement(&identity, expected.as_deref(), query.dry_run, None);
        let outcome = edit_graph(&state, request, |stored| {
            *stored = document.clone();
            Ok(())
        })
        .await?;
        if outcome.wrote() {
            let actor = Actor::of(&identity);
            crate::audit!(
                actor,
                "registries.replace",
                version = %outcome.version,
                previous_version = %outcome.previous_version,
                "registries replaced",
            );
        }
        Ok(outcome_response(&outcome))
    };
    private_no_cache(respond(result.await))
}

/// `GET /-/pnpr/v0/admin/registries/{name}`: one stored entry.
async fn get_registry(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
) -> Response {
    let result = async {
        require_reader(&state, &identity)?;
        let (document, version) = read_document(&state).await?;
        let entry = document.registry_entry(&name).ok_or(RegistryError::NotFound)?;
        Ok(versioned(json_response(StatusCode::OK, &entry), &version))
    };
    private_no_cache(respond(result.await))
}

/// `PUT /-/pnpr/v0/admin/registries/{name}`: add the entry or replace it.
async fn put_registry(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    Query(query): Query<DryRun>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        require_writer(&state, &identity)?;
        let entry: Value = parse_json_body(&body)?;
        let expected = if_match(&headers)?;
        let request = graph_edit(&identity, expected.as_deref(), query.dry_run, None);
        let outcome = edit_graph(&state, request, |document| {
            document.set_registry_entry(&name, entry.clone())
        })
        .await?;
        if outcome.wrote() {
            let actor = Actor::of(&identity);
            crate::audit!(
                actor,
                "registries.update",
                registry = %name,
                version = %outcome.version,
                previous_version = %outcome.previous_version,
                "registry updated",
            );
        }
        Ok(outcome_response(&outcome))
    };
    private_no_cache(respond(result.await))
}

/// `DELETE /-/pnpr/v0/admin/registries/{name}`.
async fn delete_registry(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    Query(query): Query<DryRun>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        require_writer(&state, &identity)?;
        let expected = if_match(&headers)?;
        let request = graph_edit(&identity, expected.as_deref(), query.dry_run, None);
        let outcome = edit_graph(&state, request, |document| {
            if document.remove_registry(&name) { Ok(()) } else { Err(RegistryError::NotFound) }
        })
        .await?;
        if outcome.wrote() {
            let actor = Actor::of(&identity);
            crate::audit!(
                actor,
                "registries.delete",
                registry = %name,
                version = %outcome.version,
                previous_version = %outcome.previous_version,
                "registry deleted",
            );
        }
        Ok(outcome_response(&outcome))
    };
    private_no_cache(respond(result.await))
}

/// `GET /-/pnpr/v0/admin/registry-revisions`: the stored revisions, newest
/// first.
async fn list_revisions(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Query(page): Query<RevisionPage>,
) -> Response {
    let result = async {
        require_reader(&state, &identity)?;
        let limit = page.limit.unwrap_or(DEFAULT_REVISIONS).clamp(1, MAX_REVISIONS);
        let revisions =
            state.inner.storage.list_registry_graph_revisions(page.before, limit).await?;
        let revisions: Vec<Value> = revisions
            .iter()
            .map(revision_view)
            .collect();
        Ok(json_response(StatusCode::OK, &json!({ "revisions": revisions })))
    };
    private_no_cache(respond(result.await))
}

/// `GET /-/pnpr/v0/admin/registry-revisions/{version}`: one revision's
/// document.
async fn get_revision(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(version): Path<String>,
) -> Response {
    let result = async {
        require_reader(&state, &identity)?;
        let document = read_revision(&state, &version).await?;
        Ok(json_response(StatusCode::OK, &serde_json::to_value(&document)?))
    };
    private_no_cache(respond(result.await))
}

/// `POST /-/pnpr/v0/admin/registry-revisions/{version}/restore`: store that
/// revision's document as the next revision.
async fn restore_revision(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(version): Path<String>,
    Query(query): Query<DryRun>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        require_writer(&state, &identity)?;
        let restored = read_revision(&state, &version).await?;
        let generation = revision_generation(&version)?;
        let expected = if_match(&headers)?;
        let request = replacement(&identity, expected.as_deref(), query.dry_run, Some(generation));
        let outcome = edit_graph(&state, request, |document| {
            *document = restored.clone();
            Ok(())
        })
        .await?;
        if outcome.wrote() {
            let actor = Actor::of(&identity);
            crate::audit!(
                actor,
                "registries.restore",
                restored = %version,
                version = %outcome.version,
                previous_version = %outcome.previous_version,
                "registries restored",
            );
        }
        Ok(outcome_response(&outcome))
    };
    private_no_cache(respond(result.await))
}

fn graph_edit<'a>(
    identity: &'a Identity,
    expected: Option<&'a [String]>,
    dry_run: bool,
    restored_from: Option<u64>,
) -> GraphEdit<'a> {
    GraphEdit {
        effect: if dry_run { Effect::DryRun } else { Effect::Write },
        restored_from,
        ..GraphEdit::latest(Actor::of(identity)).expecting(expected)
    }
}

/// [`graph_edit`] for an edit that replaces the whole document.
fn replacement<'a>(
    identity: &'a Identity,
    expected: Option<&'a [String]>,
    dry_run: bool,
    restored_from: Option<u64>,
) -> GraphEdit<'a> {
    GraphEdit { scope: Scope::Whole, ..graph_edit(identity, expected, dry_run, restored_from) }
}

async fn read_document(state: &AppState) -> Result<(RegistryGraphDocument, String), RegistryError> {
    let stored = state.inner.storage.read_registry_graph().await?;
    let version = graph_version(stored.as_ref().map(|stored| stored.generation));
    Ok((stored_document(stored.as_ref())?, version))
}

async fn read_revision(
    state: &AppState,
    version: &str,
) -> Result<RegistryGraphDocument, RegistryError> {
    let generation = revision_generation(version)?;
    let stored = state.inner.storage
        .read_registry_graph_revision(generation)
        .await?
        .ok_or(RegistryError::NotFound)?;
    parse_document(&stored.document)
}

fn revision_generation(version: &str) -> Result<u64, RegistryError> {
    version.parse().map_err(|_| RegistryError::NotFound)
}

fn revision_view(revision: &RegistryGraphRevision) -> Value {
    json!({
        "version": graph_version(Some(revision.generation)),
        "actorId": revision.actor_id,
        "createdAt": iso_from_unix_millis(revision.created_at),
        "restoredFrom": revision.restored_from.map(|generation| graph_version(Some(generation))),
    })
}

fn outcome_response(outcome: &EditOutcome) -> Response {
    let body = json!({ "version": outcome.version, "changes": outcome.changes });
    versioned(json_response(StatusCode::OK, &body), &outcome.version)
}

/// One of the `auth.admins` or `auth.registryAdmins`.
fn require_reader(state: &AppState, identity: &Identity) -> Result<(), RegistryError> {
    let auth = &state.inner.config.identity;
    require(identity, auth.is_admin(identity) || auth.is_registry_admin(identity), "read")
}

/// One of the `auth.registryAdmins`.
fn require_writer(state: &AppState, identity: &Identity) -> Result<(), RegistryError> {
    require(identity, state.inner.config.identity.is_registry_admin(identity), "change")
}

fn require(identity: &Identity, allowed: bool, action: &'static str) -> Result<(), RegistryError> {
    if allowed {
        return Ok(());
    }
    let resource = "the registries".to_string();
    match identity.username() {
        Some(user) => Err(RegistryError::Forbidden { user: user.to_string(), action, resource }),
        None => Err(RegistryError::Unauthenticated { resource }),
    }
}

/// The versions an `If-Match` header names, or `None` for no condition (no
/// header, or `*`). `If-Match` compares strongly, so a weak tag
/// (`W/"..."`) names no version and never matches.
pub(in super::super) fn if_match(
    headers: &HeaderMap,
) -> Result<Option<Vec<String>>, RegistryError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let invalid = || RegistryError::BadRequest {
        reason: "If-Match must be `*` or a list of quoted versions".to_string(),
    };
    let value = value
        .to_str()
        .map_err(|_| invalid())?
        .trim();
    if value == "*" {
        return Ok(None);
    }
    let mut versions = Vec::new();
    for tag in value.split(',').map(str::trim) {
        let (weak, quoted) = tag
            .strip_prefix("W/")
            .map(|quoted| (true, quoted))
            .unwrap_or((false, tag));
        let version = quoted
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(invalid)?;
        if !weak {
            versions.push(version.to_string());
        }
    }
    Ok(Some(versions))
}

/// `response` with `version` as its `ETag`.
pub(in super::super) fn versioned(mut response: Response, version: &str) -> Response {
    if let Ok(etag) = HeaderValue::from_str(&format!(r#""{version}""#)) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
}
