use super::{BTreeMap, DocumentMerge, HashSet, Map, Result, Value, now_iso};
/// Merge an incoming publish manifest into the existing on-disk
/// packument. The result is what we'll write back to
/// `<storage>/<pkg>/package.json`.
///
/// Merge rules (chosen to match verdaccio's behavior for the cases
/// `@pnpm/registry-mock`'s publish script exercises):
///
/// * `name`, `_id` — copied from the new body.
/// * `versions` — union; an already-hosted version is immutable except for
///   its `deprecated` flag (see `merge_versions`), an upstream-only or
///   new version is taken from the body. `hosted` is the locally hosted
///   packument (or `None`); `existing` is the merge seed, which may instead
///   be the upstream packument, so immutability keys off `hosted`, not it.
/// * `dist-tags` — union, new body overrides on key collision.
/// * `time` — union, new entries override on key collision.
///   `time.modified` is always bumped to "now".
/// * Other top-level keys (`description`, `readme`, `maintainers`,
///   `users`, etc.) come from the new body when present, falling
///   back to the existing packument otherwise.
pub fn merge_manifest(
    existing: Option<&Value>,
    incoming: &Value,
    hosted: Option<&Value>,
    now_iso: &str,
) -> Value {
    let mut out = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };

    if let Some(incoming_obj) = incoming.as_object() {
        for (key, value) in incoming_obj {
            merge_manifest_field(&mut out, key, value, hosted);
        }
    }

    stamp_time_entries(&mut out, now_iso);

    hoist_readme_from_latest(&mut out);

    // Sort `versions` by semver-ish key order so the on-disk file is
    // stable across runs. Use a BTreeMap to take advantage of
    // serde_json's `preserve_order` feature — without sorting, two
    // publishes of the same package can produce different bytes.
    if let Some(versions) = out.get_mut("versions").and_then(Value::as_object_mut) {
        let sorted: BTreeMap<String, Value> = versions
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        *versions = sorted.into_iter().collect();
    }

    Value::Object(out)
}

