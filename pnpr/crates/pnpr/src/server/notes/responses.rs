//! The responses of the note routes.

use axum::{
    body::{Body, Bytes},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_storage::metadata::{Note, note_digest};

/// How long a shared cache may keep a public note response.
pub(in super::super) const PUBLIC_NOTE_MAX_AGE: u64 = 300;

/// What a note request does.
pub(in crate::server) enum NoteRequest {
    Read,
    /// Replace the note with this text.
    Write(String),
    Delete,
}

/// The request a note route's method and body ask for.
pub(in crate::server) fn read_note_body(
    method: &axum::http::Method,
    body: &Bytes,
) -> Result<NoteRequest, Response> {
    match *method {
        axum::http::Method::GET | axum::http::Method::HEAD => Ok(NoteRequest::Read),
        axum::http::Method::DELETE => Ok(NoteRequest::Delete),
        axum::http::Method::PUT => String::from_utf8(body.to_vec())
            .map(NoteRequest::Write)
            .map_err(|_| {
                RegistryError::BadRequest { reason: "a note must be UTF-8".to_string() }
                    .into_response()
            }),
        _ => Err(StatusCode::METHOD_NOT_ALLOWED.into_response()),
    }
}

/// The publication note a version note response links to.
#[derive(Clone)]
pub(in crate::server) struct PublicationLink {
    pub(in crate::server) org: String,
    pub(in crate::server) id: u64,
    pub(in crate::server) name: Option<String>,
    pub(in crate::server) initial_digest: Option<String>,
    /// Whether every member of the publication is public.
    pub(in crate::server) public: bool,
}

/// `200` with the version's note, `204` when only its publication has one,
/// `404` otherwise. A version published with a note keeps its initial digest
/// header after the note is edited or deleted.
pub(super) fn version_note_response(
    note: Option<&Note>,
    link: Option<&PublicationLink>,
    public: bool,
) -> Response {
    let text = note.and_then(|note| note.text.as_deref());
    let mut response = match (text, link) {
        (Some(_), _) => note_text_response(note.expect("text implies a note"), public),
        (None, Some(_)) => StatusCode::NO_CONTENT.into_response(),
        (None, None) => StatusCode::NOT_FOUND.into_response(),
    };
    let headers = response.headers_mut();
    if let Some(digest) = note.and_then(|note| note.initial_digest.as_deref()) {
        insert(headers, "pnpr-note-initial-digest", digest);
    }
    if let Some(link) = link {
        insert_publication_link(headers, link);
    }
    with_cache_rules(response, public && link.is_none_or(|link| link.public))
}

fn insert_publication_link(headers: &mut axum::http::HeaderMap, link: &PublicationLink) {
    let org = if link.org.is_empty() { "-" } else { &link.org };
    let target = format!(r#"</-/publications/{org}/{}/notes>; rel="publication-notes""#, link.id);
    insert(headers, header::LINK.as_str(), &target);
    if let Some(name) = &link.name {
        insert(headers, "pnpr-publication-name", name);
    }
    if let Some(digest) = &link.initial_digest {
        insert(headers, "pnpr-publication-initial-digest", digest);
    }
}

/// `200` with a note's text and its validators.
pub(in crate::server) fn note_text_response(note: &Note, public: bool) -> Response {
    let text = note.text.clone().unwrap_or_default();
    let etag = format!(r#""{}""#, note_digest(&text));
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .header(header::ETAG, etag)
        .body(Body::from(text))
        .expect("static-shape response always builds");
    if let Some(modified) = http_date(&note.modified) {
        insert(response.headers_mut(), header::LAST_MODIFIED.as_str(), &modified);
    }
    with_cache_rules(response, public)
}

/// A public response may be cached by anyone for a short while; any other
/// depends on the caller's credentials.
pub(in crate::server) fn with_cache_rules(mut response: Response, public: bool) -> Response {
    if !public {
        return crate::server::private_no_cache(response);
    }
    let value = format!("public, max-age={PUBLIC_NOTE_MAX_AGE}");
    insert(response.headers_mut(), header::CACHE_CONTROL.as_str(), &value);
    response
}

fn http_date(iso: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(iso).ok()?;
    Some(
        parsed
            .with_timezone(&chrono::Utc)
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string(),
    )
}

fn insert(headers: &mut axum::http::HeaderMap, name: &str, value: &str) {
    if let (Ok(name), Ok(value)) =
        (axum::http::HeaderName::try_from(name), HeaderValue::from_str(value))
    {
        headers.insert(name, value);
    }
}
