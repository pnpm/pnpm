//! Byte-for-byte port of the YAML dumper pnpm uses for `pnpm-lock.yaml`.
//!
//! pnpm serializes its lockfile with [`@zkochan/js-yaml`], a fork of `js-yaml`
//! carrying lockfile-specific rendering rules that no general-purpose Rust YAML
//! serializer reproduces:
//!
//! - **`blankLines`** — a blank line separates the entries of the top-level map
//!   and of the `packages:` / `importers:` / `snapshots:` maps.
//! - **single-line keys** — `cpu`, `engines`, `os`, `libc`, and `resolution`
//!   (unless its `type` is `variations`/`binary`) render in flow style on one
//!   line; everything else renders in block style.
//! - **single-quote scalar style** — ambiguous scalars (`'9.0'`, `'>=10'`,
//!   `'@scope/name@1.0.0'`) are single-quoted, matching `js-yaml`'s default
//!   `quotingType`, not double-quoted.
//! - **`lineWidth: -1`, `noRefs: true`, `noCompatMode: true`** — no line
//!   wrapping, no anchors/aliases, no legacy-bool/base-60 quoting.
//!
//! The input is first lowered to a [`serde_json::Value`] (the crate enables
//! `serde_json/preserve_order`, so map order is retained), then its keys are
//! reordered by [`sort_lockfile_keys`] to match pnpm's lockfile key sort,
//! so the byte output is independent of pacquet's struct field order — and
//! finally rendered by a faithful translation of the fork's `dumper.js`.
//!
//! [`@zkochan/js-yaml`]: https://github.com/pnpm/js-yaml

use rayon::prelude::*;
use serde_json::{Map, Value};
use std::cmp::Ordering;

/// Entry count from which a map's independent per-entry work (deep key
/// sorting, block rendering) fans out across the rayon pool. In
/// practice only a workspace's `importers:` / `packages:` /
/// `snapshots:` sections grow past this; the small maps nested inside
/// every package entry stay serial, where the fan-out's fixed cost
/// would dominate.
const PARALLEL_ENTRY_THRESHOLD: usize = 64;

/// Keys whose collection value always renders on a single line (flow style).
/// Mirrors the fork's [`SINGLE_LINE_KEYS`][fork-single-line-keys].
///
/// [fork-single-line-keys]: https://cdn.jsdelivr.net/npm/@zkochan/js-yaml@0.0.11/lib/dumper.js
const SINGLE_LINE_KEYS: [&str; 4] = ["cpu", "engines", "os", "libc"];

/// One indentation level, in spaces (`js-yaml`'s default `indent`).
const INDENT: usize = 2;

/// Per-package / per-snapshot key priority, matching pnpm's lockfile
/// key sort.
const ORDERED_KEYS: [&str; 20] = [
    "resolution",
    "id",
    "name",
    "version",
    "engines",
    "cpu",
    "os",
    "libc",
    "deprecated",
    "hasBin",
    "prepare",
    "requiresBuild",
    "bundleDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "dependencies",
    "optionalDependencies",
    "transitivePeerDependencies",
    "dev",
    "optional",
];

/// Top-level key priority, matching pnpm's lockfile root-key sort.
const ROOT_KEYS: [&str; 9] = [
    "lockfileVersion",
    "settings",
    "catalogs",
    "overrides",
    "packageExtensionsChecksum",
    "pnpmfileChecksum",
    "patchedDependencies",
    "importers",
    "packages",
];

/// Render an already-normalized lockfile document to a YAML string
/// matching pnpm's lockfile formatting.
pub(crate) fn to_string(value: Value) -> String {
    let value = sort_lockfile_keys(value);
    let mut dump = render(&value, 0, true, true, None, false);
    dump.push('\n');
    pnpm_fs::background_drop(value);
    dump
}

