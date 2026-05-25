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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use tokio::fs;

use crate::error::Result;
use crate::publish::now_iso;

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

/// Scan `storage` for packuments whose name contains `query` (case-
/// insensitive). Returns at most `limit` matches in npm search v1
/// shape: `{ objects: [{ package: {...}, score: {...}, searchScore }],
/// total, time }`. Errors reading individual packuments are tolerated —
/// a malformed file just doesn't match anything.
pub async fn run_local_search(storage: &Path, query: &str, limit: usize) -> Result<Value> {
    let needle = query.to_lowercase();
    let mut matches: Vec<Value> = Vec::new();
    let mut total: usize = 0;

    for path in collect_packument_paths(storage).await? {
        let Some((name, _scope_dir)) = derive_name(&path, storage) else {
            continue;
        };
        if !name.to_lowercase().contains(&needle) {
            continue;
        }
        total += 1;
        if matches.len() >= limit {
            continue;
        }
        let Ok(bytes) = fs::read(&path).await else { continue };
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

/// Walk the storage tree two levels deep to find `package.json` files.
/// Storage layout is `<root>/<pkg>/package.json` for unscoped and
/// `<root>/@scope/<name>/package.json` for scoped, so a two-level walk
/// suffices and avoids descending into tarball-adjacent junk.
async fn collect_packument_paths(storage: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut top = match fs::read_dir(storage).await {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(err.into()),
    };
    while let Some(entry) = top.next_entry().await? {
        let entry_path = entry.path();
        let entry_name = entry.file_name();
        let name_str = entry_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let unscoped_pkg = entry_path.join("package.json");
        if fs::try_exists(&unscoped_pkg).await.unwrap_or(false) {
            out.push(unscoped_pkg);
            continue;
        }
        if name_str.starts_with('@')
            && let Ok(mut inner) = fs::read_dir(&entry_path).await
        {
            while let Some(child) = inner.next_entry().await? {
                let scoped_pkg = child.path().join("package.json");
                if fs::try_exists(&scoped_pkg).await.unwrap_or(false) {
                    out.push(scoped_pkg);
                }
            }
        }
    }
    Ok(out)
}

/// Reconstruct `<scope>/<name>` (or `<name>`) from the packument
/// path. We deliberately don't trust the on-disk `name` field —
/// the directory layout is the source of truth, same as verdaccio.
fn derive_name(packument_path: &Path, storage: &Path) -> Option<(String, bool)> {
    let relative = packument_path.strip_prefix(storage).ok()?;
    let mut components: Vec<&str> =
        relative.components().filter_map(|component| component.as_os_str().to_str()).collect();
    components.pop()?; // drop "package.json"
    match components.as_slice() {
        [name] => Some(((*name).to_string(), false)),
        [scope, name] if scope.starts_with('@') => Some((format!("{scope}/{name}"), true)),
        _ => None,
    }
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
mod tests {
    use super::{parse_query, parse_size};

    #[test]
    fn parses_text_query() {
        assert_eq!(parse_query("text=is-positive&size=20").as_deref(), Some("is-positive"));
        assert_eq!(parse_query("size=20&text=foo").as_deref(), Some("foo"));
        assert_eq!(parse_query("text=hello%20world").as_deref(), Some("hello world"));
        assert_eq!(parse_query("text=hi+there").as_deref(), Some("hi there"));
        assert_eq!(parse_query("text=%40scope%2Fname").as_deref(), Some("@scope/name"));
    }

    #[test]
    fn parses_q_fallback() {
        assert_eq!(parse_query("q=foo").as_deref(), Some("foo"));
    }

    #[test]
    fn text_overrides_q_regardless_of_order() {
        assert_eq!(parse_query("q=fallback&text=primary").as_deref(), Some("primary"));
        assert_eq!(parse_query("text=primary&q=fallback").as_deref(), Some("primary"));
    }

    #[test]
    fn no_query() {
        assert!(parse_query("").is_none());
        assert!(parse_query("size=20").is_none());
    }

    #[test]
    fn empty_text_is_no_query() {
        // An empty needle would make `contains("")` true downstream and
        // dump every package in storage. Treat `text=` the same as no
        // text at all.
        assert!(parse_query("text=").is_none());
        assert!(parse_query("text=&size=20").is_none());
    }

    #[test]
    fn malformed_pair_doesnt_abort_parse() {
        // A pair with no `=` (e.g. trailing `&` or an unkeyed value)
        // used to short-circuit the whole parse with `?`. Now we just
        // skip it.
        assert_eq!(parse_query("flag&text=foo").as_deref(), Some("foo"));
        assert_eq!(parse_query("text=foo&trailing").as_deref(), Some("foo"));
    }

    #[test]
    fn size_clamps() {
        assert_eq!(parse_size("size=10", 20), 10);
        assert_eq!(parse_size("size=0", 20), 1);
        assert_eq!(parse_size("size=9999", 20), 250);
        assert_eq!(parse_size("size=garbage", 20), 20);
        assert_eq!(parse_size("", 20), 20);
    }
}
