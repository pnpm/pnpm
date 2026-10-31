//! Split packuments (proof of concept). A client that accepts
//! `application/vnd.pnpr.install-v2+json` gets the abbreviated packument,
//! and when that is larger than the threshold, only the versions a fresh
//! resolution almost always picks stay in `versions`: every dist-tag target
//! and the highest release of each major (of each minor below 1.0). Every
//! other version moves to `pnpr.externalVersions`, which keeps only what
//! picking a version reads and what may change after publication: its
//! publish time, `deprecated` notice, trust rank, and selected revision. The
//! version itself, as its release record, is in the immutable version
//! document `<base>/<name>/<version>+r<revision>`. A client that does not
//! read the map sees only complete versions.
//!
//! `PNPR_SPLIT_DIST_ONLY=1` keeps the earlier shape for comparison: every
//! version stays in `versions`, with only its dist facts moved and the
//! version marked `dist.pnpr.details: "external"`. `PNPR_SPLIT_KEEP_LATEST=0`
//! splits the likely picks too.

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use node_semver::Version;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::LazyLock,
};

/// The split abbreviated form's media type.
pub(super) const SPLIT_CONTENT_TYPE: &str = "application/vnd.pnpr.install-v2+json";

/// The serialized abbreviated size above which versions are split, from
/// `PNPR_SPLIT_METADATA_THRESHOLD` (bytes). Unset, nothing is split: pnpm
/// releases that predate the split form share the metadata cache, and must
/// never read a split document a newer release cached.
static THRESHOLD: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("PNPR_SPLIT_METADATA_THRESHOLD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(usize::MAX)
});

/// Whether the versions a resolution likely picks stay whole.
static KEEP_LATEST: LazyLock<bool> =
    LazyLock::new(|| std::env::var("PNPR_SPLIT_KEEP_LATEST").map_or(true, |value| value != "0"));

/// Whether only the dist facts move, rather than the whole version.
static DIST_ONLY: LazyLock<bool> =
    LazyLock::new(|| std::env::var("PNPR_SPLIT_DIST_ONLY").is_ok_and(|value| value == "1"));

/// Whether the client's `Accept` header offers the split form.
pub(super) fn wants_split(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains(SPLIT_CONTENT_TYPE))
}

/// The split form of an abbreviated packument serialized to `body`.
pub(super) fn split_body(abbreviated: Value, body: Vec<u8>) -> serde_json::Result<Vec<u8>> {
    split_above(abbreviated, body, *THRESHOLD, *DIST_ONLY)
}

/// [`split_body`] with the threshold and the shape given.
fn split_above(
    mut abbreviated: Value,
    body: Vec<u8>,
    threshold: usize,
    dist_only: bool,
) -> serde_json::Result<Vec<u8>> {
    if body.len() <= threshold {
        return Ok(body);
    }
    let inline = if *KEEP_LATEST { likely_picks(&abbreviated) } else { HashSet::new() };
    if dist_only {
        let versions = abbreviated.get_mut("versions").and_then(Value::as_object_mut);
        versions
            .into_iter()
            .flat_map(|versions| versions.iter_mut())
            .filter(|(version, _)| !inline.contains(*version))
            .filter_map(|(_, manifest)| manifest.get_mut("dist").and_then(Value::as_object_mut))
            .for_each(externalize_dist);
    } else {
        move_external_versions(&mut abbreviated, &inline);
    }
    serde_json::to_vec(&abbreviated)
}

/// Move every version not in `inline` from `versions` and `time` to
/// `pnpr.externalVersions`.
fn move_external_versions(abbreviated: &mut Value, inline: &HashSet<String>) {
    let Some(doc) = abbreviated.as_object_mut() else { return };
    let Some(Value::Object(versions)) = doc.remove("versions") else { return };
    let mut time = match doc.remove("time") {
        Some(Value::Object(time)) => time,
        _ => Map::new(),
    };
    let mut kept = Map::new();
    let mut external = Map::new();
    for (version, manifest) in versions {
        if inline.contains(&version) {
            kept.insert(version, manifest);
        } else {
            let published = time.remove(&version).unwrap_or(Value::Null);
            external.insert(version, external_entry(&manifest, published));
        }
    }
    doc.insert("versions".into(), Value::Object(kept));
    if !time.is_empty() {
        doc.insert("time".into(), Value::Object(time));
    }
    doc.insert("pnpr".into(), json!({ "externalVersions": external }));
}

/// An external version's entry: its publish time, or `[time, flags]` when it
/// is deprecated, has trust evidence, or has a revision other than the
/// original selected.
fn external_entry(manifest: &Value, published: Value) -> Value {
    let mut flags = Map::new();
    if let Some(deprecated) = manifest
        .get("deprecated")
        .filter(|value| !value.is_null())
    {
        flags.insert("deprecated".into(), deprecated.clone());
    }
    if let Some(trust) = trust_rank(manifest) {
        flags.insert("trust".into(), Value::String(trust.into()));
    }
    let revision = selected_revision(manifest);
    if revision != 0 {
        flags.insert("revision".into(), Value::from(revision));
    }
    if flags.is_empty() { published } else { json!([published, flags]) }
}