/// Reorder a lockfile document's keys to match pnpm's on-write ordering:
/// `importers` / `packages` / `snapshots` / `catalogs` / `time` /
/// `patchedDependencies` are sorted by their direct keys, each section's
/// entries are deep-sorted (by the priority map for packages/snapshots, by the
/// root priority for importers, lexically for catalogs), and finally the root
/// keys are ordered by priority.
fn sort_lockfile_keys(value: Value) -> Value {
    let Value::Object(mut root) = value else { return value };

    for (section, priority) in [
        ("importers", &ROOT_KEYS[..]),
        ("packages", &ORDERED_KEYS[..]),
        ("snapshots", &ORDERED_KEYS[..]),
    ] {
        sort_entries_by_priority(&mut root, section, priority);
    }

    if let Some(Value::Object(catalogs)) = root.remove("catalogs") {
        let sorted = map_values(sort_direct_keys(catalogs), sort_deep_keys);
        root.insert("catalogs".to_string(), Value::Object(sorted));
    }

    for section in ["time", "patchedDependencies"] {
        if let Some(Value::Object(map)) = root.remove(section) {
            root.insert(section.to_string(), Value::Object(sort_direct_keys(map)));
        }
    }

    Value::Object(sort_by_priority(root, &ROOT_KEYS, false))
}

/// Sort one section's direct keys, then each entry's own keys by `priority`.
fn sort_entries_by_priority(root: &mut Map<String, Value>, section: &str, priority: &[&str]) {
    let Some(Value::Object(map)) = root.remove(section) else {
        return;
    };
    let sorted = map_values(sort_direct_keys(map), |entry| match entry {
        Value::Object(inner) => Value::Object(sort_by_priority(inner, priority, true)),
        other => other,
    });
    root.insert(section.to_string(), Value::Object(sorted));
}

/// Plain code-unit key comparison.
fn lex_cmp(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}

/// Prioritized keys come first in priority order, the rest follow in
/// plain code-unit order.
fn priority_cmp(priority: &[&str], left: &str, right: &str) -> Ordering {
    let rank = |key: &str| priority.iter().position(|entry| *entry == key);
    match (rank(left), rank(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => lex_cmp(left, right),
    }
}

fn map_values(
    map: Map<String, Value>,
    transform: impl Fn(Value) -> Value + Sync,
) -> Map<String, Value> {
    if map.len() < PARALLEL_ENTRY_THRESHOLD {
        return map.into_iter().map(|(key, value)| (key, transform(value))).collect();
    }
    let entries: Vec<_> = map.into_iter().collect();
    let transformed: Vec<_> =
        entries.into_par_iter().map(|(key, value)| (key, transform(value))).collect();
    transformed.into_iter().collect()
}

fn sort_direct_keys(map: Map<String, Value>) -> Map<String, Value> {
    sort_map(map, &lex_cmp, false)
}

fn sort_deep_keys(value: Value) -> Value {
    sort_value(value, &lex_cmp)
}

fn sort_by_priority(map: Map<String, Value>, priority: &[&str], deep: bool) -> Map<String, Value> {
    sort_map(map, &|left, right| priority_cmp(priority, left, right), deep)
}

fn sort_map(
    map: Map<String, Value>,
    compare: &dyn Fn(&str, &str) -> Ordering,
    deep: bool,
) -> Map<String, Value> {
    let mut entries: Vec<(String, Value)> = map.into_iter().collect();
    entries.sort_by(|(left, _), (right, _)| compare(left, right));
    entries
        .into_iter()
        .map(|(key, value)| (key, if deep { sort_value(value, compare) } else { value }))
        .collect()
}

/// Recursively sort the keys of every nested object with `compare`, recursing
/// through arrays without reordering their elements. Mirrors the `deep` option
/// of the `sort-keys` package pnpm uses.
fn sort_value(value: Value, compare: &dyn Fn(&str, &str) -> Ordering) -> Value {
    match value {
        Value::Object(map) => Value::Object(sort_map(map, compare, true)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|item| sort_value(item, compare)).collect())
        }
        other => other,
    }
}

