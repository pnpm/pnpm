//! Split packuments (proof of concept). A client that accepts
//! `application/vnd.pnpr.install-v2+json` gets the abbreviated packument,
//! and when that is larger than the threshold, each version keeps only what
//! picking a version reads: its `deprecated` notice, the `_npmUser` trust
//! flags, and its provenance. The rest of the version (dependencies,
//! integrities, bins, variant archives) is in its immutable version document
//! `<base>/<name>/<version>`. Such a version says so with
//! `dist.pnpr.details: "external"`, because an absent `integrity` already
//! means the client computes it while fetching the tarball.
//! `PNPR_SPLIT_DIST_ONLY=1` moves only each version's dist facts, keeping its
//! dependencies and its variants' `tag` and `platforms` inline.
//!
//! The versions a fresh resolution almost always picks keep their dist facts
//! inline, so most picks need no second request: every dist-tag target and
//! the highest release of each major (of each minor below 1.0). Set
//! `PNPR_SPLIT_KEEP_LATEST=0` to split every version.

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use node_semver::Version;
use serde_json::{Map, Value};
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

/// Whether the versions a resolution likely picks keep their dist inline.
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
    if let Some(versions) = abbreviated.get_mut("versions").and_then(Value::as_object_mut) {
        versions
            .iter_mut()
            .filter(|(version, _)| !inline.contains(*version))
            .filter_map(|(_, manifest)| manifest.as_object_mut())
            .for_each(|manifest| externalize(manifest, dist_only));
    }
    serde_json::to_vec(&abbreviated)
}

/// Leave a version, or only its dist facts, to its version document.
fn externalize(manifest: &mut Map<String, Value>, dist_only: bool) {
    if !dist_only {
        externalize_version(manifest);
    } else if let Some(dist) = manifest.get_mut("dist").and_then(Value::as_object_mut) {
        externalize_dist(dist);
    }
}

/// The response for a version's document. A split packument's client reads
/// the version as the abbreviated packument would list it, so the shape
/// follows `Accept` and the response varies by it. An exact version's
/// document never changes (a revision has its own route), so clients and any
/// CDN can keep it forever; a tag's target moves.
pub(super) fn version_document_response(
    manifest: Value,
    headers: &HeaderMap,
    version_or_tag: &str,
) -> Response {
    let split = wants_split(headers);
    let manifest = if split { pnpr_upstream::abbreviate_version(&manifest) } else { manifest };
    let body = match serde_json::to_vec(&manifest) {
        Ok(body) => body,
        Err(err) => return super::RegistryError::Json(err).into_response(),
    };
    let mut response = super::packument_bytes_response(body, "application/json", None);
    let headers = response.headers_mut();
    if Version::parse(version_or_tag).is_ok() {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    if split {
        headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    }
    response
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

/// Keep only what picking a version reads: its name and version, its
/// `deprecated` notice, the `_npmUser` trust flags, and its provenance.
fn externalize_version(manifest: &mut Map<String, Value>) {
    manifest.retain(|field, _| {
        matches!(field.as_str(), "name" | "version" | "deprecated" | "_npmUser" | "dist")
    });
    let attestations = manifest
        .get_mut("dist")
        .and_then(Value::as_object_mut)
        .and_then(|dist| dist.remove("attestations"));
    let mut dist = Map::new();
    if let Some(attestations) = attestations {
        dist.insert("attestations".into(), attestations);
    }
    dist.insert("pnpr".into(), serde_json::json!({ "details": "external" }));
    manifest.insert("dist".into(), Value::Object(dist));
}

/// Keep only what resolution reads of a version's dist facts: provenance,
/// for the trust policy, and each variant's tag and platforms.
fn externalize_dist(dist: &mut Map<String, Value>) {
    let attestations = dist.remove("attestations");
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