/// The strongest trust evidence a version has, as the trust policy ranks it.
fn trust_rank(manifest: &Value) -> Option<&'static str> {
    let user = manifest.get("_npmUser");
    let flag = |name: &str| {
        user.and_then(|user| user.get(name))
            .is_some_and(|value| value != &Value::Null && value != &Value::Bool(false))
    };
    let provenance = manifest
        .pointer("/dist/attestations/provenance")
        .is_some_and(|value| !value.is_null());
    if flag("approver") {
        Some("stagedPublish")
    } else if flag("trustedPublisher") && provenance {
        Some("trustedPublisher")
    } else if provenance {
        Some("provenance")
    } else {
        None
    }
}

/// The revision a version's manifest is projected from: `0`, the original,
/// unless the registry selected a replacement.
fn selected_revision(manifest: &Value) -> u64 {
    manifest
        .pointer("/dist/revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// A version document's address: `<version>+r<revision>` names one release
/// record, which never changes. A bare version or a tag names whatever the
/// registry currently selects.
pub(super) fn record_address(version_or_tag: &str) -> (&str, Option<u64>) {
    version_or_tag
        .rsplit_once("+r")
        .and_then(|(version, revision)| Some((version, revision.parse().ok()?)))
        .map_or((version_or_tag, None), |(version, revision)| (version, Some(revision)))
}

/// The response for a version's document. Addressed by revision, it is the
/// version's release record: the abbreviated manifest without what may
/// change after publication (`deprecated`) or is evidence kept apart from
/// the record (`_npmUser`, signatures, attestations), served only while that
/// revision is the selected one in this prototype, and cached forever.
/// Otherwise it is today's view of the selected revision.
pub(super) fn version_document_response(manifest: Value, revision: Option<u64>) -> Response {
    let manifest = match revision {
        Some(revision) if revision != selected_revision(&manifest) => return super::not_found(),
        Some(_) => release_record(&manifest),
        None => manifest,
    };
    let body = match serde_json::to_vec(&manifest) {
        Ok(body) => body,
        Err(err) => return super::RegistryError::Json(err).into_response(),
    };
    let mut response = super::packument_bytes_response(body, "application/json", None);
    if revision.is_some() {
        response
            .headers_mut()
            .insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
    }
    response
}

/// A manifest cut down to its release record.
fn release_record(manifest: &Value) -> Value {
    let mut record = pnpr_upstream::abbreviate_version(manifest);
    if let Some(record) = record.as_object_mut() {
        record.remove("deprecated");
        record.remove("_npmUser");
        if let Some(dist) = record.get_mut("dist").and_then(Value::as_object_mut) {
            dist.remove("signatures");
            dist.remove("attestations");
        }
    }
    record
}

/// Every dist-tag target, and the highest release of each major, or of each
/// minor below 1.0, where a caret range stays within one minor.
fn likely_picks(abbreviated: &Value) -> HashSet<String> {
    let mut picks: HashSet<String> = abbreviated
        .get("dist-tags")
        .and_then(Value::as_object)
        .map(|tags| {
            tags.values()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut highest: BTreeMap<(u64, u64), Version> = BTreeMap::new();
    let versions = abbreviated.get("versions").and_then(Value::as_object);
    for key in versions.into_iter().flat_map(|versions| versions.keys()) {
        let Ok(version) = Version::parse(key) else { continue };
        if version.is_prerelease() {
            continue;
        }
        let line = if version.major == 0 { (0, version.minor) } else { (version.major, 0) };
        if highest
            .get(&line)
            .is_none_or(|current| version > *current)
        {
            highest.insert(line, version);
        }
    }
    picks.extend(highest.values().map(Version::to_string));
    picks
}

/// Keep only what resolution reads of a version's dist facts: provenance,
/// for the trust policy, the selected revision, and each variant's tag and
/// platforms.
fn externalize_dist(dist: &mut Map<String, Value>) {
    let attestations = dist.remove("attestations");
    let revision = dist.remove("revision");
    let variants = dist
        .get("pnpr")
        .and_then(|pnpr| pnpr.get("variants"))
        .and_then(Value::as_array)
        .map(|variants| {
            variants
                .iter()
                .map(variant_summary)
                .collect::<Vec<_>>()
        });
    dist.clear();
    if let Some(attestations) = attestations {
        dist.insert("attestations".into(), attestations);
    }
    if let Some(revision) = revision {
        dist.insert("revision".into(), revision);
    }
    let mut pnpr = Map::new();
    pnpr.insert("details".into(), Value::String("external".into()));
    if let Some(variants) = variants {
        pnpr.insert("variants".into(), Value::Array(variants));
    }
    dist.insert("pnpr".into(), Value::Object(pnpr));
}

/// A variant's `tag` and `platforms`, all that resolution reads of it.
fn variant_summary(variant: &Value) -> Value {
    let kept = ["tag", "platforms"]
        .into_iter()
        .filter_map(|field| {
            variant
                .get(field)
                .map(|value| (field.to_string(), value.clone()))
        })
        .collect::<Map<_, _>>();
    Value::Object(kept)
}

#[cfg(test)]
mod tests;
