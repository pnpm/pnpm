//! The npm routes of release notes. Their paths share segment counts with
//! version documents and tarballs, so the handlers tell them apart:
//!
//! - `{base}/{name}/{version}/notes` has the three segments of a scoped
//!   version document, and is the note when its first segment is no scope.
//! - `{base}/@{scope}/{name}/{version}/notes` is a scoped version's note.
//! - `{base}/{name}/-/notes` and `{base}/@{scope}/{name}/-/notes` list a
//!   package's notes; no tarball is named `notes`.

use super::npm_handlers::{NamePath, ScopedVersionPath, get_scoped_version_manifest};
use crate::server::{
    AppState, AuthedCaller, Path, Response, State, TargetRegistry, not_found,
    notes::{NoteRequest, read_note_body, serve_package_notes, serve_version_note},
};
use axum::{body::Bytes, http::Method};
use serde::Deserialize;

/// The final segment of a version note's path.
const NOTES: &str = "notes";

#[derive(Deserialize)]
pub(super) struct ScopedNamePath {
    scope: String,
    name: String,
}

/// `{base}/{a}/{b}/{c}`: a scoped version document, or an unscoped (or
/// `%2F`-encoded scoped) version's note.
pub(super) async fn three_segments(
    state: State<AppState>,
    caller: AuthedCaller,
    registry: TargetRegistry,
    method: Method,
    Path(path): Path<ScopedVersionPath>,
    body: Bytes,
) -> Response {
    let names_a_note =
        path.version == NOTES && (!path.scope.starts_with('@') || path.scope.contains('/'));
    if !names_a_note {
        if method != Method::GET {
            return not_found();
        }
        return get_scoped_version_manifest(state, caller, registry, Path(path)).await;
    }
    version_note(&state, &caller, &registry, (&path.scope, &path.name), (&method, body)).await
}

/// `{base}/@{scope}/{name}/{version}/notes`.
pub(super) async fn scoped_version_note(
    state: State<AppState>,
    caller: AuthedCaller,
    registry: TargetRegistry,
    method: Method,
    Path(path): Path<ScopedVersionPath>,
    body: Bytes,
) -> Response {
    if !path.scope.starts_with('@') {
        return not_found();
    }
    let name = format!("{}/{}", path.scope, path.name);
    version_note(&state, &caller, &registry, (&name, &path.version), (&method, body)).await
}

async fn version_note(
    State(state): &State<AppState>,
    AuthedCaller(identity): &AuthedCaller,
    TargetRegistry(registry): &TargetRegistry,
    (name, version): (&str, &str),
    (method, body): (&Method, Bytes),
) -> Response {
    let request: NoteRequest = match read_note_body(method, &body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    serve_version_note(state, identity, registry.as_deref(), (name, version), request).await
}

/// `GET {base}/{name}/-/notes`.
pub(super) async fn package_notes(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<NamePath>,
) -> Response {
    serve_package_notes(&state, &identity, registry.as_deref(), &path.name).await
}

/// `GET {base}/@{scope}/{name}/-/notes`.
pub(super) async fn scoped_package_notes(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopedNamePath>,
) -> Response {
    if !path.scope.starts_with('@') {
        return not_found();
    }
    let name = format!("{}/{}", path.scope, path.name);
    serve_package_notes(&state, &identity, registry.as_deref(), &name).await
}
