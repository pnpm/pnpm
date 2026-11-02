//! `GET /-/publications/{org}/{id-or-name}` and
//! `GET /-/publications/{org}?member=<ecosystem>:<name>@<version>`: the
//! publications of an organization's hosted catalog.
//!
//! `-` names the catalog of the registries without an `org`. A caller sees
//! only the members it may read, and a publication with none answers 404, so
//! a publication never discloses a package. Publications are recorded only
//! when the metadata lives in `SQLite`.

use super::{
    Action, AppState, AuthedCaller, HostedGate, Identity, RegistrySource, authorize_package,
    hosted_gate, not_found,
    notes::{NoteRequest, note_text_response, read_note_body},
    private_no_cache,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::metadata::{
    ArtifactMember, Note, NoteChange, Publication, PublicationOrigin, SqliteMetadata,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// The path segment naming the catalog of registries without an `org`.
const DEFAULT_CATALOG: &str = "-";

pub(super) async fn serve_publication(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((org, id_or_name)): Path<(String, String)>,
) -> Response {
    let catalog = catalog_of(&org);
    let Some(database) = state.inner.storage.sqlite_metadata() else { return not_found() };
    let found = database.publication(catalog, &id_or_name).await;
    private_no_cache(respond(&state, &identity, found))
}

#[derive(Deserialize)]
pub(super) struct MemberQuery {
    member: String,
}

pub(super) async fn serve_member_publication(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(org): Path<String>,
    Query(query): Query<MemberQuery>,
) -> Response {
    let catalog = catalog_of(&org);
    let Some(database) = state.inner.storage.sqlite_metadata() else { return not_found() };
    let Some((name, version)) = parse_member(&query.member) else {
        return RegistryError::BadRequest {
            reason: "`member` must be <ecosystem>:<name>@<version>".to_string(),
        }
        .into_response();
    };
    let found = database.initial_publication(&name, catalog, version).await;
    private_no_cache(respond(&state, &identity, found))
}

fn catalog_of(org: &str) -> &str {
    if org == DEFAULT_CATALOG { "" } else { org }
}

/// `npm:@scope/name@1.0.0` as the package and its version.
fn parse_member(member: &str) -> Option<(CanonicalPackageName, &str)> {
    let (ecosystem, package) = member.split_once(':')?;
    let ecosystem = Ecosystem::all().find(|known| known.as_str() == ecosystem)?;
    let at = package.get(1..)?.rfind('@')? + 1;
    let (name, version) = (&package[..at], &package[at + 1..]);
    let name = CanonicalPackageName::parse(name, ecosystem).ok()?;
    (!version.is_empty()).then_some((name, version))
}

fn respond(
    state: &AppState,
    identity: &Identity,
    found: Result<Option<Publication>, RegistryError>,
) -> Response {
    match found {
        Ok(Some(publication)) => match visible(state, identity, publication) {
            Some(publication) => Json(publication_json(&publication)).into_response(),
            None => not_found(),
        },
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

/// The publication with only the members `identity` may read, or `None` when
/// it may read none of them.
fn visible(
    state: &AppState,
    identity: &Identity,
    mut publication: Publication,
) -> Option<Publication> {
    let catalog = publication.catalog.clone();
    publication.members.retain(|member| may_read(state, identity, &catalog, member));
    (!publication.members.is_empty()).then_some(publication)
}

/// Whether a hosted registry of `catalog` and the member's ecosystem lets
/// `identity` read the member's package.
fn may_read(state: &AppState, identity: &Identity, catalog: &str, member: &ArtifactMember) -> bool {
    let routing = &state.inner.config.routing;
    routing.hosted
        .iter()
        .any(|(source, hosted)| {
            hosted.org == catalog
                && routing.registries.ecosystem(source) == Some(member.ecosystem)
                && matches!(
                    hosted_gate(state, identity, source, &member.name),
                    HostedGate::Allowed(org) if org == catalog,
                )
        })
}

fn publication_json(publication: &Publication) -> Value {
    let members: Vec<Value> = publication.members
        .iter()
        .map(|member| {
            json!({
                "ecosystem": member.ecosystem.as_str(),
                "name": member.name,
                "version": member.version,
                "artifact": member.artifact,
            })
        })
        .collect();
    let origin = match publication.origin {
        PublicationOrigin::Published => "published",
        PublicationOrigin::Imported => "imported",
    };
    json!({
        "id": publication.id,
        "org": publication.catalog,
        "name": publication.name,
        "origin": origin,
        "publisher": publication.publisher,
        "committedAt": publication.committed_at,
        "members": members,
    })
}

/// Whether `identity` may read every member of `publication`.
fn reads_every_member(state: &AppState, identity: &Identity, publication: &Publication) -> bool {
    publication.members.iter().all(|member| may_read(state, identity, &publication.catalog, member))
}

/// Whether anyone, signed in or not, may read `name` in `org`.
pub(super) fn anonymously_readable(
    state: &AppState,
    org: &str,
    name: &CanonicalPackageName,
) -> bool {
    let member = ArtifactMember {
        ecosystem: name.ecosystem(),
        name: name.as_str().to_string(),
        version: String::new(),
        artifact: String::new(),
    };
    may_read(state, &Identity::Anonymous, org, &member)
}

/// The note of `publication` as a version's response references it: present
/// when the publication has a note, or was published with one, and
/// `identity` may read every member.
pub(super) async fn readable_publication_note(
    state: &AppState,
    identity: &Identity,
    database: &Arc<SqliteMetadata>,
    publication: Publication,
) -> Result<Option<ReadablePublicationNote>, RegistryError> {
    if publication.origin != PublicationOrigin::Published
        || !reads_every_member(state, identity, &publication)
    {
        return Ok(None);
    }
    let Some(note) = database.publication_note(&publication.catalog, publication.id).await? else {
        return Ok(None);
    };
    let public = reads_every_member(state, &Identity::Anonymous, &publication);
    Ok(Some(ReadablePublicationNote { publication, note, public }))
}

/// A publication note the caller may read.
pub(super) struct ReadablePublicationNote {
    pub(super) publication: Publication,
    pub(super) note: Note,
    /// Whether anyone may read every member.
    pub(super) public: bool,
}

impl From<ReadablePublicationNote> for super::notes::PublicationLink {
    fn from(readable: ReadablePublicationNote) -> Self {
        Self {
            org: readable.publication.catalog,
            id: readable.publication.id,
            name: readable.publication.name,
            initial_digest: readable.note.initial_digest,
            public: readable.public,
        }
    }
}

/// `GET|PUT|DELETE /-/publications/{org}/{id-or-name}/notes`. Reading needs
/// read access to every member, writing the `publish` rule of every member.
pub(super) async fn serve_publication_note(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((org, id_or_name)): Path<(String, String)>,
    method: axum::http::Method,
    body: axum::body::Bytes,
) -> Response {
    let request = match read_note_body(&method, &body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(database) = state.inner.storage.sqlite_metadata().cloned() else { return not_found() };
    let publication = match database.publication(catalog_of(&org), &id_or_name).await {
        Ok(Some(publication)) if reads_every_member(&state, &identity, &publication) => publication,
        Ok(_) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let result = match request {
        NoteRequest::Read => read_publication_note(&state, &database, &publication).await,
        NoteRequest::Write(text) => {
            change_publication_note(&state, &identity, &database, &publication, Some(text)).await
        }
        NoteRequest::Delete => {
            change_publication_note(&state, &identity, &database, &publication, None).await
        }
    };
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn read_publication_note(
    state: &AppState,
    database: &Arc<SqliteMetadata>,
    publication: &Publication,
) -> Result<Response, RegistryError> {
    let note = database.publication_note(&publication.catalog, publication.id).await?;
    let public = reads_every_member(state, &Identity::Anonymous, publication);
    let mut response = match &note {
        Some(note) if note.text.is_some() => note_text_response(note, public),
        _ => not_found(),
    };
    if let Some(digest) = note.as_ref().and_then(|note| note.initial_digest.as_deref())
        && let Ok(value) = axum::http::HeaderValue::from_str(digest)
    {
        response.headers_mut().insert("pnpr-note-initial-digest", value);
    }
    Ok(response)
}

async fn change_publication_note(
    state: &AppState,
    identity: &Identity,
    database: &Arc<SqliteMetadata>,
    publication: &Publication,
    text: Option<String>,
) -> Result<Response, RegistryError> {
    for member in &publication.members {
        authorize_member_publish(state, identity, &publication.catalog, member).await?;
    }
    let principal = identity
        .username()
        .unwrap_or("-")
        .to_string();
    let change = NoteChange { text, principal: principal.clone() };
    if !database.set_publication_note(&publication.catalog, publication.id, change).await? {
        return Err(RegistryError::NotFound);
    }
    tracing::info!(
        target: "pnpr::audit",
        action = "publication.note_change",
        actor = %principal,
        org = %publication.catalog,
        publication = publication.id,
        "publication note changed",
    );
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Check the `publish` rule of a hosted registry of `catalog` that serves
/// the member's package.
async fn authorize_member_publish(
    state: &AppState,
    identity: &Identity,
    catalog: &str,
    member: &ArtifactMember,
) -> Result<(), RegistryError> {
    let routing = &state.inner.config.routing;
    let source = routing.hosted
        .iter()
        .find_map(|(source, hosted)| {
            (hosted.org == catalog
                && routing.registries.ecosystem(source) == Some(member.ecosystem))
            .then(|| source.clone())
        });
    let Some(source) = source else { return Err(RegistryError::NotFound) };
    let hosted = RegistrySource::Hosted(source);
    authorize_package(state, identity, &hosted, &member.name, Action::Publish, Some(catalog)).await
}
