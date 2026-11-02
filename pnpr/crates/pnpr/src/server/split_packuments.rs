//! pnpr's own package document, `application/vnd.pnpr.install-v2+json`,
//! served only to clients that ask for it
//! ([split packuments](https://github.com/pnpm/rfcs/blob/057a5cb08f430a1824c082ed525bea13520bb4dd/pnpr/text/0000-split-packuments.md)).
//!
//! Every version has an entry with what picking it reads and what may change
//! after publication: its `deprecated` notice, `trust` rank, selected
//! `revision`, and its publish `time` when it falls inside the publish-time
//! window. An entry with a `manifest` is complete: beside the manifest it has
//! pnpr's facts about the archives. When the abbreviated document is larger
//! than the split threshold, only the [likely picks](likely_picks) keep their
//! manifest, and every other version's is in its release record (see
//! [`release_records`]).
//!
//! The document is a function of the stored packument alone, never of the
//! time it is served, so it changes only when the package does.

pub(super) use release_records::{
    record_address, version_document_response, without_pnpr_facts_in,
};

use axum::http::{HeaderMap, header};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use node_semver::Version;
use pnpr_config::PackageDocumentsConfig;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
mod release_records;

/// The media type of pnpr's package document.
pub(super) const INSTALL_V2_CONTENT_TYPE: &str = "application/vnd.pnpr.install-v2+json";

/// The fields of a version's `dist` that pnpr's format keeps at the top of
/// the version's entry and release record.
const DIST_FIELDS: [&str; 3] = ["integrity", "fileCount", "unpackedSize"];

/// The fields of `dist.pnpr` that pnpr's format keeps beside them. npm's
/// documents carry none of them.
const PNPR_FIELDS: [&str; 2] = ["requiresBuild", "bins"];

/// Whether the client's `Accept` header offers pnpr's format.
pub(super) fn wants_install_v2(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains(INSTALL_V2_CONTENT_TYPE))
}

/// The moment the document's publish times are anchored to: the package's
/// last publish. Coarsening the times against it, rather than against the
/// current time, keeps the document unchanged until the next publish.
pub(super) fn document_anchor(packument: &Value) -> Option<DateTime<Utc>> {
    last_publish(
        packument.get("versions").and_then(Value::as_object)?,
        packument.get("time").and_then(Value::as_object)?,
    )
}

/// pnpr's document for an abbreviated packument whose serialized size is
/// `abbreviated_len`.
pub(super) fn install_v2_body(
    abbreviated: &Value,
    abbreviated_len: usize,
    config: &PackageDocumentsConfig,
) -> serde_json::Result<Vec<u8>> {
    let split = u64::try_from(abbreviated_len).is_ok_and(|len| len > config.split_threshold);
    serde_json::to_vec(&install_v2(abbreviated, split, config))
}

/// pnpr's document for an abbreviated packument, with every manifest inline
/// unless `split`, and then only those of the likely picks.
fn install_v2(abbreviated: &Value, split: bool, config: &PackageDocumentsConfig) -> Value {
    let versions = object_field(abbreviated, "versions");
    let window = Duration::from_std(config.publish_time_window).ok();
    let times_since = last_publish(versions, object_field(abbreviated, "time"))
        .zip(window)
        .and_then(|(last, window)| last.checked_sub_signed(window));
    let mut doc = Map::new();
    doc.insert(
        "name".into(),
        abbreviated
            .get("name")
            .cloned()
            .unwrap_or(Value::Null),
    );
    doc.insert("distTags".into(), Value::Object(object_field(abbreviated, "dist-tags").clone()));
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

/// The object `name` of `value`, or an empty one.
fn object_field<'doc>(value: &'doc Value, name: &str) -> &'doc Map<String, Value> {
    static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);
    value
        .get(name)
        .and_then(Value::as_object)
        .unwrap_or(&EMPTY)
}

/// Every version's entry, with its manifest unless `split` leaves it to the
/// release record.
fn version_entries(
    abbreviated: &Value,
    split: bool,
    times_since: Option<&DateTime<Utc>>,
) -> Map<String, Value> {
    let time = object_field(abbreviated, "time");
    let inline = split.then(|| likely_picks(abbreviated));
    object_field(abbreviated, "versions")
        .iter()
        .map(|(version, manifest)| {
            let mut entry = version_entry(manifest, time.get(version), times_since);
            if inline
                .as_ref()
                .is_none_or(|inline| inline.contains(version))
            {
                entry.extend(release_records::release_record(manifest));
            }
            (version.clone(), Value::Object(entry))
        })
        .collect()
}

/// The latest publish time of any listed version.
fn last_publish(versions: &Map<String, Value>, time: &Map<String, Value>) -> Option<DateTime<Utc>> {
    versions
        .keys()
        .filter_map(|version| parse_time(time.get(version)?))
        .max()
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
/// revision, and its publish time when that is inside the window, or `null`
/// when the registry has none.
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

/// Every dist-tag target, and the highest release of each major, or of each
/// minor below 1.0, where a caret range stays within one minor.
fn likely_picks(abbreviated: &Value) -> HashSet<String> {
    let mut picks: HashSet<String> = object_field(abbreviated, "dist-tags")
        .values()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let mut highest: BTreeMap<(u64, u64), (Version, &String)> = BTreeMap::new();
    let releases = object_field(abbreviated, "versions")
        .keys()
        .filter_map(|key| Some((Version::parse(key).ok()?, key)))
        .filter(|(version, _)| !version.is_prerelease());
    for (version, key) in releases {
        let line = if version.major == 0 { (0, version.minor) } else { (version.major, 0) };
        if highest
            .get(&line)
            .is_none_or(|(current, _)| version > *current)
        {
            highest.insert(line, (version, key));
        }
    }
    picks.extend(highest.into_values().map(|(_, key)| key.clone()));
    picks
}

#[cfg(test)]
mod tests;
