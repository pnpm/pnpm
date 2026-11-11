//! OCI repository documents decomposed into `packages`, `oci_manifests`, and
//! `oci_tags`.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    cargo_rows::{
        bad_request, check_format_version, insert_owners, take_optional_string, take_owners,
    },
    commit::{Commit, PackageRow, parse_json},
    removed, to_column, to_generation,
};
use pnpr_error::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub(super) const OCI_STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct OciEnvelope {
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    owners: Vec<String>,
}

pub(super) struct StoredOci {
    manifests: BTreeMap<String, Map<String, Value>>,
    tags: BTreeMap<String, (String, u64)>,
}

struct OciRows {
    state_version: u32,
    envelope: OciEnvelope,
    manifests: BTreeMap<String, IncomingManifest>,
    tags: BTreeMap<String, (String, u64)>,
}

struct IncomingManifest {
    entry: Map<String, Value>,
    children: Vec<String>,
}

pub(super) fn queries(key: &PackageKey) -> [Query<'_>; 2] {
    let (namespace, ecosystem, name) = key.params();
    [
        query!(
            "SELECT digest, entry FROM oci_manifests
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
            namespace,
            ecosystem,
            name,
        ),
        query!(
            "SELECT tag, digest, updated FROM oci_tags
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
            namespace,
            ecosystem,
            name,
        ),
    ]
}

pub(super) fn from_rows(manifest_rows: Vec<Row>, tag_rows: Vec<Row>) -> Result<StoredOci> {
    let mut manifests = BTreeMap::new();
    for mut row in manifest_rows {
        let digest: String = row.get(0)?;
        let entry: Map<String, Value> = parse_json(&row.get::<String>(1)?)?;
        manifests.insert(digest, entry);
    }
    let mut tags = BTreeMap::new();
    for mut row in tag_rows {
        let tag: String = row.get(0)?;
        let digest: String = row.get(1)?;
        let updated = to_generation(row.get(2)?);
        tags.insert(tag, (digest, updated));
    }
    Ok(StoredOci { manifests, tags })
}

pub(super) fn apply(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    mut stored: StoredOci,
    bytes: &[u8],
    principal: Option<&str>,
) -> Result<bool> {
    let Some(incoming) = decompose(commit.key, bytes)? else {
        return clear_repository(commit, row, &stored, principal);
    };
    validate_incoming(&stored, &incoming)?;
    let mut changed = commit.write_envelope(row, incoming.state_version, &incoming.envelope)?;
    changed |= upsert_manifests(commit, &mut stored.manifests, &incoming.manifests)?;
    changed |= reconcile_tags(commit, &mut stored.tags, &incoming.tags)?;
    changed |= remove_dropped_manifests(commit, &stored.manifests)?;
    Ok(changed)
}

fn clear_repository(
    commit: &Commit<'_>,
    row: Option<&PackageRow>,
    stored: &StoredOci,
    principal: Option<&str>,
) -> Result<bool> {
    let Some(current) = row else { return Ok(false) };
    removed::record_package(commit.transaction, commit.key, &current.state, principal)?;
    removed::record_removed_oci_digests(
        commit.transaction,
        commit.key,
        commit.generation,
        stored.manifests.keys().map(String::as_str),
    )?;
    let (namespace, ecosystem, name) = commit.key.params();
    commit.transaction.execute(
        "DELETE FROM oci_tags WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        params![namespace, ecosystem, name],
    )?;
    commit.transaction.execute(
        "DELETE FROM oci_manifests WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
        params![namespace, ecosystem, name],
    )?;
    commit.delete_state()?;
    Ok(true)
}

fn validate_incoming(stored: &StoredOci, incoming: &OciRows) -> Result<()> {
    for (tag, (digest, _)) in &incoming.tags {
        if !incoming.manifests.contains_key(digest) {
            return Err(bad_request(&format!(
                "OCI tag {tag:?} points to missing manifest {digest:?}",
            )));
        }
    }
    for (digest, manifest) in &incoming.manifests {
        for child in &manifest.children {
            if stored.manifests.contains_key(child) && !incoming.manifests.contains_key(child) {
                return Err(bad_request(&format!(
                    "OCI index {digest:?} references removed child manifest {child:?}",
                )));
            }
        }
    }
    Ok(())
}