/// Copy the `readme` / `readmeFilename` of the `dist-tags.latest` version up to
/// the packument top level, where full-packument consumers and registry UIs
/// read it. Publish clients (`libnpmpublish`, pacquet) only send the readme
/// inside `versions[<version>]`, so without this a published package would
/// expose no top-level readme — this hoist matches npm and verdaccio.
///
/// A top-level value already present (e.g. from an earlier publish or the
/// upstream packument) is left in place when the latest version carries no
/// readme, so a metadata-only re-publish never blanks it.
fn hoist_readme_from_latest(out: &mut Map<String, Value>) {
    let Some(latest) = out
        .get("dist-tags")
        .and_then(|tags| tags.get("latest"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let hoisted: Vec<(String, Value)> = out
        .get("versions")
        .and_then(|versions| versions.get(&latest))
        .map(|version| {
            ["readme", "readmeFilename"]
                .into_iter()
                .filter_map(|key| {
                    version
                        .get(key)
                        .filter(|value| !value.is_null())
                        .cloned()
                        .map(|value| (key.to_owned(), value))
                })
                .collect()
        })
        .unwrap_or_default();
    for (key, value) in hoisted {
        out.insert(key, value);
    }
}

fn merge_objects(existing: Option<&Value>, incoming: &Value) -> Value {
    let mut merged = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };
    if let Some(incoming_obj) = incoming.as_object() {
        for (key, value) in incoming_obj {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

/// Merge the `versions` map onto the seed `existing`. A version that is
/// already **hosted** is immutable except for its `deprecated` flag: the
/// hosted manifest is kept and only `deprecated` is applied from the body
/// (set it, or remove it for undeprecate), so `dist`, `dependencies`, `bin`,
/// `engines` and every other resolution-relevant field stay as published. A
/// malformed incoming entry for a hosted version is ignored. Any other
/// version — brand-new, or one that exists only upstream in the seed — is
/// taken from the body, so its manifest matches the tarball being published.
///
/// Immutability keys off `hosted` (the locally hosted packument), not the
/// seed: `existing` may be the upstream packument, and upstream versions are
/// not immutable here — a first local publish of one must win.
/// Merge one top-level packument field into the document being written.
fn merge_manifest_field(
    out: &mut Map<String, Value>,
    key: &String,
    value: &Value,
    hosted: Option<&Value>,
) {
    let merged = match key.as_str() {
        "versions" => merge_versions(out.get(key), value, hosted),
        "dist-tags" | "time" => merge_objects(out.get(key), value),
        // Already stripped by extract_attachments; if it slips through
        // somehow, drop it so we don't persist base64 blobs alongside the
        // packument.
        "_attachments" => return,
        _ => value.clone(),
    };
    out.insert(key.clone(), merged);
}

/// Synthesize time entries for any new version that didn't get one supplied
/// by the client. pnpm reads `time.modified` for freshness checks, so it must
/// always be present.
fn stamp_time_entries(out: &mut Map<String, Value>, now_iso: &str) {
    let version_ids: Vec<String> = out
        .get("versions")
        .and_then(Value::as_object)
        .map(|versions| {
            versions
                .keys()
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let time_entry = out
        .entry("time".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(time_obj) = time_entry.as_object_mut() else {
        return;
    };
    time_obj.insert("modified".to_string(), Value::String(now_iso.to_string()));
    time_obj
        .entry("created".to_string())
        .or_insert_with(|| Value::String(now_iso.to_string()));
    for version_id in version_ids {
        time_obj
            .entry(version_id)
            .or_insert_with(|| Value::String(now_iso.to_string()));
    }
}

fn merge_versions(existing: Option<&Value>, incoming: &Value, hosted: Option<&Value>) -> Value {
    let hosted_versions = hosted
        .and_then(|h| h.get("versions"))
        .and_then(Value::as_object);
    let mut merged = match existing {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };
    if let Some(incoming_obj) = incoming.as_object() {
        for (version, manifest) in incoming_obj {
            let hosted_manifest = hosted_versions
                .and_then(|versions| versions.get(version))
                .and_then(Value::as_object);
            let entry = match (hosted_manifest, manifest.as_object()) {
                (Some(hosted_manifest), Some(incoming_manifest)) => Value::Object(
                    with_incoming_deprecation(hosted_manifest, incoming_manifest),
                ),
                // Malformed incoming entry for a hosted version: keep the
                // hosted manifest rather than overwrite it with junk.
                (Some(hosted_manifest), None) => Value::Object(hosted_manifest.clone()),
                _ => manifest.clone(),
            };
            merged.insert(version.clone(), entry);
        }
    }
    Value::Object(merged)
}

/// The hosted manifest with only its deprecation taken from the incoming one.
/// A publish may deprecate or un-deprecate a version but never rewrite what
/// the store already holds for it.
fn with_incoming_deprecation(
    hosted_manifest: &Map<String, Value>,
    incoming_manifest: &Map<String, Value>,
) -> Map<String, Value> {
    let mut updated = hosted_manifest.clone();
    match incoming_manifest.get("deprecated") {
        Some(deprecated) => updated.insert("deprecated".to_string(), deprecated.clone()),
        None => updated.remove("deprecated"),
    };
    updated
}

/// The npm half of the journal's [`crate::journal::HostedDocuments`]: merge a
/// journaled packument into the packument the store holds, minus the versions
/// whose tarball the transaction lost. Publishing and startup recovery both
/// land here, so a re-applied transaction adds its versions to whatever was
/// published in the meantime instead of erasing it.
pub fn merge_journaled_packument(merge: &DocumentMerge<'_>) -> Result<Option<Vec<u8>>> {
    let mut journaled: Value = serde_json::from_slice(merge.journaled)?;
    if !merge.lost_blobs.is_empty() {
        let lost_versions = merge.lost_blobs
            .iter()
            .map(|filename| {
                merge.name
                    .parse_tarball_name(filename)
                    .map(|(_, version)| version)
            })
            .collect::<Result<HashSet<String>>>()?;
        drop_lost_versions(&mut journaled, &lost_versions);
    }
    let existing: Option<Value> = match merge.existing {
        Some(bytes) => Some(serde_json::from_slice(bytes)?),
        None => None,
    };
    let merged = merge_manifest(existing.as_ref(), &journaled, existing.as_ref(), &now_iso());
    Ok(Some(serde_json::to_vec_pretty(&merged)?))
}

/// Drop from a journaled packument every version the transaction could not
/// place: one whose tarball slot another writer already owned, or that could
/// not reserve a bounded digest-reference slot. The journal keeps each staged
/// attachment's canonical filename, so callers resolve that name to the
/// version before reaching this helper instead of trusting a publisher-
/// supplied `dist.tarball` URL as the transaction identity.
pub(super) fn drop_lost_versions(journaled: &mut Value, lost: &HashSet<String>) {
    let Some(versions) = journaled.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    let mut removed_versions = HashSet::new();
    versions.retain(|version, _| {
        let keep = !lost.contains(version);
        if !keep {
            removed_versions.insert(version.clone());
        }
        keep
    });

    if let Some(tags) = journaled.get_mut("dist-tags").and_then(Value::as_object_mut) {
        tags.retain(|_, version| {
            version
                .as_str()
                .is_none_or(|version| !removed_versions.contains(version))
        });
    }
    if let Some(time) = journaled.get_mut("time").and_then(Value::as_object_mut) {
        time.retain(|version, _| !removed_versions.contains(version));
    }
}
