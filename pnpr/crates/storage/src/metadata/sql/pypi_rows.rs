//! Python project documents decomposed into `packages`, `pypi_files`, and
//! `removed_pypi_files`.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    cargo_rows::{
        bad_request, check_format_version, insert_owners, take_optional_string, take_owners,
    },
    commit::{Commit, PackageRow, parse_json},
    removed, to_column,
};
use pnpr_error::{RegistryError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const PYPI_STATE_VERSION: u32 = 2;
const YANKED: &str = "yanked";

#[derive(Serialize, Deserialize)]
struct PypiEnvelope {
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    owners: Vec<String>,
}

pub(super) struct StoredPypi {
    files: BTreeMap<String, StoredPypiFile>,
    removed_files: BTreeSet<String>,
}

struct StoredPypiFile {
    position: i64,
    yanked: bool,
    yank_reason: Option<String>,
    entry: Map<String, Value>,
}

struct PypiRows {
    state_version: u32,
    envelope: PypiEnvelope,
    files: Vec<IncomingPypiFile>,
    deleted_files: BTreeSet<String>,
}

struct IncomingPypiFile {
    filename: String,
    yanked: bool,
    yank_reason: Option<String>,
    entry: Map<String, Value>,
}

pub(super) fn queries(key: &PackageKey) -> [Query<'_>; 2] {
    let (namespace, ecosystem, name) = key.params();
    [
        query!(
            "SELECT filename, position, yanked, yank_reason, entry FROM pypi_files
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 ORDER BY position",
            namespace,
            ecosystem,
            name,
        ),
        query!(
            "SELECT filename FROM removed_pypi_files
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
            namespace,
            ecosystem,
            name,
        ),
    ]
}

pub(super) fn from_rows(file_rows: Vec<Row>, removed_rows: Vec<Row>) -> Result<StoredPypi> {
    let mut files = BTreeMap::new();
    for mut row in file_rows {
        let filename: String = row.get(0)?;
        let position: i64 = row.get(1)?;
        let yanked = row.get::<i64>(2)? != 0;
        let yank_reason: Option<String> = row.get(3)?;
        let mut entry: Map<String, Value> = parse_json(&row.get::<String>(4)?)?;
        canonicalize_pypi_entry(&mut entry);
        files.insert(filename, StoredPypiFile { position, yanked, yank_reason, entry });
    }
    let removed_files = removed_rows
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    Ok(StoredPypi { files, removed_files })
}

pub(super) fn apply(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    mut stored: StoredPypi,
    bytes: &[u8],
    principal: Option<&str>,
) -> Result<bool> {
    let Some(incoming) = decompose(commit.key, bytes)? else {
        return clear_project(commit, row, principal);
    };
    validate_incoming(&stored, &incoming)?;
    let mut changed = commit.write_envelope(row, incoming.state_version, &incoming.envelope)?;
    let mut max_position = stored.files
        .values()
        .map(|existing| existing.position)
        .max()
        .unwrap_or(-1);
    for item in incoming.files {
        if let Some(existing) = stored.files.remove(&item.filename) {
            changed |= update_yank(commit, &existing, &item)?;
        } else {
            max_position += 1;
            insert_pypi_file(commit, &item, max_position)?;
            changed = true;
        }
    }
    changed |= retire_deleted_files(
        commit,
        &stored.files,
        &stored.removed_files,
        &incoming.deleted_files,
    )?;
    Ok(changed)
}

fn clear_project(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    principal: Option<&str>,
) -> Result<bool> {
    let Some(current) = row else { return Ok(false) };
    removed::record_package(commit.transaction, commit.key, &current.state, principal)?;
    let (namespace, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        removed::RECORD_REMOVED_PYPI_FILES.get(commit.transaction.dialect()),
        params![namespace, ecosystem, name, to_column(commit.generation)],
    )?;
    commit.transaction.execute(
        "DELETE FROM pypi_files WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        params![namespace, ecosystem, name],
    )?;
    commit.delete_state()?;
    Ok(true)
}

fn validate_incoming(stored: &StoredPypi, incoming: &PypiRows) -> Result<()> {
    for item in &incoming.files {
        if stored.removed_files.contains(&item.filename)
            || incoming.deleted_files.contains(&item.filename)
        {
            return Err(file_exists(&item.filename));
        }
        if let Some(existing) = stored.files.get(&item.filename)
            && existing.entry != item.entry
        {
            return Err(file_exists(&item.filename));
        }
    }
    Ok(())
}

fn file_exists(filename: &str) -> RegistryError {
    RegistryError::BadRequest { reason: format!("File already exists: {filename:?}") }
}

fn update_yank(
    commit: &Commit<'_>,
    existing: &StoredPypiFile,
    incoming: &IncomingPypiFile,
) -> Result<bool> {
    if existing.yanked == incoming.yanked && existing.yank_reason == incoming.yank_reason {
        return Ok(false);
    }
    let (namespace, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "UPDATE pypi_files SET yanked = ?5, yank_reason = ?6
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND filename = ?4",
        params![
            namespace,
            ecosystem,
            name,
            &incoming.filename,
            i64::from(incoming.yanked),
            incoming.yank_reason.as_deref(),
        ],
    )?;
    Ok(true)
}

fn insert_pypi_file(commit: &Commit<'_>, item: &IncomingPypiFile, position: i64) -> Result<()> {
    let (namespace, ecosystem, name) = commit.key.params();
    let entry = serde_json::to_string(&item.entry)?;
    commit.transaction.execute(
        "INSERT INTO pypi_files
           (namespace, ecosystem, name, filename, position, yanked, yank_reason, entry)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            namespace,
            ecosystem,
            name,
            &item.filename,
            position,
            i64::from(item.yanked),
            item.yank_reason.as_deref(),
            entry,
        ],
    )
}