fn upsert_manifests(
    commit: &Commit<'_>,
    stored: &mut BTreeMap<String, Map<String, Value>>,
    incoming: &BTreeMap<String, IncomingManifest>,
) -> Result<bool> {
    let mut changed = false;
    let (namespace, ecosystem, name) = commit.key.params();
    for (digest, manifest) in incoming {
        match stored.remove(digest) {
            Some(existing) if existing == manifest.entry => {}
            Some(_) => {
                let entry = serde_json::to_string(&manifest.entry)?;
                commit.transaction.execute(
                    "UPDATE oci_manifests SET entry = ?5
                     WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND digest = ?4",
                    params![namespace, ecosystem, name, digest, entry],
                )?;
                changed = true;
            }
            None => {
                let entry = serde_json::to_string(&manifest.entry)?;
                commit.transaction.execute(
                    "INSERT INTO oci_manifests (namespace, ecosystem, name, digest, entry)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![namespace, ecosystem, name, digest, entry],
                )?;
                changed = true;
            }
        }
    }
    Ok(changed)
}

fn reconcile_tags(
    commit: &Commit<'_>,
    stored: &mut BTreeMap<String, (String, u64)>,
    incoming: &BTreeMap<String, (String, u64)>,
) -> Result<bool> {
    let mut changed = false;
    let (namespace, ecosystem, name) = commit.key.params();
    for (tag, target) in incoming {
        if stored.remove(tag).as_ref() != Some(target) {
            commit.transaction.execute(
                "INSERT INTO oci_tags (namespace, ecosystem, name, tag, digest, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (namespace, ecosystem, name, tag) DO UPDATE SET
                   digest = excluded.digest, updated = excluded.updated",
                params![namespace, ecosystem, name, tag, &target.0, to_column(target.1)],
            )?;
            changed = true;
        }
    }
    for tag in stored.keys() {
        commit.transaction.execute(
            "DELETE FROM oci_tags
             WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3 AND tag = ?4",
            params![namespace, ecosystem, name, tag],
        )?;
        changed = true;
    }
    Ok(changed)
}

fn remove_dropped_manifests(
    commit: &Commit<'_>,
    dropped: &BTreeMap<String, Map<String, Value>>,
) -> Result<bool> {
    if dropped.is_empty() {
        return Ok(false);
    }
    removed::record_removed_oci_digests(
        commit.transaction,
        commit.key,
        commit.generation,
        dropped.keys().map(String::as_str),
    )?;
    let (namespace, ecosystem, name) = commit.key.params();
    let keys: Vec<&str> = dropped
        .keys()
        .map(String::as_str)
        .collect();
    let digests = super::blobs::json_array(&keys);
    commit.transaction.execute(
        "DELETE FROM oci_manifests
         WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3
           AND digest IN (SELECT value FROM json_each(?4))",
        params![namespace, ecosystem, name, &digests],
    )?;
    Ok(true)
}

fn decompose(key: &PackageKey, bytes: &[u8]) -> Result<Option<OciRows>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let Value::Object(mut doc) = serde_json::from_slice(bytes)? else {
        return Err(bad_request("an OCI image document must be an object"));
    };
    let state_version = check_format_version(key, &doc, OCI_STATE_VERSION)?;
    let name =
        take_optional_string(&mut doc, "name")?.unwrap_or_else(|| key.name.clone());
    let owners = take_owners(&mut doc)?;
    let manifests = parse_manifests(doc.remove("manifests"))?;
    let tags = parse_tags(doc.remove("tags"))?;
    let envelope = OciEnvelope { name, owners };
    Ok(Some(OciRows { state_version, envelope, manifests, tags }))
}

fn parse_manifests(raw: Option<Value>) -> Result<BTreeMap<String, IncomingManifest>> {
    let Some(raw) = raw else { return Ok(BTreeMap::new()) };
    let Value::Array(items) = raw else {
        return Err(bad_request("OCI `manifests` must be an array"));
    };
    let mut manifests = BTreeMap::new();
    for item in items {
        let (digest, parsed) = parse_manifest_entry(item)?;
        if manifests.insert(digest.clone(), parsed).is_some() {
            return Err(bad_request(&format!("duplicate OCI manifest {digest:?}")));
        }
    }
    Ok(manifests)
}

