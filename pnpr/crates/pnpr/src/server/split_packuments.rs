//! The `application/vnd.pnpr.install-v2+json` package document (proof of
//! concept). It is pnpr's own format, served only to clients that ask for it:
//!
//! ```json
//! { "name": "acme", "distTags": { "latest": "2.1.0" }, "timesSince": "...",
//!   "versions": { "1.0.0": { "deprecated": "use 2" },
//!                 "2.1.0": { "time": "...", "trust": "provenance", "manifest": { ... } } } }
//! ```
//!
//! Every version has an entry with what picking it reads and what may change
//! after publication: its `deprecated` notice, `trust` rank, selected
//! `revision` when it is not the original, and its publish `time` if it was
//! published within [`TIME_WINDOW_DAYS`] (30 by default) of the package's last
//! publish. `timesSince` is where that window starts, so an entry without a
//! time was published before it. An entry with a `manifest` is complete. When
//! the abbreviated document is larger than the threshold, only the versions a
//! fresh resolution almost always picks keep their manifest: every dist-tag
//! target and the highest release of each major (of each minor below 1.0).
//! The manifest of any version is its release record, the immutable version
//! document `<base>/<name>/<version>+r<revision>`.
//!
//! `PNPR_SPLIT_KEEP_LATEST=0` leaves out the manifests of the likely picks
//! too.

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use node_semver::Version;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::LazyLock,
};

/// The media type of pnpr's package document.
pub(super) const SPLIT_CONTENT_TYPE: &str = "application/vnd.pnpr.install-v2+json";

/// The serialized abbreviated size above which only the likely picks keep
/// their manifests, from `PNPR_SPLIT_METADATA_THRESHOLD` (bytes, 64 KiB by
/// default). `PNPR_INSTALL_V2=0` turns the format off, for comparison.
static THRESHOLD: LazyLock<Option<usize>> = LazyLock::new(|| {
    if std::env::var("PNPR_INSTALL_V2").is_ok_and(|value| value == "0") {
        return None;
    }
    let threshold = std::env::var("PNPR_SPLIT_METADATA_THRESHOLD").ok();
    Some(
        threshold
            .and_then(|value| value.parse().ok())
            .unwrap_or(64 * 1024),
    )
});

/// The fields of a version's `dist` that pnpr's format keeps, at the top of
/// the version's entry and release record.
const DIST_FIELDS: [&str; 3] = ["integrity", "fileCount", "unpackedSize"];

/// The fields of `dist.pnpr` that pnpr's format keeps, beside them. npm's
/// documents carry none of them.
const PNPR_FIELDS: [&str; 3] = ["requiresBuild", "bins", "variants"];

/// Whether the versions a resolution likely picks keep their manifests.
static KEEP_LATEST: LazyLock<bool> =
    LazyLock::new(|| std::env::var("PNPR_SPLIT_KEEP_LATEST").map_or(true, |value| value != "0"));

/// How far back from a package's last publish the versions keep their
/// publish times, from `PNPR_SPLIT_TIME_WINDOW_DAYS`. pnpm accepts a minimum
/// release age of at most 30 days, so the window is never shorter: a version
/// older than it is old enough for any of them.
static TIME_WINDOW_DAYS: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("PNPR_SPLIT_TIME_WINDOW_DAYS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or(MIN_TIME_WINDOW_DAYS, |days: i64| days.max(MIN_TIME_WINDOW_DAYS))
});

/// The longest minimum release age pnpm accepts, in days.
const MIN_TIME_WINDOW_DAYS: i64 = 30;

/// Whether the client's `Accept` header offers pnpr's format, and it is
/// served.
pub(super) fn wants_split(headers: &HeaderMap) -> bool {
    THRESHOLD.is_some()
        && headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|accept| accept.contains(SPLIT_CONTENT_TYPE))
}

