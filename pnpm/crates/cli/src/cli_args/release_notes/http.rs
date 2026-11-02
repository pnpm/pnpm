//! The requests for the note routes. A `404` is an absent note, never an
//! error: a registry that stores no notes for the package answers it too.

use crate::cli_args::registry_client::{apply_auth_and_otp, auth_header_for_package};
use futures_util::StreamExt;
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_network::{ThrottledClient, redact_url_credentials};
use pnpm_release_notes::{MAX_NOTE_BYTES, NotesList, PublicationRef};
use reqwest::{Url, header::HeaderMap};

/// The most a `<name>/-/notes` document may take. A registry caps it at
/// 1 MiB of JSON.
const MAX_LIST_BYTES: usize = 2 * 1024 * 1024;
/// The most a single note response may take: the content limit, with room
/// for a registry that counts it differently.
const MAX_NOTE_RESPONSE_BYTES: usize = 4 * MAX_NOTE_BYTES;

pub(super) struct NotesHttp<'a> {
    pub client: &'a ThrottledClient,
    pub config: &'a Config,
}

/// What `<name>/<version>/notes` answered.
#[derive(Debug, Default)]
pub(crate) struct VersionNoteResponse {
    pub text: Option<String>,
    /// `PNPR-Note-Initial-Digest`.
    pub initial_digest: Option<String>,
    pub publication: Option<PublicationLink>,
}

/// The publication note a version route links to.
#[derive(Debug, Clone)]
pub(crate) struct PublicationLink {
    pub url: String,
    /// `PNPR-Publication-Name`.
    pub name: Option<String>,
    /// `PNPR-Publication-Initial-Digest`.
    pub initial_digest: Option<String>,
}

impl NotesHttp<'_> {
    /// `<registry>/<name>/-/notes`, or `None` when the registry has none.
    pub async fn notes_list(
        &self,
        registry: &str,
        name: &str,
    ) -> miette::Result<Option<NotesList>> {
        let url = join(registry, &format!("{name}/-/notes"))?;
        let Some((_, body)) = self.get(&url, name, MAX_LIST_BYTES).await? else {
            return Ok(None);
        };
        serde_json::from_slice(&body)
            .map(Some)
            .into_diagnostic()
            .map_err(|error| error.wrap_err(format!("parse {}", redact_url_credentials(&url))))
    }

    /// `<registry>/<name>/<version>/notes`, or `None` on a `404`.
    pub async fn version_note(
        &self,
        registry: &str,
        name: &str,
        version: &str,
    ) -> miette::Result<Option<VersionNoteResponse>> {
        let url = join(registry, &format!("{name}/{version}/notes"))?;
        let Some((headers, body)) = self.get(&url, name, MAX_NOTE_RESPONSE_BYTES).await? else {
            return Ok(None);
        };
        Ok(Some(VersionNoteResponse {
            text: (!body.is_empty()).then(|| String::from_utf8_lossy(&body).into_owned()),
            initial_digest: header(&headers, "pnpr-note-initial-digest"),
            publication: publication_link(&url, &headers),
        }))
    }

    /// The publication note at `url`, read on behalf of `package`'s
    /// credentials, with its `PNPR-Note-Initial-Digest`.
    pub async fn publication_note(
        &self,
        url: &str,
        package: &str,
    ) -> miette::Result<Option<(String, Option<String>)>> {
        let Some((headers, body)) = self.get(url, package, MAX_NOTE_RESPONSE_BYTES).await? else {
            return Ok(None);
        };
        let text = String::from_utf8_lossy(&body).into_owned();
        Ok(Some((text, header(&headers, "pnpr-note-initial-digest"))))
    }

    /// `GET url`: its headers and body, or `None` on a `404`. `204` has an
    /// empty body.
    async fn get(
        &self,
        url: &str,
        package: &str,
        limit: usize,
    ) -> miette::Result<Option<(HeaderMap, Vec<u8>)>> {
        let guard = self.client.acquire_for_url(url).await;
        let auth_header = auth_header_for_package(self.config, url, package);
        let response = apply_auth_and_otp(guard.get(url), auth_header.as_deref(), None)
            .send()
            .await
            .into_diagnostic()?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        let display_url = redact_url_credentials(url);
        if !status.is_success() {
            miette::bail!("GET {display_url} returned {status}");
        }
        let headers = response.headers().clone();
        let body = read_bounded(response, limit).await
            .ok_or_else(|| miette::miette!("GET {display_url} returned more than {limit} bytes"))?;
        Ok(Some((headers, body)))
    }
}

/// The URL of a publication's note, from the server root of `registry`.
pub(super) fn publication_url(registry: &str, publication: &PublicationRef) -> Option<String> {
    let path = publication.notes_path()?;
    Url::parse(registry)
        .and_then(|registry| registry.join(&format!("/{path}")))
        .ok()
        .map(String::from)
}

fn join(registry: &str, path: &str) -> miette::Result<String> {
    crate::cli_args::registry_client::join_registry_endpoint(registry, path).into_diagnostic()
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// The `rel="publication-notes"` target of a `Link` header, when it is on
/// the origin that answered: a registry never sends the reader's
/// credentials elsewhere.
fn publication_link(request_url: &str, headers: &HeaderMap) -> Option<PublicationLink> {
    let base = Url::parse(request_url).ok()?;
    let target = headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .find(|link| link.contains(r#"rel="publication-notes""#))?;
    let reference = target
        .trim()
        .strip_prefix('<')?
        .split('>')
        .next()?;
    let url = base.join(reference).ok()?;
    (url.origin() == base.origin()).then(|| PublicationLink {
        url: url.into(),
        name: header(headers, "pnpr-publication-name"),
        initial_digest: header(headers, "pnpr-publication-initial-digest"),
    })
}

async fn read_bounded(response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if body.len() + chunk.len() > limit {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}