/// Render one node. Mirrors the fork's `writeNode`.
///
/// - `level` — current indentation depth.
/// - `block` — whether block style is permitted here (`false` inside flow).
/// - `compact` — whether the first child of a block collection omits its
///   leading newline (the collection sits on the same line as its key).
/// - `object_key` — the map key this value is bound to, driving the
///   single-line and blank-line decisions.
/// - `force_single_line` — propagated `singleLineOnly`: forces single-line
///   scalar styling for keys and for values nested in a single-line map.
fn render(
    value: &Value,
    level: usize,
    block: bool,
    compact: bool,
    object_key: Option<&str>,
    force_single_line: bool,
) -> String {
    match value {
        Value::Object(map) => render_map(map, level, block, compact, object_key),
        Value::Array(seq) => {
            let single_line = object_key.is_some_and(is_single_line_key);
            if block && !seq.is_empty() && !single_line {
                write_block_sequence(seq, level, compact)
            } else {
                write_flow_sequence(seq, level)
            }
        }
        Value::String(string) => write_scalar(string, level, force_single_line, block),
        Value::Bool(boolean) => if *boolean { "true" } else { "false" }.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Null => "null".to_string(),
    }
}

/// The lockfile's top-level sections and their entries are separated by a
/// blank line; everything nested inside them is not.
fn render_map(
    map: &serde_json::Map<String, Value>,
    level: usize,
    block: bool,
    compact: bool,
    object_key: Option<&str>,
) -> String {
    let single_line = is_single_line_map(object_key, map);
    if !block || map.is_empty() || single_line {
        return write_flow_mapping(map, level, single_line);
    }
    let double_line =
        level == 0 || matches!(object_key, Some("packages" | "importers" | "snapshots"));
    write_block_mapping(map, level, compact, double_line)
}

fn is_single_line_key(key: &str) -> bool {
    SINGLE_LINE_KEYS.contains(&key)
}

/// Whether a map value renders on a single line. `resolution` is single-line
/// except for the nested `variations`/`binary` shapes (detected by the `type`
/// discriminator pnpm's tagged resolutions carry).
fn is_single_line_map(object_key: Option<&str>, map: &serde_json::Map<String, Value>) -> bool {
    match object_key {
        Some(key) if is_single_line_key(key) => true,
        Some("resolution") => {
            !matches!(map.get("type").and_then(Value::as_str), Some("variations" | "binary"))
        }
        _ => false,
    }
}

/// `generateNextLine`: a newline (doubled when `double_line`) plus this level's
/// indent.
fn next_line(level: usize, double_line: bool) -> String {
    let mut line = String::from("\n");
    if double_line {
        line.push('\n');
    }
    line.extend(std::iter::repeat_n(' ', INDENT * level));
    line
}

/// A rendered key longer than this (measured in UTF-16 code units,
/// matching js-yaml's `state.dump.length > 1024`) is emitted as an
/// explicit `? <key>` / `: <value>` pair: YAML caps a *simple* key at
/// 1024 characters, so an inline key of that length would not re-parse.
const EXPLICIT_KEY_THRESHOLD: usize = 1024;

fn write_block_mapping(
    map: &serde_json::Map<String, Value>,
    level: usize,
    compact: bool,
    double_line: bool,
) -> String {
    if map.len() < PARALLEL_ENTRY_THRESHOLD {
        return write_block_mapping_serial(map, level, compact, double_line);
    }
    // Each entry's rendering depends only on its own key and value, so a
    // large map fans its entries out across the rayon pool, and the serial
    // stitch below applies the only order-dependent rule — the first entry
    // of a compact block omits its leading newline.
    let pairs: Vec<_> = map.iter().collect();
    let entries: Vec<String> = pairs
        .par_iter()
        .map(|(key, value)| {
            let mut entry = String::new();
            render_entry_into(&mut entry, key, value, level);
            entry
        })
        .collect();
    let mut result = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if !compact || index > 0 {
            result.push_str(&next_line(level, double_line));
        }
        result.push_str(entry);
    }
    if result.is_empty() { "{}".to_string() } else { result }
}

/// Small maps — the nested ones inside every package entry, above all —
/// append straight into one buffer, chunk-free.
fn write_block_mapping_serial(
    map: &serde_json::Map<String, Value>,
    level: usize,
    compact: bool,
    double_line: bool,
) -> String {
    let mut result = String::new();
    for (key, value) in map {
        if !compact || !result.is_empty() {
            result.push_str(&next_line(level, double_line));
        }
        render_entry_into(&mut result, key, value, level);
    }
    if result.is_empty() { "{}".to_string() } else { result }
}