fn retire_deleted_files(
    commit: &Commit<'_>,
    dropped: &BTreeMap<String, StoredPypiFile>,
    already_removed: &BTreeSet<String>,
    declared_deleted: &BTreeSet<String>,
) -> Result<bool> {
    let (namespace, ecosystem, name) = commit.key.params();
    if !dropped.is_empty() {
        let keys: Vec<&str> = dropped
            .keys()
            .map(String::as_str)
            .collect();
        let filenames = super::blobs::json_array(&keys);
        commit.transaction.execute(
            removed::DELETE_PYPI_FILE_LIST.get(commit.transaction.dialect()),
            params![namespace, ecosystem, name, &filenames],
        )?;
    }
    let to_record: Vec<&str> = dropped
        .keys()
        .chain(
            declared_deleted
                .iter()
                .filter(|name| !already_removed.contains(*name) && !dropped.contains_key(*name)),
        )
        .map(String::as_str)
        .collect();
    if to_record.is_empty() {
        return Ok(!dropped.is_empty());
    }
    let filenames = super::blobs::json_array(&to_record);
    commit.transaction.execute(
        removed::RECORD_REMOVED_PYPI_FILE_LIST.get(commit.transaction.dialect()),
        params![namespace, ecosystem, name, &filenames, to_column(commit.generation)],
    )?;
    Ok(true)
}

fn decompose(key: &PackageKey, bytes: &[u8]) -> Result<Option<PypiRows>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let Value::Object(mut doc) = serde_json::from_slice(bytes)? else {
        return Err(bad_request("a project document must be an object"));
    };
    let state_version = check_format_version(key, &doc, PYPI_STATE_VERSION)?;
    let name =
        take_optional_string(&mut doc, "name")?.unwrap_or_else(|| key.name.clone());
    let owners = take_owners(&mut doc)?;
    let files = parse_files(doc.remove("files"))?;
    let deleted_files = parse_deleted_files(doc.remove("deleted_files"))?;
    let envelope = PypiEnvelope { name, owners };
    Ok(Some(PypiRows { state_version, envelope, files, deleted_files }))
}

fn parse_files(raw: Option<Value>) -> Result<Vec<IncomingPypiFile>> {
    let Some(raw) = raw else { return Ok(Vec::new()) };
    let Value::Array(items) = raw else {
        return Err(bad_request("project `files` must be an array"));
    };
    let mut seen = BTreeSet::new();
    let mut files = Vec::with_capacity(items.len());
    for item in items {
        let parsed = parse_file_entry(item)?;
        if !seen.insert(parsed.filename.clone()) {
            return Err(bad_request(&format!("duplicate project file {:?}", parsed.filename)));
        }
        files.push(parsed);
    }
    Ok(files)
}