/// pnpr's document for an abbreviated packument serialized to `body`.
pub(super) fn split_body(abbreviated: &Value, body: &[u8]) -> serde_json::Result<Vec<u8>> {
    let threshold = THRESHOLD.unwrap_or(usize::MAX);
    serde_json::to_vec(&install_v2(abbreviated, body.len() > threshold))
}

/// pnpr's document for an abbreviated packument, with every manifest inline
/// unless `split`, and then only those of the likely picks.
fn install_v2(abbreviated: &Value, split: bool) -> Value {
    let empty = Map::new();
    let field = |name: &str| {
        abbreviated
            .get(name)
            .and_then(Value::as_object)
            .unwrap_or(&empty)
    };
    let times_since = times_since(field("versions"), field("time"));
    let mut doc = Map::new();
    doc.insert(
        "name".into(),
        abbreviated
            .get("name")
            .cloned()
            .unwrap_or(Value::Null),
    );
    doc.insert("distTags".into(), Value::Object(field("dist-tags").clone()));
    if let Some(modified) = abbreviated.get("modified") {
        doc.insert("modified".into(), modified.clone());
    }
    if let Some(times_since) = &times_since {
        let since = times_since.to_rfc3339_opts(SecondsFormat::Millis, true);
        doc.insert("timesSince".into(), Value::String(since));
    }
    let entries = version_entries(abbreviated, split, times_since.as_ref());
    doc.insert("versions".into(), Value::Object(entries));
    Value::Object(doc)
}

/// Every version's entry, with its manifest unless `split` leaves it to the
/// release record.
fn version_entries(
    abbreviated: &Value,
    split: bool,
    times_since: Option<&DateTime<Utc>>,
) -> Map<String, Value> {
    let empty = Map::new();
    let field = |name: &str| {
        abbreviated
            .get(name)
            .and_then(Value::as_object)
            .unwrap_or(&empty)
    };
    let inline = (split && *KEEP_LATEST).then(|| likely_picks(abbreviated));
    field("versions")
        .iter()
        .map(|(version, manifest)| {
            let mut entry = version_entry(manifest, field("time").get(version), times_since);
            if !split
                || inline
                    .as_ref()
                    .is_some_and(|inline| inline.contains(version))
            {
                entry.extend(release_record(manifest));
            }
            (version.clone(), Value::Object(entry))
        })
        .collect()
}

/// Where the publish-time window starts: [`TIME_WINDOW_DAYS`] before the
/// package's last publish. It moves only when the package is published to,
/// so the document does not change as time passes.
fn times_since(versions: &Map<String, Value>, time: &Map<String, Value>) -> Option<DateTime<Utc>> {
    let last = versions
        .keys()
        .filter_map(|version| parse_time(time.get(version)?))
        .max()?;
    Some(last - Duration::days(*TIME_WINDOW_DAYS))
}

/// A publish time as the abbreviated document gives it: RFC 3339, or
/// coarsened to the minute (`2026-10-31T10:05Z`) or the day (`2026-10-31`).
fn parse_time(value: &Value) -> Option<DateTime<Utc>> {
    let value = value.as_str()?;
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%MZ") {
        return Some(parsed.and_utc());
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc())
}

