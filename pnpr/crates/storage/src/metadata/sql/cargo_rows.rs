//! Cargo crate documents decomposed into `packages` and `cargo_versions`.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    commit::{Commit, PackageRow, parse_json},
    removed, to_column,
};
use pnpr_error::{RegistryError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CARGO_STATE_VERSION: u32 = 1;
const YANKED: &str = "yanked";

#[derive(Serialize, Deserialize)]
struct CargoEnvelope {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    owners: Vec<String>,
}

pub(super) struct StoredCargo {
    versions: BTreeMap<String, StoredCargoVersion>,
    removed: BTreeSet<String>,
}

struct StoredCargoVersion {
    position: i64,
    yanked: bool,
    entry: Map<String, Value>,
}

struct CargoRows {
    state_version: u32,
    envelope: CargoEnvelope,
    versions: Vec<IncomingCargoVersion>,
}

struct IncomingCargoVersion {
    version: String,
    yanked: bool,
    entry: Map<String, Value>,
}

pub(super) fn queries(key: &PackageKey) -> [Query<'_>; 2] {
    let (namespace, ecosystem, name) = key.params();
    [
        query!(
            "SELECT version, position, yanked, entry FROM cargo_versions
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 ORDER BY position",
            namespace,
            ecosystem,
            name,
        ),
        query!(
            "SELECT version FROM removed_versions
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
            namespace,
            ecosystem,
            name,
        ),
    ]
}

pub(super) fn from_rows(version_rows: Vec<Row>, removed_rows: Vec<Row>) -> Result<StoredCargo> {
    let mut versions = BTreeMap::new();
    for mut row in version_rows {
        let version: String = row.get(0)?;
        let position: i64 = row.get(1)?;
        let yanked = row.get::<i64>(2)? != 0;
        let mut entry: Map<String, Value> = parse_json(&row.get::<String>(3)?)?;
        canonicalize_cargo_entry(&mut entry);
        versions.insert(version, StoredCargoVersion { position, yanked, entry });
    }
    let removed = removed_rows
        .into_iter()
        .map(|mut row| row.get(0))
        .collect::<Result<_>>()?;
    Ok(StoredCargo { versions, removed })
}

pub(super) fn apply(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    mut stored: StoredCargo,
    bytes: &[u8],
    principal: Option<&str>,
) -> Result<bool> {
    let Some(incoming) = decompose(commit.key, bytes)? else {
        return clear_crate(commit, row, principal);
    };
    validate_incoming(commit.key, &stored, &incoming.versions)?;
    let mut changed = commit.write_envelope(row, incoming.state_version, &incoming.envelope)?;
    let mut max_position = stored.versions
        .values()
        .map(|existing| existing.position)
        .max()
        .unwrap_or(-1);
    for item in incoming.versions {
        if let Some(existing) = stored.versions.remove(&item.version) {
            changed |= update_yanked(commit, &item.version, existing.yanked, item.yanked)?;
        } else {
            max_position += 1;
            insert_cargo_version(commit, &item, max_position)?;
            changed = true;
        }
    }
    changed |= remove_dropped_versions(commit, row, &stored.versions, principal)?;
    Ok(changed)
}

fn clear_crate(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    principal: Option<&str>,
) -> Result<bool> {
    let Some(current) = row else { return Ok(false) };
    removed::record_package(commit.transaction, commit.key, &current.state, principal)?;
    let (namespace, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
         SELECT namespace, ecosystem, name, version, ?4 FROM cargo_versions
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
         ON CONFLICT DO NOTHING",
        params![namespace, ecosystem, name, to_column(commit.generation)],
    )?;
    commit.transaction.execute(
        "DELETE FROM cargo_versions WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        params![namespace, ecosystem, name],
    )?;
    commit.delete_state()?;
    Ok(true)
}

fn validate_incoming(
    key: &PackageKey,
    stored: &StoredCargo,
    incoming: &[IncomingCargoVersion],
) -> Result<()> {
    for item in incoming {
        if stored.removed.contains(&item.version) {
            return Err(RegistryError::VersionAlreadyPublished {
                package: key.name.clone(),
                version: item.version.clone(),
            });
        }
        if let Some(existing) = stored.versions.get(&item.version)
            && existing.entry != item.entry
        {
            return Err(RegistryError::VersionAlreadyPublished {
                package: key.name.clone(),
                version: item.version.clone(),
            });
        }
    }
    Ok(())
}

fn update_yanked(
    commit: &Commit<'_>,
    version: &str,
    was_yanked: bool,
    is_yanked: bool,
) -> Result<bool> {
    if was_yanked == is_yanked {
        return Ok(false);
    }
    let (namespace, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "UPDATE cargo_versions SET yanked = ?5
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
        params![namespace, ecosystem, name, version, i64::from(is_yanked)],
    )?;
    Ok(true)
}

fn insert_cargo_version(
    commit: &Commit<'_>,
    item: &IncomingCargoVersion,
    position: i64,
) -> Result<()> {
    let (namespace, ecosystem, name) = commit.key.params();
    let entry = serde_json::to_string(&item.entry)?;
    commit.transaction.execute(
        "INSERT INTO cargo_versions (namespace, ecosystem, name, version, position, yanked, entry)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![namespace, ecosystem, name, &item.version, position, i64::from(item.yanked), entry,],
    )
}

fn remove_dropped_versions(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    dropped: &BTreeMap<String, StoredCargoVersion>,
    principal: Option<&str>,
) -> Result<bool> {
    if dropped.is_empty() {
        return Ok(false);
    }
    if let Some(current) = row {
        removed::record_package(commit.transaction, commit.key, &current.state, principal)?;
    }
    let (namespace, ecosystem, name) = commit.key.params();
    let keys: Vec<&str> = dropped
        .keys()
        .map(String::as_str)
        .collect();
    let versions = super::blobs::json_array(&keys);
    commit.transaction.execute(
        "INSERT INTO removed_versions (namespace, ecosystem, name, version, generation)
         SELECT ?1, ?2, ?3, value, ?5
         FROM (SELECT value FROM json_each(?4)) AS removed
         WHERE value != ''
         ON CONFLICT DO NOTHING",
        params![namespace, ecosystem, name, &versions, to_column(commit.generation)],
    )?;
    commit.transaction.execute(
        "DELETE FROM cargo_versions
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
           AND version IN (SELECT value FROM json_each(?4))",
        params![namespace, ecosystem, name, &versions],
    )?;
    Ok(true)
}

fn decompose(key: &PackageKey, bytes: &[u8]) -> Result<Option<CargoRows>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let Value::Object(mut doc) = serde_json::from_slice(bytes)? else {
        return Err(bad_request("a crate document must be an object"));
    };
    let state_version = check_format_version(key, &doc, CARGO_STATE_VERSION)?;
    let name =
        take_optional_string(&mut doc, "name")?.unwrap_or_else(|| key.name.clone());
    let description = take_nullable_string(&mut doc, "description")?;
    let owners = take_owners(&mut doc)?;
    let versions = parse_versions(doc.remove("versions"))?;
    let envelope = CargoEnvelope { name, description, owners };
    Ok(Some(CargoRows { state_version, envelope, versions }))
}