fn parse_file_entry(item: Value) -> Result<IncomingPypiFile> {
    let Value::Object(mut entry) = item else {
        return Err(bad_request("each project file must be an object"));
    };
    let filename = entry
        .get("filename")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| bad_request("project file must carry a `filename` string"))?
        .to_string();
    let (yanked, yank_reason) = match entry.shift_remove(YANKED) {
        None | Some(Value::Bool(false)) => (false, None),
        Some(Value::Bool(true)) => (true, None),
        Some(Value::String(reason)) => (true, Some(reason)),
        Some(_) => return Err(bad_request("project file `yanked` must be a boolean or string")),
    };
    canonicalize_pypi_entry(&mut entry);
    Ok(IncomingPypiFile { filename, yanked, yank_reason, entry })
}

fn canonicalize_pypi_entry(entry: &mut Map<String, Value>) {
    entry
        .entry("hashes")
        .or_insert_with(|| Value::Object(Map::new()));
    for optional in [
        "url",
        "requires-python",
        "size",
        "upload-time",
        "core-metadata",
        "dist-info-metadata",
        "data-dist-info-metadata",
    ] {
        if entry.get(optional).is_some_and(Value::is_null) {
            entry.shift_remove(optional);
        }
    }
}

fn parse_deleted_files(raw: Option<Value>) -> Result<BTreeSet<String>> {
    let Some(raw) = raw else { return Ok(BTreeSet::new()) };
    let Value::Array(items) = raw else {
        return Err(bad_request("`deleted_files` must be an array"));
    };
    items
        .into_iter()
        .map(|item| match item {
            Value::String(name) if !name.is_empty() => Ok(name),
            _ => Err(bad_request("`deleted_files` entries must be non-empty strings")),
        })
        .collect()
}

pub(super) fn view_queries<'query>(
    key: &'query PackageKey,
    unless: &'query i64,
) -> [Query<'query>; 2] {
    let (namespace, ecosystem, name) = key.params();
    [
        query!(
            "SELECT f.yanked, f.yank_reason, f.entry FROM pypi_files f
             WHERE f.namespace = ?1 AND f.ecosystem = ?2 AND f.name = ?3
               AND NOT EXISTS (
                 SELECT 1 FROM packages p
                 WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)
             ORDER BY f.position",
            namespace,
            ecosystem,
            name,
            unless,
        ),
        query!(
            "SELECT r.filename FROM removed_pypi_files r
             WHERE r.namespace = ?1 AND r.ecosystem = ?2 AND r.name = ?3
               AND NOT EXISTS (
                 SELECT 1 FROM packages p
                 WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)
             ORDER BY r.filename",
            namespace,
            ecosystem,
            name,
            unless,
        ),
    ]
}

pub(super) fn assemble(
    connection: &dyn Connection,
    key: &PackageKey,
    package: &PackageRow,
    listed: Option<(Vec<Row>, Vec<Row>)>,
    no_generation: i64,
) -> Result<Vec<u8>> {
    let envelope: PypiEnvelope = serde_json::from_slice(&package.state)?;
    let (file_rows, removed_rows) = if let Some(pair) = listed {
        pair
    } else {
        let mut results = connection
            .query_all(&view_queries(key, &no_generation))?
            .into_iter();
        (results.next().unwrap_or_default(), results.next().unwrap_or_default())
    };
    let files = render_files(file_rows)?;
    let deleted: Vec<Value> = removed_rows
        .into_iter()
        .map(|mut row| Ok(Value::String(row.get(0)?)))
        .collect::<Result<_>>()?;
    let mut rendered = Map::new();
    rendered.insert("formatVersion".to_string(), Value::from(package.state_version));
    rendered.insert("name".to_string(), Value::String(envelope.name));
    rendered.insert("files".to_string(), Value::Array(files));
    if !deleted.is_empty() {
        rendered.insert("deleted_files".to_string(), Value::Array(deleted));
    }
    insert_owners(&mut rendered, envelope.owners);
    Ok(serde_json::to_vec(&rendered)?)
}

fn render_files(rows: Vec<Row>) -> Result<Vec<Value>> {
    let mut files = Vec::with_capacity(rows.len());
    for mut row in rows {
        let yanked = row.get::<i64>(0)? != 0;
        let yank_reason: Option<String> = row.get(1)?;
        let mut entry: Map<String, Value> = parse_json(&row.get::<String>(2)?)?;
        let yank_value = match (yanked, yank_reason) {
            (false, _) => Value::Bool(false),
            (true, None) => Value::Bool(true),
            (true, Some(reason)) => Value::String(reason),
        };
        entry.insert(YANKED.to_string(), yank_value);
        files.push(Value::Object(entry));
    }
    Ok(files)
}
