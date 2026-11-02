//! `GET {base}/{name}/-/notes`: every note of a package in one cacheable
//! document, newest version first.

use super::{
    NotesTarget, notes_target,
    responses::{PublicationLink, with_cache_rules},
};
use crate::server::{AppState, Identity, publications::readable_publication_note};
use axum::{
    Json,
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_storage::metadata::Note;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Most entries the document lists.
const MAX_ENTRIES: usize = 100;
/// Most bytes of serialized entries the document holds.
const MAX_BYTES: usize = 1024 * 1024;

pub(in crate::server) async fn serve_package_notes(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let notes = match notes_target(state, identity, registry, raw_name, false).await {
        Ok(notes) => notes,
        Err(response) => return response,
    };
    match package_notes(state, identity, &notes).await {
        Ok(response) => response,
        Err(err) => err.into_response(),
    }
}

async fn package_notes(
    state: &AppState,
    identity: &Identity,
    notes: &NotesTarget,
) -> Result<Response, RegistryError> {
    let NotesTarget { database, name, target } = notes;
    let mut versions: BTreeMap<String, (Option<Note>, Option<PublicationLink>)> = BTreeMap::new();
    for (version, note) in database.package_notes(name, &target.org).await? {
        versions.entry(version).or_default().0 = Some(note);
    }
    let mut links: BTreeMap<u64, Option<PublicationLink>> = BTreeMap::new();
    for (version, id) in database.package_publications(name, &target.org).await? {
        if let std::collections::btree_map::Entry::Vacant(vacant) = links.entry(id) {
            let link = match database.publication(&target.org, &id.to_string()).await? {
                Some(publication) => {
                    readable_publication_note(state, identity, database, publication)
                        .await?
                        .map(PublicationLink::from)
                }
                None => None,
            };
            vacant.insert(link);
        }
        if let Some(Some(link)) = links.get(&id) {
            versions.entry(version).or_default().1 = Some(link.clone());
        }
    }
    let package_public =
        crate::server::publications::anonymously_readable(state, &target.org, name);
    Ok(listing_response(versions, package_public))
}

fn listing_response(
    versions: BTreeMap<String, (Option<Note>, Option<PublicationLink>)>,
    package_public: bool,
) -> Response {
    let mut ordered: Vec<_> = versions.into_iter().collect();
    ordered.sort_by(|(left, _), (right, _)| newest_first(left, right));
    let mut listing = Listing { public: package_public, ..Listing::default() };
    for (version, (note, link)) in ordered {
        let (entry, entry_public) = entry_json(&version, note.as_ref(), link.as_ref());
        if !listing.push(entry, entry_public) {
            break;
        }
    }
    listing.into_response()
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
fn entry_json(version: &str, note: Option<&Note>, link: Option<&PublicationLink>) -> (Value, bool) {
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