fn parse_versions(raw: Option<Value>) -> Result<Vec<IncomingCargoVersion>> {
    let Some(raw) = raw else { return Ok(Vec::new()) };
    let Value::Array(items) = raw else {
        return Err(bad_request("crate `versions` must be an array"));
    };
    let mut seen = BTreeSet::new();
    let mut versions = Vec::with_capacity(items.len());
    for item in items {
        let parsed = parse_version_entry(item)?;
        if !seen.insert(parsed.version.clone()) {
            return Err(bad_request(&format!("duplicate crate version {:?}", parsed.version)));
        }
        versions.push(parsed);
    }
    Ok(versions)
}

fn parse_version_entry(item: Value) -> Result<IncomingCargoVersion> {
    let Value::Object(mut entry) = item else {
        return Err(bad_request("each crate version must be an object"));
    };
    let version = entry
        .get("vers")
        .and_then(Value::as_str)
        .filter(|vers| !vers.is_empty())
        .ok_or_else(|| bad_request("crate version must carry a `vers` string"))?
        .to_string();
    let yanked = match entry.shift_remove(YANKED) {
        None => false,
        Some(Value::Bool(flag)) => flag,
        Some(_) => return Err(bad_request("crate version `yanked` must be a boolean")),
    };
    canonicalize_cargo_entry(&mut entry);
    Ok(IncomingCargoVersion { version, yanked, entry })
}

fn canonicalize_cargo_entry(entry: &mut Map<String, Value>) {
    entry
        .entry("deps")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(Value::Array(deps)) = entry.get_mut("deps") {
        for dep in deps {
            if let Value::Object(dep_map) = dep {
                canonicalize_cargo_dep(dep_map);
            }
        }
    }
    entry
        .entry("features")
        .or_insert_with(|| Value::Object(Map::new()));
    entry.entry("v").or_insert_with(|| Value::from(1_u32));
    for optional in ["links", "features2", "rust_version"] {
        if entry.get(optional).is_some_and(Value::is_null) {
            entry.shift_remove(optional);
        }
    }
}