fn render_entry_into(result: &mut String, key: &str, value: &Value, level: usize) {
    let rendered_key = write_scalar(key, level + 1, true, true);
    let explicit_pair = rendered_key.encode_utf16().count() > EXPLICIT_KEY_THRESHOLD;
    if explicit_pair {
        result.push_str("? ");
        result.push_str(&rendered_key);
        result.push_str(&next_line(level, false));
    } else {
        result.push_str(&rendered_key);
    }
    let rendered = render(value, level + 1, true, explicit_pair, Some(key), false);
    result.push(':');
    if !rendered.starts_with('\n') {
        result.push(' ');
    }
    result.push_str(&rendered);
}

fn write_block_sequence(seq: &[Value], level: usize, compact: bool) -> String {
    let mut result = String::new();
    for value in seq {
        let rendered = render(value, level + 1, true, true, None, false);
        if !compact || !result.is_empty() {
            result.push_str(&next_line(level, false));
        }
        result.push('-');
        if !rendered.starts_with('\n') {
            result.push(' ');
        }
        result.push_str(&rendered);
    }
    if result.is_empty() { "[]".to_string() } else { result }
}

fn write_flow_mapping(
    map: &serde_json::Map<String, Value>,
    level: usize,
    single_line: bool,
) -> String {
    let mut result = String::new();
    for (key, value) in map {
        if !result.is_empty() {
            result.push_str(", ");
        }
        result.push_str(&write_scalar(key, level, single_line, false));
        result.push_str(": ");
        result.push_str(&render(value, level, false, false, None, single_line));
    }
    format!("{{{result}}}")
}

fn write_flow_sequence(seq: &[Value], level: usize) -> String {
    let mut result = String::new();
    for value in seq {
        if !result.is_empty() {
            result.push_str(", ");
        }
        result.push_str(&render(value, level, false, false, None, false));
    }
    format!("[{result}]")
}

impl TimestampScan<'_> {
    /// `[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}`
    fn date(&mut self) -> bool {
        self.digits(4, 4)
            && self.byte(b'-')
            && self.digits(1, 2)
            && self.byte(b'-')
            && self.digits(1, 2)
    }

    fn byte(&mut self, expected: u8) -> bool {
        if self.bytes.get(self.index) != Some(&expected) {
            return false;
        }
        self.index += 1;
        true
    }

    /// Consume between `min` and `max` digits, reporting whether at least
    /// `min` were there.
    fn digits(&mut self, min: usize, max: usize) -> bool {
        let start = self.index;
        while self.index < self.bytes.len()
            && self.index - start < max
            && self.bytes[self.index].is_ascii_digit()
        {
            self.index += 1;
        }
        self.index - start >= min
    }

    /// `(?:[Tt]|[ \t]+)`
    fn time_separator(&mut self) -> bool {
        match self.bytes.get(self.index) {
            Some(b'T' | b't') => {
                self.index += 1;
                true
            }
            Some(b' ' | b'\t') => {
                self.skip_spaces();
                true
            }
            _ => false,
        }
    }

    fn skip_spaces(&mut self) {
        while matches!(self.bytes.get(self.index), Some(b' ' | b'\t')) {
            self.index += 1;
        }
    }

    /// `[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?`
    fn time(&mut self) -> bool {
        if !(self.digits(1, 2)
            && self.byte(b':')
            && self.digits(2, 2)
            && self.byte(b':')
            && self.digits(2, 2))
        {
            return false;
        }
        if self.byte(b'.') {
            self.digits(0, usize::MAX);
        }
        true
    }

    /// `(?:[ \t]*(Z|([-+])([0-9][0-9]?)(?::([0-9][0-9]))?))?`, and nothing
    /// after it.
    fn timezone(&mut self) -> bool {
        self.skip_spaces();
        if self.index == self.bytes.len() {
            return true;
        }
        match self.bytes.get(self.index) {
            Some(b'Z') => self.index += 1,
            Some(b'-' | b'+') => {
                self.index += 1;
                if !self.digits(1, 2) {
                    return false;
                }
                if self.byte(b':') && !self.digits(2, 2) {
                    return false;
                }
            }
            _ => return false,
        }
        self.index == self.bytes.len()
    }
}

#[cfg(test)]
mod tests;

mod implicit;
use implicit::{TimestampScan, resolves_implicitly};

mod scalars;
use scalars::write_scalar;