fn parse_manifest_entry(item: Value) -> Result<(String, IncomingManifest)> {
    let Value::Object(entry) = item else {
        return Err(bad_request("each OCI manifest must be an object"));
    };
    let digest = entry
        .get("digest")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad_request("OCI manifest must carry a `digest` string"))?
        .to_string();
    let mut children = Vec::new();
    if let Some(raw_children) = entry.get("children") {
        let list = raw_children
            .as_array()
            .ok_or_else(|| bad_request("OCI manifest `children` must be an array"))?;
        for child in list {
            let child = child
                .as_str()
                .ok_or_else(|| bad_request("OCI manifest child must be a digest string"))?;
            children.push(child.to_string());
        }
    }
    Ok((digest, IncomingManifest { entry, children }))
}

fn parse_tags(raw: Option<Value>) -> Result<BTreeMap<String, (String, u64)>> {
    let Some(raw) = raw else { return Ok(BTreeMap::new()) };
    let Value::Array(items) = raw else {
        return Err(bad_request("OCI `tags` must be an array"));
    };
    let mut tags = BTreeMap::new();
    for item in items {
        let Value::Object(entry) = item else {
            return Err(bad_request("each OCI tag must be an object"));
        };
        let tag = required_str(&entry, "tag")?.to_string();
        let digest = required_str(&entry, "digest")?.to_string();
        let updated = entry
            .get("updated")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad_request("OCI tag must carry an `updated` timestamp"))?;
        if tags
            .insert(tag.clone(), (digest, updated))
            .is_some()
        {
            return Err(bad_request(&format!("duplicate OCI tag {tag:?}")));
        }
    }
    Ok(tags)
}

fn required_str<'entry>(entry: &'entry Map<String, Value>, field: &str) -> Result<&'entry str> {
    entry
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad_request(&format!("OCI tag must carry a `{field}` string")))
}

pub(super) fn view_queries<'query>(
    key: &'query PackageKey,
    unless: &'query i64,
) -> [Query<'query>; 2] {
    let (namespace, ecosystem, name) = key.params();
    [
        query!(
            "SELECT m.entry FROM oci_manifests m
             WHERE m.namespace = ?1 AND m.ecosystem = ?2 AND m.name = ?3
               AND NOT EXISTS (
                 SELECT 1 FROM packages p
                 WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)
             ORDER BY m.digest",
            namespace,
            ecosystem,
            name,
            unless,
        ),
        query!(
            "SELECT t.tag, t.digest, t.updated FROM oci_tags t
             WHERE t.namespace = ?1 AND t.ecosystem = ?2 AND t.name = ?3
               AND NOT EXISTS (
                 SELECT 1 FROM packages p
                 WHERE p.namespace = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)
             ORDER BY t.tag",
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
    let envelope: OciEnvelope = serde_json::from_slice(&package.state)?;
    let (manifest_rows, tag_rows) = if let Some(pair) = listed {
        pair
    } else {
        let mut results = connection
            .query_all(&view_queries(key, &no_generation))?
            .into_iter();
        (results.next().unwrap_or_default(), results.next().unwrap_or_default())
    };
    let manifests: Vec<Value> = manifest_rows
        .into_iter()
        .map(|mut row| parse_json(&row.get::<String>(0)?))
        .collect::<Result<_>>()?;
    let tags: Vec<Value> = tag_rows
        .into_iter()
        .map(|mut row| {
            let tag: String = row.get(0)?;
            let digest: String = row.get(1)?;
            let updated = to_generation(row.get(2)?);
            Ok(json!({ "tag": tag, "digest": digest, "updated": updated }))
        })
        .collect::<Result<_>>()?;
    let mut rendered = Map::new();
    rendered.insert("formatVersion".to_string(), Value::from(package.state_version));
    rendered.insert("name".to_string(), Value::String(envelope.name));
    rendered.insert("manifests".to_string(), Value::Array(manifests));
    rendered.insert("tags".to_string(), Value::Array(tags));
    insert_owners(&mut rendered, envelope.owners);
    Ok(serde_json::to_vec(&rendered)?)
}
