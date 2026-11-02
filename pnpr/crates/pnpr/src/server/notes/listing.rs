//! `GET {base}/{name}/-/notes`: every note of a package in one cacheable
//! document, newest version first.

use super::{
    NotesTarget, notes_target,
    responses::{PublicationLink, with_cache_rules},
};
use crate::server::{
    AppState, Identity, PublishTarget, publications::readable_publication_note,
    resolve_publish_target_for, routes_to_upstream,
};
use axum::{
    Json,
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;
use pnpr_storage::metadata::Note;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Most entries the document lists.
const MAX_ENTRIES: usize = 100;
/// Most bytes of serialized entries the document holds.
const MAX_BYTES: usize = 1024 * 1024;
/// Most versions the listing examines for one document.
const MAX_CANDIDATES: usize = 4 * MAX_ENTRIES;

pub(in crate::server) async fn serve_package_notes(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    if upstream_without_notes(state, identity, registry, raw_name) {
        return with_cache_rules(Json(json!({ "notes": [] })).into_response(), false);
    }
    let notes = match notes_target(state, identity, registry, raw_name, false).await {
        Ok(notes) => notes,
        Err(response) => return response,
    };
    match package_notes(state, identity, &notes).await {
        Ok(response) => response,
        Err(err) => err.into_response(),
    }
}

/// Whether the package routes to an upstream registry the caller may read.
/// An upstream such as npm's registry keeps no notes, so its packages list
/// none. Forwarding to an upstream that is another pnpr is not done yet.
fn upstream_without_notes(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> bool {
    let target = resolve_publish_target_for(state, identity, registry, Ecosystem::Npm, raw_name);
    matches!(target, PublishTarget::Reject(_))
        && routes_to_upstream(state, registry, Ecosystem::Npm, raw_name)
}

/// Build the listing newest first, reading a version's note and resolving
/// its publication only while the document still has room, so a package
/// with a long history costs no more than the entries the document holds.
async fn package_notes(
    state: &AppState,
    identity: &Identity,
    notes: &NotesTarget,
) -> Result<Response, RegistryError> {
    let candidates = Candidates::load(notes).await?;
    let package_public =
        crate::server::publications::anonymously_readable(state, &notes.target.source, &notes.name);
    let mut listing = Listing { public: package_public, ..Listing::default() };
    let mut links = BTreeMap::new();
    for (examined, version) in candidates
        .newest_first()
        .into_iter()
        .enumerate()
    {
        // Entries the caller may not read are skipped; stop looking long
        // before a package's whole history is examined.
        if examined == MAX_CANDIDATES {
            listing.truncated = true;
            break;
        }
        let Some((entry, public)) =
            candidates.entry(state, identity, notes, &mut links, version).await?
        else {
            continue;
        };
        if !listing.push(entry, public) {
            break;
        }
    }
    Ok(listing.into_response())
}

/// The versions the listing may name, without their notes: those whose note
/// holds text or a publish-time digest, and those whose publication's note
/// does.
struct Candidates {
    noted: BTreeSet<String>,
    published: BTreeMap<String, u64>,
}

impl Candidates {
    async fn load(notes: &NotesTarget) -> Result<Self, RegistryError> {
        let NotesTarget { database, name, target } = notes;
        let noted = database.noted_versions(name, &target.org).await?;
        let published = database.noted_publications(name, &target.org).await?;
        Ok(Self { noted: noted.into_iter().collect(), published: published.into_iter().collect() })
    }

    fn newest_first(&self) -> Vec<&String> {
        let mut versions: Vec<&String> = self.noted
            .iter()
            .chain(self.published.keys())
            .collect();
        versions.sort_by(|left, right| newest_first(left, right));
        versions.dedup();
        versions
    }

    /// The entry of `version`, and whether anyone may read it; `None` when
    /// the caller may read nothing about it.
    async fn entry(
        &self,
        state: &AppState,
        identity: &Identity,
        notes: &NotesTarget,
        links: &mut BTreeMap<u64, Option<PublicationLink>>,
        version: &str,
    ) -> Result<Option<(Value, bool)>, RegistryError> {
        let note = if self.noted.contains(version) {
            notes.database.version_note(&notes.name, &notes.target.org, version).await?
        } else {
            None
        };
        // A note added later and then deleted names nothing.
        let note = note.filter(|note| note.text.is_some() || note.initial_digest.is_some());
        let link = match self.published.get(version) {
            Some(&id) => publication_link(state, identity, notes, links, id).await?,
            None => None,
        };
        if note.is_none() && link.is_none() {
            return Ok(None);
        }
        let public_url = &state.inner.config.http.public_url;
        Ok(Some(entry_json(version, note.as_ref(), link.as_ref(), public_url)))
    }
}

/// The link to publication `id`'s note when the caller may read it, looked
/// up once per publication.
async fn publication_link(
    state: &AppState,
    identity: &Identity,
    notes: &NotesTarget,
    links: &mut BTreeMap<u64, Option<PublicationLink>>,
    id: u64,
) -> Result<Option<PublicationLink>, RegistryError> {
    if let Some(link) = links.get(&id) {
        return Ok(link.clone());
    }
    let link = match notes.database.publication(&notes.target.org, &id.to_string()).await? {
        Some(publication) => {
            readable_publication_note(state, identity, &notes.database, publication)
                .await?
                .map(PublicationLink::from)
        }
        None => None,
    };
    links.insert(id, link.clone());
    Ok(link)
}

/// The entries of the document, within its bounds.
#[derive(Default)]
struct Listing {
    entries: Vec<Value>,
    size: usize,
    truncated: bool,
    /// Whether every entry holds only what anyone may read.
    public: bool,
}

impl Listing {
    /// Add an entry, or report `false` once the document is full.
    fn push(&mut self, entry: Value, public: bool) -> bool {
        let size = entry.to_string().len();
        if self.entries.len() == MAX_ENTRIES || self.size + size > MAX_BYTES {
            self.truncated = true;
            return false;
        }
        self.public &= public;
        self.size += size;
        self.entries.push(entry);
        true
    }

    fn into_response(self) -> Response {
        let mut body = json!({ "notes": self.entries });
        if self.truncated {
            body["truncated"] = Value::Bool(true);
        }
        with_cache_rules(Json(body).into_response(), self.public)
    }
}

/// A version's entry, and whether it holds only what anyone may read.
fn entry_json(
    version: &str,
    note: Option<&Note>,
    link: Option<&PublicationLink>,
    public_url: &str,
) -> (Value, bool) {
    let mut entry = Map::new();
    entry.insert("version".to_string(), json!(version));
    if let Some(note) = note {
        entry.insert("modified".to_string(), json!(note.modified));
        if let Some(text) = &note.text {
            entry.insert("text".to_string(), json!(text));
        }
        if let Some(digest) = &note.initial_digest {
            entry.insert("initialDigest".to_string(), json!(digest));
        }
    }
    if let Some(link) = link {
        let org = if link.org.is_empty() { "-" } else { &link.org };
        let reference = json!({
            "org": org,
            "id": link.id.to_string(),
            "name": link.name,
            "initialDigest": link.initial_digest,
            "url": link.notes_path(public_url),
        });
        entry.insert("publication".to_string(), reference);
    }
    (Value::Object(entry), link.is_none_or(|link| link.public))
}

/// Order versions newest first: by semver, then any that are not semver.
fn newest_first(left: &str, right: &str) -> std::cmp::Ordering {
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => right.cmp(&left),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => right.cmp(left),
    }
}