/// A version's entry: its `deprecated` notice, trust rank, and selected
/// revision when it is not the original, and its publish time when that is
/// inside the window, or `null` when the registry has none.
fn version_entry(
    manifest: &Value,
    published: Option<&Value>,
    times_since: Option<&DateTime<Utc>>,
) -> Map<String, Value> {
    let mut entry = Map::new();
    match published.map(|value| (value, parse_time(value))) {
        Some((_, Some(at))) if times_since.is_some_and(|since| at < *since) => {}
        Some((value, Some(_))) => {
            entry.insert("time".into(), value.clone());
        }
        _ => {
            entry.insert("time".into(), Value::Null);
        }
    }
    if let Some(deprecated) = manifest
        .get("deprecated")
        .filter(|value| !value.is_null())
    {
        entry.insert("deprecated".into(), deprecated.clone());
    }
    if let Some(trust) = trust_rank(manifest) {
        entry.insert("trust".into(), Value::String(trust.into()));
    }
    // A revision-aware version is marked even at revision 0: its tar is at
    // `<name>@<version>+r<revision>` and its integrity is the tar's.
    if let Some(revision) = manifest.pointer("/dist/revision") {
        entry.insert("revision".into(), revision.clone());
    }
    entry
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
pub(super) fn version_document_response(mut manifest: Value, revision: Option<u64>) -> Response {
    let manifest = match revision {
        Some(revision) if revision != selected_revision(&manifest) => {
            match revision_from_history(&manifest, revision) {
                Some(manifest) => manifest,
                None => return super::not_found(),
            }
        }
        Some(_) => named_record(&manifest, &manifest),
        None => {
            without_pnpr_facts(&mut manifest);
            manifest
        }
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

/// A version's release record, without its `name` and `version`: pnpr's
/// facts about its archives, and under `manifest` the publisher's manifest
/// without `dist` and without what may change after publication
/// (`deprecated`) or is evidence kept apart from the record (`_npmUser`).
fn release_record(manifest: &Value) -> Map<String, Value> {
    let mut fields = match pnpr_upstream::abbreviate_version(manifest) {
        Value::Object(fields) => fields,
        _ => Map::new(),
    };
    for field in ["name", "version", "deprecated", "_npmUser"] {
        fields.remove(field);
    }
    let dist = match fields.remove("dist") {
        Some(Value::Object(dist)) => dist,
        _ => Map::new(),
    };
    let pnpr = dist.get("pnpr").and_then(Value::as_object);
    let mut record = Map::new();
    for field in DIST_FIELDS {
        if let Some(value) = dist.get(field) {
            record.insert(field.into(), value.clone());
        }
    }
    for field in PNPR_FIELDS {
        if let Some(value) = pnpr.and_then(|pnpr| pnpr.get(field)) {
            record.insert(field.into(), value.clone());
        }
    }
    record.insert("manifest".into(), Value::Object(fields));
    record
}

/// The release record of a revision other than the selected one, from the
/// version's revision history: its manifest and its tar's integrity.
fn revision_from_history(manifest: &Value, revision: u64) -> Option<Value> {
    let entry = manifest
        .pointer("/dist/revisions")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("revision").and_then(Value::as_u64) == Some(revision))?;
    let mut fields = entry
        .get("manifest")?
        .as_object()?
        .clone();
    let mut dist = Map::new();
    dist.insert("integrity".into(), entry.get("integrity")?.clone());
    fields.insert("dist".into(), Value::Object(dist));
    Some(named_record(manifest, &Value::Object(fields)))
}

/// The release record of `manifest`, named by the version `named` is of.
fn named_record(named: &Value, manifest: &Value) -> Value {
    let mut record = Map::new();
    for field in ["name", "version"] {
        record.insert(
            field.into(),
            named
                .get(field)
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    record.extend(release_record(manifest));
    Value::Object(record)
}

/// Remove pnpm's extensions from a version manifest served in npm's format:
/// pnpr's facts about the archives, the variants it stores with a hosted
/// version, and the revision protocol's marker and history. The selected
/// revision's `tarball` and `integrity` stay, so npm clients install it.
pub(super) fn without_pnpr_facts(manifest: &mut Value) {
    let Some(fields) = manifest.as_object_mut() else { return };
    fields.remove(super::publishing::variants::STORED_FIELD);
    if let Some(dist) = fields.get_mut("dist").and_then(Value::as_object_mut) {
        for field in ["pnpr", "revision", "revisions"] {
            dist.remove(field);
        }
    }
}

/// Remove pnpr's facts from every version of a document served in npm's
/// format.
pub(super) fn without_pnpr_facts_in(doc: &mut Value) {
    let versions = doc.get_mut("versions").and_then(Value::as_object_mut);
    versions
        .into_iter()
        .flat_map(|versions| versions.values_mut())
        .for_each(without_pnpr_facts);
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

#[cfg(test)]
mod tests;
