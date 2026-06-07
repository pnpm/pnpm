//! Local implementation of the npm `/-/v1/search` endpoint.
//!
//! Verdaccio (which this server replaces in `@pnpm/registry-mock`)
//! does **not** proxy search to its upstream npmjs — it scans the
//! local storage and matches on package name. Tests rely on that
//! behavior: `releasing/commands/test/search.ts` asserts that a
//! query for a guaranteed-not-to-exist string returns "No packages
//! found", which an upstream proxy can't guarantee because npm's
//! search returns dozens of fuzzy matches for almost anything.
//!
//! Implementation is intentionally simple: a one-shot scan of
//! `<storage>/<pkg>/package.json` files at request time, filtered
//! by a case-insensitive substring match on `name`. Sufficient for
//! the `@pnpm/registry-mock` fixture (a few dozen packages) and the
//! test queries that exercise it.

use crate::{error::Result, package_name::PackageName, publish::now_iso, storage::Storage};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Parse the `text` query parameter out of a `/-/v1/search?...`
/// query string. npm clients always send `text=...`; we accept
/// `q=...` as a fallback because some older callers use that.
/// Returns `None` for "no text provided", in which case the
/// caller should return an empty result rather than dumping the
/// entire storage.
///
/// Three things this avoids:
/// * The first malformed pair (no `=`) doesn't abort the whole
///   parse — `size=20&text=foo` shouldn't return None just because
///   a third pair somewhere is missing an `=`.
/// * An empty decoded value (`text=`) is treated as "no text",
///   not as "match everything" — a downstream substring filter
///   uses `contains(needle)` which is always true for an empty
///   needle and would dump the entire storage to anonymous
///   callers.
/// * `q=` is a *fallback*: when both `text` and `q` are present
///   `text` wins regardless of order.
pub fn parse_query(query_string: &str) -> Option<String> {
    let mut fallback: Option<String> = None;
    for pair in query_string.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let decoded = percent_decode(value);
        if decoded.is_empty() {
            continue;
        }
        match key {
            "text" => return Some(decoded),
            "q" if fallback.is_none() => fallback = Some(decoded),
            _ => {}
        }
    }
    fallback
}

/// `size=` URL param; bounded the same way npm bounds it (1..=250).
pub fn parse_size(query_string: &str, default_size: usize) -> usize {
    for pair in query_string.split('&') {
        if let Some((key, value)) = pair.split_once('=')
            && key == "size"
            && let Ok(parsed) = value.parse::<usize>()
        {
            return parsed.clamp(1, 250);
        }
    }
    default_size
}

/// Scan the hosted store for packuments whose name contains `query`
/// (case-insensitive). Returns at most `limit` matches in npm search v1
/// shape: `{ objects: [{ package: {...}, score: {...}, searchScore }],
/// total, time }`. Errors reading individual packuments are tolerated —
/// a malformed packument just doesn't match anything. Works against
/// both the local-directory and the S3-backed hosted store.
pub async fn run_local_search(storage: &Storage, query: &str, limit: usize) -> Result<Value> {
    let needle = query.to_lowercase();
    let mut matches: Vec<Value> = Vec::new();
    let mut total: usize = 0;

    for name in storage.hosted_package_names().await? {
        if !name.to_lowercase().contains(&needle) {
            continue;
        }
        total += 1;
        if matches.len() >= limit {
            continue;
        }
        let Ok(parsed) = PackageName::parse(&name) else { continue };
        let Ok(Some(bytes)) = storage.read_hosted_packument(&parsed).await else { continue };
        let Ok(packument) = serde_json::from_slice::<Value>(&bytes) else { continue };
        if let Some(entry) = build_search_entry(&name, &packument) {
            matches.push(entry);
        }
    }

    Ok(json!({
        "objects": matches,
        "total": total,
        "time": now_iso(),
    }))
}

/// Construct one entry for the `objects` array — public because the
/// server's upstream-augment path also needs it when injecting a
/// packument fetched from npm into the local-search response.
pub fn build_search_entry(name: &str, packument: &Value) -> Option<Value> {
    Some(json!({
        "package": build_search_package(name, packument)?,
        "score": {"final": 1.0, "detail": {"quality": 1.0, "popularity": 1.0, "maintenance": 1.0}},
        "searchScore": 1.0,
    }))
}

/// Project a packument into the subset of fields npm's search
/// endpoint returns per result. Pulls the latest version (or any
/// version if there's no `dist-tags.latest`) for `version` /
/// `description` / `keywords`.
fn build_search_package(name: &str, packument: &Value) -> Option<Value> {
    let obj = packument.as_object()?;
    let dist_tags = obj.get("dist-tags").and_then(Value::as_object);
    let latest_tag = dist_tags.and_then(|tags| tags.get("latest")).and_then(Value::as_str);
    let versions = obj.get("versions").and_then(Value::as_object)?;
    let version_id: &str = latest_tag
        .filter(|tag| versions.contains_key(*tag))
        .or_else(|| versions.keys().next().map(String::as_str))?;
    let version_obj = versions.get(version_id).and_then(Value::as_object);
    let mut pkg = Map::new();
    pkg.insert("name".to_string(), Value::String(name.to_string()));
    pkg.insert("version".to_string(), Value::String(version_id.to_string()));
    if let Some(version_obj) = version_obj {
        for field in ["description", "keywords", "author", "maintainers", "homepage"] {
            if let Some(value) = version_obj.get(field) {
                pkg.insert(field.to_string(), value.clone());
            }
        }
    }
    // `time.<version>` if present, else `time.modified` as a fallback.
    if let Some(time) = obj.get("time").and_then(Value::as_object) {
        let date = time.get(version_id).cloned().or_else(|| time.get("modified").cloned());
        if let Some(date) = date {
            pkg.insert("date".to_string(), date);
        }
    }
    // Stable-order publisher block when "_npmUser" is set, to keep
    // diffs deterministic.
    if let Some(npm_user) = version_obj.and_then(|v| v.get("_npmUser")) {
        pkg.insert("publisher".to_string(), npm_user.clone());
    }
    // `links.npm` is what the npm website surfaces. Synthesized
    // from the name so the search response looks the part.
    let mut links = BTreeMap::new();
    links.insert("npm".to_string(), Value::String(format!("https://npmx.dev/package/{name}")));
    pkg.insert("links".to_string(), serde_json::to_value(links).ok()?);
    Some(Value::Object(pkg))
}

fn percent_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut bytes = input.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(' '),
            b'%' => {
                let Some(hi) = bytes.next() else {
                    out.push('%');
                    return out;
                };
                let Some(lo) = bytes.next() else {
                    out.push('%');
                    out.push(hi as char);
                    return out;
                };
                let pair = [hi, lo];
                if let Ok(s) = std::str::from_utf8(&pair)
                    && let Ok(decoded) = u8::from_str_radix(s, 16)
                {
                    out.push(decoded as char);
                } else {
                    out.push('%');
                    out.push(hi as char);
                    out.push(lo as char);
                }
            }
            other => out.push(other as char),
        }
    }
    out
}

#[cfg(test)]
mod tests;