fn canonicalize_cargo_dep(dep: &mut Map<String, Value>) {
    dep.entry("features")
        .or_insert_with(|| Value::Array(Vec::new()));
    dep.entry("optional").or_insert_with(|| Value::Bool(false));
    dep.entry("default_features").or_insert_with(|| Value::Bool(true));
    dep.entry("kind").or_insert_with(|| Value::String("normal".to_string()));
    for optional in ["target", "registry", "package"] {
        if dep.get(optional).is_some_and(Value::is_null) {
            dep.shift_remove(optional);
        }
    }
}

pub(super) fn view_query<'query>(key: &'query PackageKey, unless: &'query i64) -> Query<'query> {
    let (namespace, ecosystem, name) = key.params();
    query!(
        "SELECT v.yanked, v.entry FROM cargo_versions v
         WHERE v.namespace = ?1 AND v.ecosystem = ?2 AND v.name = ?3
           AND NOT EXISTS (
             SELECT 1 FROM packages p
             WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)
         ORDER BY v.position",
        namespace,
        ecosystem,
        name,
        unless,
    )
}

pub(super) fn assemble(
    connection: &dyn Connection,
    key: &PackageKey,
    package: &PackageRow,
    listed: Option<Vec<Row>>,
    no_generation: i64,
) -> Result<Vec<u8>> {
    let envelope: CargoEnvelope = serde_json::from_slice(&package.state)?;
    let rows = match listed {
        Some(rows) => rows,
        None => connection
            .query_all(&[view_query(key, &no_generation)])?
            .into_iter()
            .next()
            .unwrap_or_default(),
    };
    let mut versions = Vec::with_capacity(rows.len());
    for mut row in rows {
        let yanked = row.get::<i64>(0)? != 0;
        let mut entry: Map<String, Value> = parse_json(&row.get::<String>(1)?)?;
        entry.insert(YANKED.to_string(), Value::Bool(yanked));
        versions.push(Value::Object(entry));
    }
    let mut rendered = Map::new();
    rendered.insert("formatVersion".to_string(), Value::from(package.state_version));
    rendered.insert("name".to_string(), Value::String(envelope.name));
    rendered.insert("versions".to_string(), Value::Array(versions));
    if let Some(description) = envelope.description {
        rendered.insert("description".to_string(), Value::String(description));
    }
    insert_owners(&mut rendered, envelope.owners);
    Ok(serde_json::to_vec(&rendered)?)
}

pub(super) fn check_format_version(
    key: &PackageKey,
    doc: &Map<String, Value>,
    supported: u32,
) -> Result<u32> {
    let Some(raw) = doc.get("formatVersion") else {
        return Ok(supported);
    };
    let Some(version) = raw.as_u64() else {
        return Err(bad_request("`formatVersion` must be a positive integer"));
    };
    if version < 1 {
        return Err(bad_request("`formatVersion` must be at least 1"));
    }
    if version > u64::from(supported) {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.name.clone(),
            format_version: u32::try_from(version).unwrap_or(u32::MAX),
            supported_version: supported,
        });
    }
    Ok(u32::try_from(version).unwrap_or(supported))
}

pub(super) fn take_optional_string(
    doc: &mut Map<String, Value>,
    field: &str,
) -> Result<Option<String>> {
    match doc.remove(field) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(bad_request(&format!("`{field}` must be a string"))),
    }
}

/// The `owners` account IDs a crate, project, or image document records.
pub(super) fn take_owners(doc: &mut Map<String, Value>) -> Result<Vec<String>> {
    let Some(raw) = doc.remove("owners") else { return Ok(Vec::new()) };
    serde_json::from_value(raw).map_err(|_| bad_request("`owners` must be an array of strings"))
}

pub(super) fn insert_owners(rendered: &mut Map<String, Value>, owners: Vec<String>) {
    if !owners.is_empty() {
        rendered.insert("owners".to_string(), Value::from(owners));
    }
}

fn take_nullable_string(doc: &mut Map<String, Value>, field: &str) -> Result<Option<String>> {
    match doc.remove(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(bad_request(&format!("`{field}` must be a string or null"))),
    }
}

pub(super) fn bad_request(reason: &str) -> RegistryError {
    RegistryError::BadRequest { reason: reason.to_string() }
}
