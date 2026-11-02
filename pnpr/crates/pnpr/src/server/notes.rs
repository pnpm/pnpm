//! Release notes of hosted npm package versions:
//!
//! - `GET|PUT|DELETE {base}/{name}/{version}/notes` — one version's note.
//!   `GET` also takes a dist-tag.
//! - `GET {base}/{name}/-/notes` — the notes of every version, newest first.
//!
//! A version note has the access rules of its package: reading it needs read
//! access, writing it the package's `publish` rule. Notes are served only
//! when the metadata lives in `SQLite`. A version's publication note is
//! referenced only when the caller may read every member of the publication.

pub(super) use listing::serve_package_notes;
pub(super) use responses::{NoteRequest, PublicationLink, note_text_response, read_note_body};

mod listing;
mod responses;

use super::{
    Action, AppState, Identity, RegistrySource, WriteTarget, authorize_package, not_found,
    publications::readable_publication_note, resolve_write_target,
};
use axum::response::{IntoResponse, Response};
use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_storage::metadata::{NoteChange, SqliteMetadata};
use responses::version_note_response;
use std::sync::Arc;

/// The hosted package a notes request addresses, once the caller may reach
/// it.
struct NotesTarget {
    database: Arc<SqliteMetadata>,
    name: CanonicalPackageName,
    target: WriteTarget,
}

/// Resolve the hosted package and check that the caller may read it, or for a
/// write, publish it. A package that routes to an upstream has no notes.
async fn notes_target(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    writes: bool,
) -> Result<NotesTarget, Response> {
    let database = state.inner.storage
        .sqlite_metadata()
        .cloned()
        .ok_or_else(not_found)?;
    let name = CanonicalPackageName::parse(raw_name, Ecosystem::Npm).map_err(|_| not_found())?;
    let target = match resolve_write_target(state, identity, registry, &name) {
        Ok(target) => target,
        Err(RegistryError::BadRequest { .. }) => return Err(not_found()),
        Err(err) => return Err(err.into_response()),
    };
    if writes {
        let source = RegistrySource::Hosted(target.source.clone());
        authorize_package(
            state,
            identity,
            &source,
            name.as_str(),
            Action::Publish,
            Some(&target.org),
        )
        .await
        .map_err(IntoResponse::into_response)?;
    }
    Ok(NotesTarget { database, name, target })
}

/// `{base}/{name}/{version}/notes`.
pub(super) async fn serve_version_note(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    (raw_name, version): (&str, &str),
    request: NoteRequest,
) -> Response {
    let writes = !matches!(request, NoteRequest::Read);
    let notes = match notes_target(state, identity, registry, raw_name, writes).await {
        Ok(notes) => notes,
        Err(response) => return response,
    };
    let result = match request {
        NoteRequest::Read => read_version_note(state, identity, &notes, version).await,
        NoteRequest::Write(text) => write_version_note(identity, &notes, version, Some(text)).await,
        NoteRequest::Delete => write_version_note(identity, &notes, version, None).await,
    };
    result.unwrap_or_else(IntoResponse::into_response)
}

async fn read_version_note(
    state: &AppState,
    identity: &Identity,
    notes: &NotesTarget,
    version_or_tag: &str,
) -> Result<Response, RegistryError> {
    let version = resolve_tag(state, notes, version_or_tag).await?;
    let NotesTarget { database, name, target } = notes;
    let note = database.version_note(name, &target.org, &version).await?;
    let publication = database.initial_publication(name, &target.org, &version).await?;
    let link = match publication {
        Some(publication) => readable_publication_note(state, identity, database, publication)
            .await?
            .map(PublicationLink::from),
        None => None,
    };
    let public = super::publications::anonymously_readable(state, &target.org, name);
    Ok(version_note_response(note.as_ref(), link.as_ref(), public))
}

/// The version a dist-tag names, or `version_or_tag` itself.
async fn resolve_tag(
    state: &AppState,
    notes: &NotesTarget,
    version_or_tag: &str,
) -> Result<String, RegistryError> {
    if node_semver::Version::parse(version_or_tag).is_ok() {
        return Ok(version_or_tag.to_string());
    }
    let storage = state.inner.storage.for_hosted(&notes.target.org, Ecosystem::Npm);
    let Some(view) = storage.read_hosted_package_view(&notes.name).await? else {
        return Err(RegistryError::NotFound);
    };
    let document: serde_json::Value = serde_json::from_slice(&view.bytes)?;
    let tagged = document
        .get("dist-tags")
        .and_then(|tags| tags.get(version_or_tag));
    Ok(tagged
        .and_then(serde_json::Value::as_str)
        .unwrap_or(version_or_tag)
        .to_string())
}

async fn write_version_note(
    identity: &Identity,
    notes: &NotesTarget,
    version: &str,
    text: Option<String>,
) -> Result<Response, RegistryError> {
    let principal = identity
        .username()
        .unwrap_or("-")
        .to_string();
    let deleted = text.is_none();
    let change = NoteChange { text, principal: principal.clone() };
    let NotesTarget { database, name, target } = notes;
    if !database.set_version_note(name, &target.org, version, change).await? {
        return Err(RegistryError::NotFound);
    }
    tracing::info!(
        target: "pnpr::audit",
        action = if deleted { "package.note_delete" } else { "package.note_set" },
        actor = %principal,
        package = %name.as_str(),
        version = %version,
        registry = %target.source,
        "package version note changed",
    );
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}
