//! Split packuments (proof of concept). A client that accepts
//! `application/vnd.pnpr.install-v2+json` gets the abbreviated packument,
//! and when that is larger than the threshold, each version's dist facts
//! (integrities, sizes, bins, variant archives) are left in its immutable
//! version document `<base>/<name>/<version>`. Such a version says so with
//! `dist.pnpr.details: "external"`, because an absent `integrity` already
//! means the client computes it while fetching the tarball. The variants'
//! `tag` and `platforms` stay inline.
//!
//! The versions a fresh resolution almost always picks keep their dist facts
//! inline, so most picks need no second request: every dist-tag target and
//! the highest release of each major (of each minor below 1.0). Set
//! `PNPR_SPLIT_KEEP_LATEST=0` to split every version.

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::Response,
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

/// Whether the client's `Accept` header offers the split form.
pub(super) fn wants_split(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains(SPLIT_CONTENT_TYPE))
}

/// The split form of an abbreviated packument serialized to `body`.
pub(super) fn split_body(abbreviated: Value, body: Vec<u8>) -> serde_json::Result<Vec<u8>> {
    split_above(abbreviated, body, *THRESHOLD)
}

/// [`split_body`] with the threshold given.
fn split_above(
    mut abbreviated: Value,
    body: Vec<u8>,
    threshold: usize,
) -> serde_json::Result<Vec<u8>> {
    if body.len() <= threshold {
        return Ok(body);
    }
    let inline = if *KEEP_LATEST { likely_picks(&abbreviated) } else { HashSet::new() };
    if let Some(versions) = abbreviated.get_mut("versions").and_then(Value::as_object_mut) {
        versions
            .iter_mut()
            .filter(|(version, _)| !inline.contains(*version))
            .filter_map(|(_, manifest)| manifest.get_mut("dist").and_then(Value::as_object_mut))
            .for_each(externalize_dist);
    }
    serde_json::to_vec(&abbreviated)
}

/// The response for an exact version's document, which never changes (a
/// revision has its own route), so a split packument's clients and any CDN
/// can keep it forever. A tag's target moves.
pub(super) fn version_document_response(body: Vec<u8>, version_or_tag: &str) -> Response {
    let mut response = super::packument_bytes_response(body, "application/json", None);
    if Version::parse(version_or_tag).is_ok() {
        response
            .headers_mut()
            .insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
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

/// Keep only what resolution reads before a version is picked: provenance,
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
