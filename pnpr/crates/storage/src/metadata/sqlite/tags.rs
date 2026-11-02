//! Dist-tags and their insert-only history.

use super::{
    PackageKey,
    commit::{Commit, package_row},
    packument::TIME,
    to_column,
};
use crate::{
    metadata::{TagChange, TagUpdate},
    publish::now_iso,
};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use rusqlite::{Connection, params};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A [`TagChange`] the blocking pool can own.
#[derive(Debug)]
pub(super) struct OwnedTagChange {
    tag: String,
    target: Option<String>,
    principal: Option<String>,
}

impl From<TagChange<'_>> for OwnedTagChange {
    fn from(change: TagChange<'_>) -> Self {
        Self {
            tag: change.tag.to_string(),
            target: change.target.map(str::to_string),
            principal: change.principal.map(str::to_string),
        }
    }
}

/// One move of a tag, as its history records it.
pub(super) struct TagMove<'change> {
    pub(super) tag: &'change str,
    pub(super) previous: Option<&'change str>,
    pub(super) target: Option<&'change str>,
    pub(super) principal: Option<&'change str>,
}

pub(super) fn stored_tags(
    connection: &Connection,
    key: &PackageKey,
) -> rusqlite::Result<BTreeMap<String, String>> {
    let mut statement = connection.prepare_cached(
        "SELECT tag, version FROM tags WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
    )?;
    statement
        .query_map(key.params(), |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

/// Point the tag where `change` says and append the move to its history.
pub(super) fn record_tag_move(commit: &Commit<'_>, change: &TagMove<'_>) -> Result<()> {
    let (catalog, ecosystem, name) = commit.key.params();
    let generation = to_column(commit.generation);
    match change.target {
        Some(target) => commit.transaction.execute(
            "INSERT INTO tags (catalog, ecosystem, name, tag, version, generation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (catalog, ecosystem, name, tag)
             DO UPDATE SET version = excluded.version, generation = excluded.generation",
            params![catalog, ecosystem, name, change.tag, target, generation],
        )?,
        None => commit.transaction.execute(
            "DELETE FROM tags WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND tag = ?4",
            params![catalog, ecosystem, name, change.tag],
        )?,
    };
    commit.transaction.execute(
        "INSERT INTO tag_history (catalog, ecosystem, name, tag, generation, previous, target,
                                  principal, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            catalog,
            ecosystem,
            name,
            change.tag,
            generation,
            change.previous,
            change.target,
            change.principal,
            now_iso(),
        ],
    )?;
    Ok(())
}

/// Move one tag of an npm package in its own commit, stamping
/// `time.modified`. A change that leaves the tag where it is commits nothing.
pub(super) fn change_tag(
    connection: &mut Connection,
    key: &PackageKey,
    change: &OwnedTagChange,
) -> Result<TagUpdate> {
    if key.ecosystem != Ecosystem::Npm {
        return Ok(TagUpdate::NotFound);
    }
    let commit = Commit::begin(connection, key)?;
    let Some(package) = package_row(&commit.transaction, key)? else {
        return Ok(TagUpdate::NotFound);
    };
    let tags = stored_tags(&commit.transaction, key)?;
    let previous = tags.get(&change.tag).map(String::as_str);
    if previous == change.target.as_deref() {
        let generation = Some(key.generation(package.generation));
        return Ok(TagUpdate::Committed { generation, moved: false });
    }
    let tag_move = TagMove {
        tag: &change.tag,
        previous,
        target: change.target.as_deref(),
        principal: change.principal.as_deref(),
    };
    record_tag_move(&commit, &tag_move)?;
    commit.write_state(&stamp_modified(&package.state)?)?;
    let generation = key.generation(commit.generation);
    commit.finish()?;
    Ok(TagUpdate::Committed { generation: Some(generation), moved: true })
}

/// The package-level fields with `time.modified` set to now.
fn stamp_modified(state: &[u8]) -> Result<Vec<u8>> {
    let mut state: Map<String, Value> = serde_json::from_slice(state)?;
    let time = state
        .entry(TIME)
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(time) = time.as_object_mut() {
        time.insert("modified".to_string(), Value::String(now_iso()));
    }
    Ok(serde_json::to_vec(&state)?)
}

/// One recorded move of a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagHistoryEntry {
    /// The catalog generation that moved the tag.
    pub generation: u64,
    pub previous: Option<String>,
    /// `None` when the move removed the tag.
    pub target: Option<String>,
    pub principal: Option<String>,
    pub changed_at: String,
}

pub(super) fn tag_history(
    connection: &Connection,
    key: &PackageKey,
    tag: &str,
) -> Result<Vec<TagHistoryEntry>> {
    let mut statement = connection.prepare_cached(
        "SELECT generation, previous, target, principal, changed_at FROM tag_history
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND tag = ?4
         ORDER BY generation",
    )?;
    let (catalog, ecosystem, name) = key.params();
    let entries = statement.query_map(params![catalog, ecosystem, name, tag], |row| {
        Ok(TagHistoryEntry {
            generation: super::to_generation(row.get(0)?),
            previous: row.get(1)?,
            target: row.get(2)?,
            principal: row.get(3)?,
            changed_at: row.get(4)?,
        })
    })?;
    Ok(entries.collect::<rusqlite::Result<_>>()?)
}
